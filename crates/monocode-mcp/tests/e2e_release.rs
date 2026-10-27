//! A relay lets go of a control connection no call uses, so the app's
//! connection slots stay free, and connects again on the next call. A change
//! after the release runs normally: the relay hung up before it sent
//! anything, so no call meets a closed connection. A real `rmcp` client over
//! an in-memory duplex; a fake app on a real local socket under the
//! checkout's `target/c/`.
#[cfg(test)]
pub mod common;

use monocode_control::{
    Endpoint, Scope, SessionCredential,
    frame::{
        self, ClientFrame, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Outcome, Reply, ServerFrame,
    },
};
use monocode_mcp::Options;
use rmcp::model::ProtocolVersion;
use serde_json::json;
use std::{
    io::BufReader,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    /// Welcomes every hello and renames whatever it is asked to. Counts
    /// the hellos, and says when a relay hangs up.
    pub(super) fn fake_app(endpoint: &Endpoint) -> (Arc<AtomicUsize>, UnboundedReceiver<()>) {
        let hellos = Arc::new(AtomicUsize::new(0));
        let (closed, hung_up) = unbounded_channel();
        let counted = Arc::clone(&hellos);
        common::fake_app(endpoint, move |connection| {
            let mut reader = BufReader::new(connection);
            while let Ok(Some(next)) =
                frame::read_frame::<_, ClientFrame>(&mut reader, MAX_REQUEST_BYTES)
            {
                let answer = match next {
                    ClientFrame::Hello(_) => {
                        counted.fetch_add(1, Ordering::SeqCst);
                        common::welcome(Scope::ALL.to_vec())
                    }
                    ClientFrame::Call(call) => ServerFrame::Reply(Reply {
                        id: call.id,
                        result: Outcome::Ok(json!({"sessionId": 7, "title": "Renamed"})),
                    }),
                    ClientFrame::Subscribe(_) | ClientFrame::Unsubscribe(_) => continue,
                };
                if frame::write_frame(&mut &*connection, &answer, MAX_RESPONSE_BYTES).is_err() {
                    break;
                }
            }
            let _ = closed.send(());
        });
        (hellos, hung_up)
    }
}
use common::{call, connect, scratch, text};
use support::fake_app;

#[tokio::test]
async fn an_unused_connection_is_let_go_and_the_next_change_connects_again() {
    let endpoint = Endpoint::for_data_dir(&scratch("mr1")).unwrap();
    let (hellos, mut hung_up) = fake_app(&endpoint);
    let credential = SessionCredential {
        id: 3,
        token: "c0ffee".into(),
    };
    let options = Options::new(Ok(endpoint))
        .session(Some(credential))
        .release_after(Duration::from_millis(50));
    let session = connect(options, ProtocolVersion::LATEST, "Claude Code").await;
    let rename = |title: &str| json!({"sessionId": 7, "title": title});
    let first = call(&session, "session_rename", rename("First")).await;
    assert_ne!(first.is_error, Some(true), "{}", text(&first));
    let hung = tokio::time::timeout(Duration::from_secs(20), hung_up.recv()).await;
    assert!(
        matches!(hung, Ok(Some(()))),
        "the relay hangs up the unused connection"
    );
    assert_eq!(hellos.load(Ordering::SeqCst), 1);
    let second = call(&session, "session_rename", rename("Second")).await;
    assert_ne!(
        second.is_error,
        Some(true),
        "a change after the release runs: {}",
        text(&second)
    );
    assert!(
        !text(&second).contains("may or may not"),
        "{}",
        text(&second)
    );
    assert_eq!(
        hellos.load(Ordering::SeqCst),
        2,
        "the second call said hello on a new connection"
    );
}
