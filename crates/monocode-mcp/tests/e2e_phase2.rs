//! End to end for phase 2: one new tool per domain through a real `rmcp`
//! client, the relay and a fake app on a real local socket under the
//! checkout's `target/c/`; and concurrent tool calls over the relay's one
//! control connection.
#[cfg(test)]
pub mod common;

use monocode_control::{
    Endpoint, Scope,
    frame::{
        self, Call, ClientFrame, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Outcome, Reply, ServerFrame,
    },
    transport::Connection,
};
use monocode_mcp::Options;
use rmcp::model::ProtocolVersion;
use serde_json::{Value, json};
use std::{
    io::BufReader,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    /// Each call's operation and request ID.
    pub(super) type Calls = Arc<Mutex<Vec<(String, Option<String>)>>>;

    /// Each call's operation and request ID, and a signal when a
    /// `session_wait` is held.
    #[derive(Clone, Default)]
    pub(super) struct Seen {
        pub(super) calls: Calls,
        pub(super) parked: Arc<tokio::sync::Notify>,
    }

    /// What the fake app answers for each phase 2 tool the tests call.
    pub(super) fn answer(name: &str) -> Value {
        match name {
            "session_configure" => {
                json!({"sessionId": 7, "harness": "claude", "model": "opus", "effort": "high", "access": "read-only"})
            }
            "session_rename" => json!({"sessionId": 7, "title": "Docs pass"}),
            "question_answer" => {
                json!({"sessionId": 7, "questionId": "q1", "declined": false, "status": {"state": "working"}})
            }
            "queue_remove" => json!({"sessionId": 7, "queueLength": 0, "paused": false}),
            "worktree_list" => {
                json!({"project": "/work/monocode", "worktrees": [{"path": "/work/monocode", "branch": "main", "head": "abc", "main": true, "missing": false}], "branches": [{"name": "main"}], "branchesOmitted": 0})
            }
            "project_reorder" => json!({"order": ["/work/b", "/work/a"]}),
            "layout_set" => {
                json!({"style": "focused", "boxy": true, "sidebarTab": "sessions", "rightPanelOpen": false, "primarySidebarVisible": true, "skipped": []})
            }
            "settings_update" => json!({"changed": ["theme"], "settings": settings()}),
            "search_sessions" => {
                json!({"hits": [{"sessionId": 7, "project": "/work/monocode", "title": "Flaky test", "kind": "message", "snippet": "the flaky test"}], "truncated": false})
            }
            "notes_write" => {
                json!({"id": "n1", "title": "Plan", "body": "# Plan", "tags": [], "updatedAt": 1})
            }
            "changes_read" => {
                json!({"root": "/work/monocode", "branch": "main", "ahead": 0, "behind": 0, "files": [{"path": "src/main.rs", "status": "modified", "additions": 2, "deletions": 1, "staged": false}], "omitted": 0, "additions": 2, "deletions": 1, "commits": []})
            }
            "project_list" => json!({"projects": [], "total": 0}),
            "session_wait" => {
                json!({"sessionId": 7, "status": {"state": "idle"}, "statusText": "Ready", "timedOut": false})
            }
            _ => Value::Null,
        }
    }

    fn settings() -> Value {
        json!({
            "theme": "light", "workspaceTheme": "default", "uiScale": 100,
            "transcriptLayout": "chat", "diffView": "editor", "tabAnimations": false,
            "alerts": {"sounds": true, "notifications": false},
            "files": {"wordWrap": true, "autosave": false, "formatOnSave": true},
            "followUp": "steer", "defaultModels": {}, "favoriteModels": [],
            "showRemainingUsage": false
        })
    }

    fn reply(id: u64, name: &str) -> ServerFrame {
        ServerFrame::Reply(Reply {
            id,
            result: Outcome::Ok(answer(name)),
        })
    }

    /// A fake app. It answers every call from [`answer`], except that it
    /// holds a `session_wait` until it has answered a `project_list`.
    pub(super) fn fake_app(endpoint: &Endpoint) -> Seen {
        let seen = Seen::default();
        let record = seen.clone();
        common::fake_app(endpoint, move |connection| {
            serve_connection(connection, &record);
        });
        seen
    }

    fn serve_connection(connection: &Connection, record: &Seen) {
        let mut reader = BufReader::new(connection);
        let mut parked: Option<Call> = None;
        while let Ok(Some(next)) =
            frame::read_frame::<_, ClientFrame>(&mut reader, MAX_REQUEST_BYTES)
        {
            let frames = match next {
                ClientFrame::Hello(_) => vec![welcome(Scope::ALL.to_vec())],
                ClientFrame::Call(call) => {
                    let name = call.command.name();
                    record
                        .calls
                        .lock()
                        .unwrap()
                        .push((name.to_owned(), call.request_id.clone()));
                    match name {
                        "session_wait" => {
                            parked = Some(call);
                            record.parked.notify_one();
                            Vec::new()
                        }
                        "project_list" => {
                            let mut frames = vec![reply(call.id, name)];
                            frames.extend(parked.take().map(|w| reply(w.id, "session_wait")));
                            frames
                        }
                        _ => vec![reply(call.id, name)],
                    }
                }
                ClientFrame::Subscribe(_) | ClientFrame::Unsubscribe(_) => Vec::new(),
            };
            for frame in frames {
                if frame::write_frame(&mut &*connection, &frame, MAX_RESPONSE_BYTES).is_err() {
                    return;
                }
            }
        }
    }
}
use common::{call, scratch, text, welcome};
use support::{answer, fake_app};

async fn connect(endpoint: Endpoint) -> common::Session {
    common::connect(
        Options::new(Ok(endpoint)),
        ProtocolVersion::LATEST,
        "e2e-phase2",
    )
    .await
}

#[tokio::test]
async fn one_tool_per_phase_2_domain_reaches_the_app_and_returns_its_output() {
    let endpoint = Endpoint::for_data_dir(&scratch("m9")).unwrap();
    let seen = fake_app(&endpoint);
    let session = connect(endpoint).await;
    let cases = [
        (
            "session_configure",
            json!({"sessionId": 7, "effort": "high", "access": "read-only"}),
            "Session 7: claude opus (high effort), read-only",
        ),
        (
            "session_rename",
            json!({"sessionId": 7, "title": "Docs pass", "requestId": "rename-1"}),
            "Session 7 is now “Docs pass”",
        ),
        (
            "question_answer",
            json!({"sessionId": 7, "questionId": "q1", "answers": [{"itemId": "one", "options": ["first"]}]}),
            "Question q1 of session 7 answered; session working",
        ),
        (
            "queue_remove",
            json!({"sessionId": 7, "messageId": "m1"}),
            "Message removed in session 7; 0 queued",
        ),
        (
            "worktree_list",
            json!({}),
            "1 worktrees, 1 branches in /work/monocode",
        ),
        (
            "project_reorder",
            json!({"order": ["/work/b", "/work/a"]}),
            "2 projects reordered",
        ),
        (
            "layout_set",
            json!({"style": "focused", "boxy": true}),
            "Focused layout; right panel closed",
        ),
        (
            "settings_update",
            json!({"theme": "light"}),
            "Changed theme",
        ),
        ("search_sessions", json!({"query": "flaky"}), "1 hits"),
        (
            "notes_write",
            json!({"body": "# Plan", "requestId": "note-1"}),
            "Saved note “Plan” (n1)",
        ),
        (
            "changes_read",
            json!({"project": "/work/monocode"}),
            "1 files changed (+2 −1) in /work/monocode",
        ),
    ];
    for (name, arguments, summary) in cases {
        let result = call(&session, name, arguments).await;
        assert_eq!(result.is_error, Some(false), "{name}: {}", text(&result));
        assert_eq!(result.structured_content, Some(answer(name)), "{name}");
        assert_eq!(text(&result), summary, "{name}");
    }
    let calls = seen.calls.lock().unwrap().clone();
    let names: Vec<_> = calls.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names.len(), 11, "{names:?}");
    assert_eq!(calls[1], ("session_rename".into(), Some("rename-1".into())));
    assert_eq!(calls[9], ("notes_write".into(), Some("note-1".into())));
    let tools = session.peer().list_all_tools().await.unwrap();
    let remove = tools.iter().find(|t| t.name == "queue_remove").unwrap();
    let remove = remove.annotations.as_ref().unwrap();
    assert_eq!(remove.destructive_hint, Some(true));
    let forbidden = call(&session, "settings_update", json!({"binaryPaths": {}})).await;
    assert_eq!(
        forbidden.structured_content.unwrap()["code"],
        "invalid_input",
        "keys outside the allowlist never reach the app"
    );
    assert_eq!(seen.calls.lock().unwrap().len(), 11);
    session.cancel().await.unwrap();
}

/// The fake app answers `session_wait` only after it answered a
/// `project_list`; a relay that ran one call at a time would never send the
/// second one, so the deadline would fail the test.
#[tokio::test]
async fn a_pending_session_wait_does_not_delay_a_concurrent_project_list() {
    let endpoint = Endpoint::for_data_dir(&scratch("m10")).unwrap();
    let seen = fake_app(&endpoint);
    let session = Arc::new(connect(endpoint).await);
    let waiting = session.clone();
    let wait = tokio::spawn(async move {
        call(
            &waiting,
            "session_wait",
            json!({"sessionId": 7, "timeoutMs": 60000}),
        )
        .await
    });
    // Send the list once the app holds the wait, so the order is certain.
    seen.parked.notified().await;
    let both = tokio::time::timeout(Duration::from_secs(20), async {
        let list = call(&session, "project_list", json!({})).await;
        (list, wait.await.unwrap())
    })
    .await;
    let Ok((list, wait)) = both else {
        unreachable!("project_list was held up behind session_wait")
    };
    assert_eq!(list.is_error, Some(false), "{}", text(&list));
    assert_eq!(wait.is_error, Some(false), "{}", text(&wait));
    assert_eq!(wait.structured_content, Some(answer("session_wait")));
    let names: Vec<_> = seen
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|(n, _)| n.clone())
        .collect();
    assert_eq!(names, ["session_wait", "project_list"]);
}
