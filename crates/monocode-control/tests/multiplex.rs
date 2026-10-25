//! One client connection carries several calls at once: replies are routed
//! by call id, so a slow call never holds up a quick one.
#[cfg(test)]
pub mod common;

use monocode_control::{
    Client, Command, Endpoint, Grant, PROTOCOL_VERSION, Scope,
    frame::{
        self, ClientFrame, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Outcome, Reply, ServerFrame,
        Welcome,
    },
    ops::{dispatch::SessionWaitInput, project::ProjectListInput},
    transport::Listener,
};
use serde_json::json;
use std::{
    io::BufReader,
    sync::{Arc, mpsc},
    time::Duration,
};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    pub(super) fn wait() -> Command {
        Command::SessionWait(SessionWaitInput {
            session_id: 7,
            timeout_ms: None,
        })
    }
}
use common::{info, scratch};
use support::wait;

/// Who the tests call as.
const CLIENT: &str = "multiplex test";

/// The app holds the first call's reply until the client has the second
/// call's reply. A client that waited for each reply in turn would never
/// send the second call; the deadline turns that into a failure.
#[test]
fn a_pending_call_does_not_delay_a_concurrent_one() {
    let endpoint = Endpoint::for_data_dir(&scratch("t9")).unwrap();
    let listener = Listener::bind(&endpoint).unwrap();
    let (quick_done, quick_answered) = mpsc::channel::<()>();
    let app = std::thread::spawn(move || {
        let connection = listener.accept().unwrap();
        let mut reader = BufReader::new(&connection);
        let read = |reader: &mut BufReader<&_>| {
            frame::read_frame::<_, ClientFrame>(reader, MAX_REQUEST_BYTES)
                .unwrap()
                .unwrap()
        };
        assert!(matches!(read(&mut reader), ClientFrame::Hello(_)));
        let welcome = ServerFrame::Welcome(Welcome {
            version: PROTOCOL_VERSION,
            app_version: "1".into(),
            grant: Grant {
                scopes: vec![Scope::Read, Scope::Dispatch],
            },
            push: false,
        });
        frame::write_frame(&mut &connection, &welcome, MAX_RESPONSE_BYTES).unwrap();
        // Both calls arrive before either is answered, in either order.
        let mut calls = [read(&mut reader), read(&mut reader)].map(|frame| match frame {
            ClientFrame::Call(call) => call,
            other => unreachable!("calls after hello: {other:?}"),
        });
        calls.sort_by_key(|call| call.command.name() != "session_wait");
        let [slow, quick] = calls;
        assert_eq!(
            (slow.command.name(), quick.command.name()),
            ("session_wait", "project_list")
        );
        let reply = |id, value| {
            ServerFrame::Reply(Reply {
                id,
                result: Outcome::Ok(value),
            })
        };
        frame::write_frame(
            &mut &connection,
            &reply(quick.id, json!({"which": "quick"})),
            MAX_RESPONSE_BYTES,
        )
        .unwrap();
        let answered = quick_answered.recv_timeout(Duration::from_secs(20));
        assert!(
            answered.is_ok(),
            "the quick reply reaches its caller while the slow call waits"
        );
        frame::write_frame(
            &mut &connection,
            &reply(slow.id, json!({"which": "slow"})),
            MAX_RESPONSE_BYTES,
        )
        .unwrap();
        // Hold the connection until the client hangs up.
        while let Ok(Some(_)) = frame::read_frame::<_, ClientFrame>(&mut reader, MAX_REQUEST_BYTES)
        {
        }
    });
    let client = Arc::new(Client::new(endpoint, info(CLIENT)));
    client.connect().unwrap();
    let waiting = Arc::clone(&client);
    let slow = std::thread::spawn(move || waiting.send(None, wait()));
    // The quick call comes from another thread on the same client.
    let quick = Arc::clone(&client);
    let quick = std::thread::spawn(move || {
        quick.send(None, Command::ProjectList(ProjectListInput::default()))
    });
    let quick = quick.join().unwrap().unwrap();
    assert_eq!(quick, json!({"which": "quick"}));
    quick_done.send(()).unwrap();
    assert_eq!(slow.join().unwrap().unwrap(), json!({"which": "slow"}));
    client.disconnect();
    app.join().unwrap();
}
