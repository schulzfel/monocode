//! End to end with pushed changes: a real `rmcp` client subscribes to a
//! resource on each protocol version, the fake app changes it and pushes an
//! event, the client gets `notifications/resources/updated` and reads the
//! change. Also unsubscribing, refused resources, and following again after
//! the app restarts.
#[cfg(test)]
pub mod common;

use monocode_control::{
    Endpoint, Grant, PROTOCOL_VERSION, Scope, Topic,
    frame::{
        self, ClientFrame, Event, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Outcome, Reply,
        ServerFrame, Welcome,
    },
    transport::{Connection, Listener},
};
use monocode_mcp::{Options, serve};
use rmcp::{
    ClientHandler, ClientLifecycleMode,
    model::{
        ClientCapabilities, ClientConfig, ClientRequest, Implementation, ProtocolVersion,
        ReadResourceRequestParams, Request, ResourceContents, ResourceUpdatedNotificationParam,
        ServerNotification, SubscribeRequestParams, SubscriptionFilter, UnsubscribeRequestParams,
    },
    service::{ClientServiceExt, NotificationContext, RoleClient, RunningService},
};
use serde_json::{Value, json};
use std::{
    io::BufReader,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    /// One accepted connection: writes are whole frames under the lock.
    struct Peer {
        connection: Connection,
        writing: Mutex<()>,
        topics: Mutex<Vec<Topic>>,
    }

    impl Peer {
        fn send(&self, frame: &ServerFrame) {
            let _held = self.writing.lock().unwrap();
            let _ = frame::write_frame(&mut &self.connection, frame, MAX_RESPONSE_BYTES);
        }
    }

    /// A fake app that pushes: session 7's status text is whatever the test
    /// last set, and every subscription or unsubscription is reported.
    #[derive(Clone)]
    pub(super) struct App {
        text: Arc<Mutex<String>>,
        peers: Arc<Mutex<Vec<Arc<Peer>>>>,
        revision: Arc<AtomicU64>,
        subscriptions: UnboundedSender<(bool, Vec<Topic>)>,
        /// A change made while the next subscription is taken on.
        on_subscribe: Arc<Mutex<Option<String>>>,
    }

    impl App {
        pub(super) fn start(endpoint: &Endpoint) -> (Self, UnboundedReceiver<(bool, Vec<Topic>)>) {
            let (subscriptions, heard) = unbounded_channel();
            let app = Self {
                text: Arc::new(Mutex::new("Ready".into())),
                peers: Arc::default(),
                revision: Arc::default(),
                subscriptions,
                on_subscribe: Arc::default(),
            };
            let listener = Listener::bind(endpoint).unwrap();
            let serving = app.clone();
            std::thread::spawn(move || {
                while let Ok(connection) = listener.accept() {
                    let peer = Arc::new(Peer {
                        connection,
                        writing: Mutex::new(()),
                        topics: Mutex::default(),
                    });
                    serving.peers.lock().unwrap().push(peer.clone());
                    let serving = serving.clone();
                    std::thread::spawn(move || serving.serve(&peer));
                }
            });
            (app, heard)
        }

        fn serve(&self, peer: &Peer) {
            let mut reader = BufReader::new(&peer.connection);
            while let Ok(Some(next)) =
                frame::read_frame::<_, ClientFrame>(&mut reader, MAX_REQUEST_BYTES)
            {
                let (id, result) = match next {
                    ClientFrame::Hello(_) => {
                        peer.send(&ServerFrame::Welcome(Welcome {
                            version: PROTOCOL_VERSION,
                            app_version: "0.16.0".into(),
                            grant: Grant {
                                scopes: vec![Scope::Read],
                            },
                            push: true,
                        }));
                        continue;
                    }
                    ClientFrame::Subscribe(s) => {
                        peer.topics.lock().unwrap().extend(s.topics.clone());
                        // As the real app's push thread may, the change goes
                        // out before the reply.
                        let text = self.on_subscribe.lock().unwrap().take();
                        if let Some(text) = text {
                            self.change(&text);
                        }
                        let _ = self.subscriptions.send((true, s.topics.clone()));
                        (s.id, json!({"topics": s.topics}))
                    }
                    ClientFrame::Unsubscribe(s) => {
                        peer.topics
                            .lock()
                            .unwrap()
                            .retain(|t| !s.topics.contains(t));
                        let _ = self.subscriptions.send((false, s.topics));
                        (s.id, json!({"topics": []}))
                    }
                    ClientFrame::Call(call) => (
                        call.id,
                        json!({
                            "sessionId": 7, "project": "/work/monocode", "status": {"kind": "working"},
                            "statusText": *self.text.lock().unwrap(), "busy": true,
                            "pendingApprovals": 0, "pendingQuestions": 0, "queueLength": 0,
                            "harness": "claude", "access": "supervised"
                        }),
                    ),
                };
                let result = Outcome::Ok(result);
                peer.send(&ServerFrame::Reply(Reply { id, result }));
            }
        }

        /// Changes session 7 and pushes the change to whoever follows it.
        pub(super) fn change(&self, text: &str) {
            text.clone_into(&mut self.text.lock().unwrap());
            let topic = Topic::Session { id: 7 };
            let revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
            for peer in self.peers.lock().unwrap().iter() {
                if peer.topics.lock().unwrap().contains(&topic) {
                    let topic = topic.clone();
                    peer.send(&ServerFrame::Event(Event { topic, revision }));
                }
            }
        }

        /// Changes session 7 to `text` while the next subscription is taken
        /// on: after the topic is followed, before the reply is written.
        pub(super) fn change_on_subscribe(&self, text: &str) {
            *self.on_subscribe.lock().unwrap() = Some(text.into());
        }

        /// Hangs up every connection, as a quitting app would.
        pub(super) fn hang_up(&self) {
            for peer in self.peers.lock().unwrap().drain(..) {
                let _ = peer.connection.shutdown();
            }
        }
    }

    /// A client that records the URIs of `notifications/resources/updated`.
    pub(super) struct Recorder {
        config: ClientConfig,
        updates: UnboundedSender<String>,
    }

    impl ClientHandler for Recorder {
        fn get_info(&self) -> ClientConfig {
            self.config.clone()
        }
        async fn on_resource_updated(
            &self,
            params: ResourceUpdatedNotificationParam,
            _context: NotificationContext<RoleClient>,
        ) {
            let _ = self.updates.send(params.uri);
        }
    }

    pub(super) type Session = RunningService<RoleClient, Recorder>;

    /// A client on `version`: `initialize` up to 2025-11-25, and the
    /// `server/discover` lifecycle from 2026-07-28 on.
    pub(super) async fn connect(
        endpoint: Endpoint,
        version: ProtocolVersion,
    ) -> (Session, UnboundedReceiver<String>) {
        let (server_io, client_io) = tokio::io::duplex(1 << 16);
        tokio::spawn(async move { serve(Options::new(Ok(endpoint)), server_io).await });
        let (updates, heard) = unbounded_channel();
        let config = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("push-client", "1"),
        )
        .with_protocol_version(version.clone());
        let recorder = Recorder { config, updates };
        let lifecycle = if version.has_initialize() {
            ClientLifecycleMode::Initialize
        } else {
            ClientLifecycleMode::Discover {
                preferred_versions: vec![version],
            }
        };
        let session = recorder
            .serve_with_lifecycle(client_io, lifecycle)
            .await
            .unwrap();
        (session, heard)
    }

    pub(super) async fn next<T>(heard: &mut UnboundedReceiver<T>) -> T {
        tokio::time::timeout(Duration::from_secs(20), heard.recv())
            .await
            .unwrap()
            .unwrap()
    }

    /// `resources/subscribe` or `resources/unsubscribe`, sent as raw requests.
    pub(super) async fn legacy(session: &Session, uri: &str, on: bool) -> Result<(), String> {
        let request = if on {
            ClientRequest::SubscribeRequest(Request::new(SubscribeRequestParams::new(uri)))
        } else {
            ClientRequest::UnsubscribeRequest(Request::new(UnsubscribeRequestParams::new(uri)))
        };
        session
            .peer()
            .send_request(request)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub(super) async fn status_text(session: &Session) -> Value {
        let read = session
            .peer()
            .read_resource(ReadResourceRequestParams::new("monocode://sessions/7"))
            .await
            .unwrap();
        let [ResourceContents::TextResourceContents { text, .. }] = &read.contents[..] else {
            unreachable!("one text content")
        };
        serde_json::from_str::<Value>(text).unwrap()["statusText"].clone()
    }
}
use common::scratch;
use support::{App, connect, legacy, next, status_text};

const SESSION: &str = "monocode://sessions/7";

#[tokio::test]
async fn resources_subscribe_hears_an_app_side_change_and_the_read_shows_it() {
    let endpoint = Endpoint::for_data_dir(&scratch("e1")).unwrap();
    let (app, mut subscribed) = App::start(&endpoint);
    let (session, mut updates) = connect(endpoint, ProtocolVersion::V_2025_11_25).await;
    legacy(&session, SESSION, true).await.unwrap();
    assert_eq!(
        next(&mut subscribed).await,
        (true, vec![Topic::Session { id: 7 }])
    );
    assert_eq!(status_text(&session).await, "Ready");
    app.change("Waiting for approval");
    assert_eq!(next(&mut updates).await, SESSION);
    assert_eq!(status_text(&session).await, "Waiting for approval");
    // The 2026 mechanism is not offered on the old protocol, and vice versa.
    let listen = session.peer().listen(SubscriptionFilter::default()).await;
    assert!(listen.is_err(), "subscriptions/listen is 2026-07-28 only");
    legacy(&session, SESSION, false).await.unwrap();
    assert_eq!(
        next(&mut subscribed).await,
        (false, vec![Topic::Session { id: 7 }])
    );
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn subscriptions_listen_hears_an_app_side_change_and_the_read_shows_it() {
    let endpoint = Endpoint::for_data_dir(&scratch("e2")).unwrap();
    let (app, mut subscribed) = App::start(&endpoint);
    let (session, _updates) = connect(endpoint, ProtocolVersion::V_2026_07_28).await;
    let filter = SubscriptionFilter::builder()
        .resource_subscriptions([SESSION, "monocode://app", "monocode://nothing"])
        .resources_list_changed()
        .build();
    let mut listening = session.peer().listen(filter).await.unwrap();
    let accepted = listening.acknowledged().clone();
    assert_eq!(
        accepted.resource_subscriptions,
        Some(vec![SESSION.to_owned()]),
        "only resources that change are accepted"
    );
    assert_eq!(
        accepted.resources_list_changed, None,
        "the lists never change"
    );
    assert_eq!(
        next(&mut subscribed).await,
        (true, vec![Topic::Session { id: 7 }])
    );
    app.change("Streaming…");
    let notification = tokio::time::timeout(Duration::from_secs(20), listening.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let ServerNotification::ResourceUpdatedNotification(updated) = notification else {
        unreachable!("an update: {notification:?}")
    };
    assert_eq!(updated.params.uri, SESSION);
    assert_eq!(status_text(&session).await, "Streaming…");
    let legacy_call = legacy(&session, SESSION, true).await;
    assert!(
        legacy_call.is_err(),
        "resources/subscribe is 2025-11-25 only"
    );
    drop(listening);
    assert_eq!(
        next(&mut subscribed).await,
        (false, vec![Topic::Session { id: 7 }]),
        "ending the listen request stops following"
    );
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn a_resource_that_does_not_change_cannot_be_subscribed() {
    let endpoint = Endpoint::for_data_dir(&scratch("e3")).unwrap();
    let (_app, _subscribed) = App::start(&endpoint);
    let (session, _updates) = connect(endpoint, ProtocolVersion::V_2025_11_25).await;
    let refused = legacy(&session, "monocode://app", true).await.unwrap_err();
    assert!(refused.contains("cannot be subscribed"), "{refused}");
    assert!(legacy(&session, "monocode://nothing", true).await.is_err());
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn after_the_app_restarts_the_relay_follows_again_and_says_everything_may_have_changed() {
    let endpoint = Endpoint::for_data_dir(&scratch("e4")).unwrap();
    let (app, mut subscribed) = App::start(&endpoint);
    let (session, mut updates) = connect(endpoint, ProtocolVersion::V_2025_11_25).await;
    legacy(&session, SESSION, true).await.unwrap();
    legacy(&session, "monocode://workspace", true)
        .await
        .unwrap();
    assert_eq!(next(&mut subscribed).await.1, [Topic::Session { id: 7 }]);
    assert_eq!(next(&mut subscribed).await.1, [Topic::Workspace]);
    app.hang_up();
    let (on, mut topics) = next(&mut subscribed).await;
    topics.sort();
    assert!(on, "the relay connects again by itself");
    assert_eq!(topics, [Topic::Workspace, Topic::Session { id: 7 }]);
    let mut resumed = vec![next(&mut updates).await, next(&mut updates).await];
    resumed.sort();
    assert_eq!(resumed, ["monocode://sessions/7", "monocode://workspace"]);
    app.change("Ready again");
    assert_eq!(next(&mut updates).await, SESSION);
    assert_eq!(status_text(&session).await, "Ready again");
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn a_change_pushed_before_the_subscribe_reply_reaches_a_resources_subscriber() {
    let endpoint = Endpoint::for_data_dir(&scratch("e5")).unwrap();
    let (app, _subscribed) = App::start(&endpoint);
    let (session, mut updates) = connect(endpoint, ProtocolVersion::V_2025_11_25).await;
    app.change_on_subscribe("Pushed while subscribing");
    legacy(&session, SESSION, true).await.unwrap();
    assert_eq!(next(&mut updates).await, SESSION);
    assert_eq!(status_text(&session).await, "Pushed while subscribing");
    session.cancel().await.unwrap();
}

#[tokio::test]
async fn a_change_pushed_before_the_subscribe_reply_reaches_a_listener() {
    let endpoint = Endpoint::for_data_dir(&scratch("e6")).unwrap();
    let (app, _subscribed) = App::start(&endpoint);
    let (session, _updates) = connect(endpoint, ProtocolVersion::V_2026_07_28).await;
    app.change_on_subscribe("Pushed while listening");
    let filter = SubscriptionFilter::builder()
        .resource_subscriptions([SESSION])
        .build();
    let mut listening = session.peer().listen(filter).await.unwrap();
    let notification = tokio::time::timeout(Duration::from_secs(20), listening.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let ServerNotification::ResourceUpdatedNotification(updated) = notification else {
        unreachable!("an update: {notification:?}")
    };
    assert_eq!(updated.params.uri, SESSION);
    assert_eq!(status_text(&session).await, "Pushed while listening");
    session.cancel().await.unwrap();
}
