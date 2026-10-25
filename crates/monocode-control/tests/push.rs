//! Following topics: subscriptions, events between calls, following again
//! after the app restarts, and reading a frame across a read timeout.
#[cfg(test)]
pub mod common;

use monocode_control::{
    Client, Endpoint, ErrorCode, Grant, Notice, PROTOCOL_VERSION, Scope, Topic,
    frame::{
        self, ClientFrame, Event, Lines, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Outcome, Reply,
        ServerFrame, Welcome,
    },
    ops::project::{ProjectList, ProjectListInput},
    transport::{Connection, Listener},
};
use serde_json::json;
use std::{
    io::{self, BufReader, Read},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    /// Frames the fake app received, per connection.
    pub(super) type Seen = Arc<Mutex<Vec<Vec<ClientFrame>>>>;

    /// A fake app. It welcomes with `push`, answers every subscription and
    /// then sends one event per topic, answers `project_list`, and hangs up
    /// a connection after `frames` frames (hello included).
    pub(super) fn app(endpoint: &Endpoint, push: bool, frames: usize) -> Seen {
        let listener = Listener::bind(endpoint).unwrap();
        let seen = Seen::default();
        let record = seen.clone();
        std::thread::spawn(move || {
            let mut revision = 0;
            while let Ok(connection) = listener.accept() {
                let index = {
                    let mut seen = record.lock().unwrap();
                    seen.push(Vec::new());
                    seen.len() - 1
                };
                let mut reader = BufReader::new(&connection);
                let mut count = 0;
                while let Ok(Some(next)) =
                    frame::read_frame::<_, ClientFrame>(&mut reader, MAX_REQUEST_BYTES)
                {
                    record.lock().unwrap()[index].push(next.clone());
                    let mut out = Vec::new();
                    match next {
                        ClientFrame::Hello(_) => out.push(ServerFrame::Welcome(Welcome {
                            version: PROTOCOL_VERSION,
                            app_version: "1".into(),
                            grant: Grant {
                                scopes: vec![Scope::Read],
                            },
                            push,
                        })),
                        ClientFrame::Subscribe(subscription) => {
                            out.push(reply(
                                subscription.id,
                                json!({"topics": subscription.topics}),
                            ));
                            for topic in subscription.topics {
                                revision += 1;
                                out.push(ServerFrame::Event(Event { topic, revision }));
                            }
                        }
                        ClientFrame::Unsubscribe(subscription) => {
                            out.push(reply(subscription.id, json!({"topics": []})));
                        }
                        ClientFrame::Call(call) => {
                            out.push(reply(call.id, json!({"projects": [], "total": 0})));
                        }
                    }
                    for frame in out {
                        frame::write_frame(&mut &connection, &frame, MAX_RESPONSE_BYTES).unwrap();
                    }
                    count += 1;
                    if count == frames {
                        break;
                    }
                }
            }
        });
        seen
    }

    fn reply(id: u64, value: serde_json::Value) -> ServerFrame {
        ServerFrame::Reply(Reply {
            id,
            result: Outcome::Ok(value),
        })
    }

    /// A client whose notices arrive on the returned channel.
    pub(super) fn client(endpoint: Endpoint) -> (Client, mpsc::Receiver<Notice>) {
        let client = Client::new(endpoint, info(CLIENT));
        let (notices, heard) = mpsc::channel();
        let notices = Mutex::new(notices);
        client.on_notice(Arc::new(move |notice| {
            let _ = notices.lock().unwrap().send(notice);
        }));
        (client, heard)
    }

    pub(super) fn next(heard: &mpsc::Receiver<Notice>) -> Notice {
        heard.recv_timeout(Duration::from_secs(20)).unwrap()
    }
}
use common::{info, scratch};
use support::{app, client, next};

/// Who the tests call as.
const CLIENT: &str = "push test";

#[test]
fn events_arrive_between_calls_once_a_topic_is_followed() {
    let endpoint = Endpoint::for_data_dir(&scratch("s1")).unwrap();
    let seen = app(&endpoint, true, usize::MAX);
    let (client, heard) = client(endpoint);
    client.subscribe(&[Topic::Session { id: 7 }]).unwrap();
    assert_eq!(
        next(&heard),
        Notice::Changed(Event {
            topic: Topic::Session { id: 7 },
            revision: 1
        }),
        "the pump reads the event though no call is waiting"
    );
    client.subscribe(&[Topic::Session { id: 7 }]).unwrap();
    assert_eq!(client.topics(), [Topic::Session { id: 7 }]);
    let listed = client.call::<ProjectList>(ProjectListInput::default());
    assert_eq!(listed.unwrap().total, 0, "calls still get their replies");
    client.unsubscribe(&[Topic::Session { id: 7 }]).unwrap();
    assert_eq!(client.topics(), Vec::<Topic>::new());
    let frames: Vec<String> = seen.lock().unwrap()[0]
        .iter()
        .map(|f| {
            serde_json::to_value(f).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        frames,
        ["hello", "subscribe", "call", "unsubscribe"],
        "following a topic twice sends one subscription"
    );
}

#[test]
fn a_new_connection_follows_the_same_topics_and_says_so() {
    let endpoint = Endpoint::for_data_dir(&scratch("s2")).unwrap();
    // Each connection ends after its hello and one subscription.
    let seen = app(&endpoint, true, 2);
    let (client, heard) = client(endpoint);
    let topics = [Topic::Workspace, Topic::SessionTranscript { id: 3 }];
    client.subscribe(&topics).unwrap();
    assert!(matches!(next(&heard), Notice::Changed(_)));
    assert!(matches!(next(&heard), Notice::Changed(_)));
    assert_eq!(next(&heard), Notice::Lost, "the app hung up");
    client.resubscribe().unwrap();
    assert_eq!(next(&heard), Notice::Resumed);
    let resent = seen.lock().unwrap()[1][1].clone();
    let ClientFrame::Subscribe(resent) = resent else {
        unreachable!("the second connection subscribes: {resent:?}")
    };
    assert_eq!(resent.topics, topics, "the same topics, in one frame");
    let Notice::Changed(event) = next(&heard) else {
        unreachable!("events flow again")
    };
    assert!(event.revision > 2, "the revision only grows: {event:?}");
}

#[test]
fn an_app_that_does_not_push_refuses_subscriptions_and_keeps_serving_calls() {
    let endpoint = Endpoint::for_data_dir(&scratch("s3")).unwrap();
    let seen = app(&endpoint, false, usize::MAX);
    let (client, _heard) = client(endpoint);
    let refused = client.subscribe(&[Topic::Projects]).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Internal);
    assert!(refused.message.contains("does not push"), "{refused}");
    assert!(
        client
            .call::<ProjectList>(ProjectListInput::default())
            .is_ok()
    );
    assert_eq!(client.topics(), Vec::<Topic>::new());
    assert_eq!(seen.lock().unwrap()[0].len(), 2, "hello and the call only");
}

#[test]
fn dropping_the_client_hangs_up_its_pumped_connection() {
    let endpoint = Endpoint::for_data_dir(&scratch("s4")).unwrap();
    let listener = Listener::bind(&endpoint).unwrap();
    let app = std::thread::spawn(move || {
        let connection = listener.accept().unwrap();
        let mut reader = BufReader::new(&connection);
        let read = |reader: &mut BufReader<&Connection>| {
            frame::read_frame::<_, ClientFrame>(reader, MAX_REQUEST_BYTES).unwrap()
        };
        assert!(matches!(read(&mut reader), Some(ClientFrame::Hello(_))));
        let welcome = ServerFrame::Welcome(Welcome {
            version: PROTOCOL_VERSION,
            app_version: "1".into(),
            grant: Grant {
                scopes: vec![Scope::Read],
            },
            push: true,
        });
        frame::write_frame(&mut &connection, &welcome, MAX_RESPONSE_BYTES).unwrap();
        let Some(ClientFrame::Subscribe(subscription)) = read(&mut reader) else {
            unreachable!("a subscription")
        };
        let ok = ServerFrame::Reply(Reply {
            id: subscription.id,
            result: Outcome::Ok(json!({})),
        });
        frame::write_frame(&mut &connection, &ok, MAX_RESPONSE_BYTES).unwrap();
        // The client's pump holds the connection until the client goes.
        read(&mut reader)
    });
    let (client, _heard) = client(endpoint);
    client.subscribe(&[Topic::Projects]).unwrap();
    drop(client);
    assert!(
        app.join().unwrap().is_none(),
        "the app sees a clean hang-up"
    );
}

/// Yields its chunks one read at a time, with a timeout between them.
struct Stutter(Vec<Option<&'static [u8]>>);

impl Read for Stutter {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.0.is_empty() {
            return Ok(0);
        }
        match self.0.remove(0) {
            Some(chunk) => {
                buf[..chunk.len()].copy_from_slice(chunk);
                Ok(chunk.len())
            }
            None => Err(io::ErrorKind::WouldBlock.into()),
        }
    }
}

#[test]
fn a_frame_split_by_a_read_timeout_is_kept_whole() {
    let mut lines = Lines::new(Stutter(vec![
        Some(br#"{"type":"event","topic":{"kind":"projects"},"#),
        None,
        Some(b"\"revision\":4}\r\n"),
        Some(b"x"),
        None,
        Some(b"y\n"),
    ]));
    let first = lines.next_frame::<ServerFrame>(MAX_RESPONSE_BYTES);
    let Err(timeout) = first else {
        unreachable!("the timeout surfaces: {first:?}")
    };
    assert!(frame::timed_out(&timeout));
    let event = lines.next_frame::<ServerFrame>(MAX_RESPONSE_BYTES).unwrap();
    assert_eq!(
        event,
        Some(ServerFrame::Event(Event {
            topic: Topic::Projects,
            revision: 4
        }))
    );
    assert!(lines.next_line(8).is_err(), "a second timeout");
    assert_eq!(lines.next_line(8).unwrap().unwrap(), b"xy");
    assert!(lines.next_line(8).unwrap().is_none(), "then a clean end");
    let mut long = Lines::new(Stutter(vec![Some(b"0123456789"), Some(b"\n")]));
    assert!(matches!(
        long.next_line(4),
        Err(frame::FrameError::TooLarge(4))
    ));
}
