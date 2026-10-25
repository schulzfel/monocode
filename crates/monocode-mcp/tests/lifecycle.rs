//! How the relay reports an MCP client that leaves before a session starts.
//! The relay accepts both lifecycles: `initialize` (2025-11-25) and
//! `server/discover` followed by requests that carry their own protocol
//! metadata (2026-07-28). Its error names what actually happened.
use monocode_control::ControlError;
use monocode_mcp::{Options, serve};
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    /// Sends `lines` to a fresh relay, one reply each, then hangs up;
    /// returns how the relay ended and what it answered.
    pub(super) async fn session_of(lines: &[&str]) -> (Result<(), String>, Vec<Value>) {
        let (server_io, client_io) = tokio::io::duplex(1 << 16);
        let relay = tokio::spawn(serve(
            Options::new(Err(ControlError::internal("no app in this test"))),
            server_io,
        ));
        let (read, mut write) = tokio::io::split(client_io);
        let mut replies = BufReader::new(read).lines();
        let mut heard = Vec::new();
        for line in lines {
            write
                .write_all(format!("{line}\n").as_bytes())
                .await
                .unwrap();
            let reply = tokio::time::timeout(Duration::from_secs(20), replies.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            heard.push(serde_json::from_str(&reply).unwrap());
        }
        drop(write);
        drop(replies);
        let ended = tokio::time::timeout(Duration::from_secs(20), relay)
            .await
            .unwrap()
            .unwrap();
        (ended, heard)
    }
}
use support::session_of;

#[tokio::test]
async fn a_client_that_leaves_after_a_refused_discover_is_reported_as_such() {
    // No `_meta` protocol version: the discover request is refused.
    let discover = r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{}}"#;
    let (ended, heard) = session_of(&[discover]).await;
    assert!(heard[0]["error"].is_object(), "refused: {:?}", heard[0]);
    let message = ended.unwrap_err();
    assert!(
        !message.contains("initialize request"),
        "the relay does not claim it only waited for initialize: {message}"
    );
    assert!(
        message.contains("server/discover") && message.contains("initialize"),
        "it names both ways a session starts: {message}"
    );
}

#[tokio::test]
async fn a_client_that_leaves_without_a_word_is_reported_the_same_way() {
    let (ended, _) = session_of(&[]).await;
    let message = ended.unwrap_err();
    assert!(message.contains("before a session started"), "{message}");
}
