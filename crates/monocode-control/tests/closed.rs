//! A connection the app closes while a call runs. A call that is safe to
//! repeat may be retried; one that changes something may already have acted,
//! so it is not retryable and says to check before calling again.
#[cfg(test)]
pub mod common;

use monocode_control::{
    Client, Command, Endpoint, ErrorCode, Grant, PROTOCOL_VERSION, Scope,
    frame::{self, ClientFrame, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, ServerFrame, Welcome},
    ops::{
        dispatch::SessionSendInput,
        project::{ProjectList, ProjectListInput},
    },
    transport::{Connection, Listener},
};
use std::io::BufReader;

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    /// Welcomes one connection, reads its first call and hangs up without
    /// answering it.
    pub(super) fn hang_up_on_first_call(connection: &Connection) -> ClientFrame {
        let mut reader = BufReader::new(connection);
        let _hello: ClientFrame = frame::read_frame(&mut reader, MAX_REQUEST_BYTES)
            .unwrap()
            .unwrap();
        let welcome = ServerFrame::Welcome(Welcome {
            version: PROTOCOL_VERSION,
            app_version: "0.16.0".into(),
            grant: Grant {
                scopes: vec![Scope::Read, Scope::Dispatch],
            },
            push: false,
        });
        frame::write_frame(&mut &*connection, &welcome, MAX_RESPONSE_BYTES).unwrap();
        frame::read_frame(&mut reader, MAX_REQUEST_BYTES)
            .unwrap()
            .unwrap()
    }
}
use common::{info, scratch};
use support::hang_up_on_first_call;

/// Who the tests call as.
const CLIENT: &str = "closed test";

#[test]
fn a_change_cut_off_mid_call_is_not_retryable_and_says_to_check_first() {
    let endpoint = Endpoint::for_data_dir(&scratch("c1")).unwrap();
    let listener = Listener::bind(&endpoint).unwrap();
    let app = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for _ in 0..2 {
            let connection = listener.accept().unwrap();
            seen.push(hang_up_on_first_call(&connection));
        }
        (listener, seen)
    });
    let client = Client::new(endpoint, info(CLIENT));
    let send = Command::SessionSend(SessionSendInput {
        session_id: 3,
        prompt: "Fix the tests".into(),
        ..SessionSendInput::default()
    });
    let error = client.send(None, send).unwrap_err();
    assert_eq!(error.code, ErrorCode::Internal);
    assert!(
        !error.retryable,
        "the message may already be on its way: {error:?}"
    );
    assert!(
        error.message.contains("may or may not") && error.message.contains("session_list"),
        "{}",
        error.message
    );
    // A read changes nothing, so it stays retryable.
    let read = client
        .call::<ProjectList>(ProjectListInput::default())
        .unwrap_err();
    assert!(read.retryable, "{read:?}");
    client.disconnect();
    let (_, seen) = app.join().unwrap();
    assert_eq!(seen.len(), 2, "each call reached the app exactly once");
}
