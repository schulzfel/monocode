//! Operator's session actions as an agent session calls them: paging by
//! turn index, sending only into an idle session, archiving a working one,
//! refused deletes, taking a session out of its folder, and launches split
//! beside the caller's own pane.
use super::tests::{call, repository, sessions, set_busy, setup};
use super::tests_gate::add_session;
use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    Command,
    ops::{
        configure::SessionDraftOutput,
        dispatch::{
            Delivery, SessionSendInput, SessionSendOutput, SessionStartInput, SessionStartOutput,
            WorkspaceMode,
        },
        organize::{FolderListOutput, ProjectInput, SessionArchiveOutput, SessionMoveOutput},
        session::{SessionReadInput, SessionReadOutput},
        worktree::{WorktreeCreateInput, WorktreeCreateOutput},
    },
    session_caller_name,
};
use serde_json::json;

const HERE: &str = "/work/monocode";
const BUSY: &str = "Session is busy, loading, or has an unsent draft. Its draft was preserved.";

/// Calls `command` (an operation and its input) as session `id`.
fn by(
    window: WindowHandle<Shell>,
    id: i64,
    command: Value,
    cx: &mut TestAppContext,
) -> Result<Value, ControlError> {
    let command: Command = serde_json::from_value(command).unwrap();
    call(window, &session_caller_name(id), None, command, cx)
}

fn output<T: serde::de::DeserializeOwned>(value: Result<Value, ControlError>) -> T {
    serde_json::from_value(value.unwrap()).unwrap()
}

#[gpui::test]
fn session_read_pages_by_a_clamped_turn_index(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (_, b) = sessions(window, cx);
    let id = add_session(window, HERE, true, cx);
    window
        .update(cx, |shell, _, _| {
            let row = |role: &str, text: &str| monocode_core::transcript::Row {
                role: role.into(),
                text: text.into(),
                first: true,
                sent_at: None,
            };
            let rows = [
                row("user", "First"),
                row("assistant", "One"),
                row("user", "Second"),
                row("assistant", "Two"),
                row("user", "/operator Third"),
                row("assistant", "Three"),
            ];
            locked(&shell.store)
                .unwrap()
                .write_rows(id, 0, &rows)
                .unwrap();
        })
        .unwrap();
    let read = |input: Value, cx: &mut TestAppContext| {
        let mut input = input;
        input["sessionId"] = json!(id);
        by(
            window,
            b,
            json!({"operation": "session_read", "input": input}),
            cx,
        )
    };
    let users = |page: &SessionReadOutput| -> Vec<String> {
        page.turns.iter().map(|t| t.user.text.clone()).collect()
    };
    let early: SessionReadOutput = output(read(json!({"beforeIndex": 2, "limit": 1}), cx));
    assert_eq!(users(&early), ["Second"]);
    assert_eq!(early.next_before.as_deref(), Some("row-2"), "its own turn");
    let all: SessionReadOutput = output(read(json!({"beforeIndex": 99}), cx));
    assert_eq!(
        users(&all),
        ["First", "Second", "Third"],
        "clamped; /operator stripped"
    );
    let none: SessionReadOutput = output(read(json!({"beforeIndex": 0}), cx));
    assert!(none.turns.is_empty());
    let both = read(json!({"before": "row-2", "beforeIndex": 1}), cx).unwrap_err();
    assert_eq!(both.code, ErrorCode::InvalidInput);
    let typed = Command::SessionRead(SessionReadInput {
        session_id: id,
        before_index: Some(1),
        ..SessionReadInput::default()
    });
    let page: SessionReadOutput = output(call(window, &session_caller_name(b), None, typed, cx));
    assert_eq!(users(&page), ["First"]);
}

/// A send that waits for nothing: only into an idle session.
fn if_idle(target: i64, prompt: &str) -> Value {
    json!({"operation": "session_send", "input": {"sessionId": target, "prompt": prompt, "ifIdle": true}})
}

/// Session `id`'s composer text and saved draft.
fn draft_of(
    window: WindowHandle<Shell>,
    id: i64,
    cx: &mut TestAppContext,
) -> (String, Option<String>) {
    window
        .update(cx, |shell, _, cx| {
            let view = shell.sessions[&id].read(cx);
            let saved = view.has_saved_draft().then(|| {
                locked(&shell.store)
                    .unwrap()
                    .follow_ups(id)
                    .unwrap()
                    .draft
                    .map(|d| d.text)
                    .unwrap_or_default()
            });
            (view.input.read(cx).content.to_string(), saved)
        })
        .unwrap()
}

/// Loads session `id`'s chat, with a provider that does not exist, so a
/// turn it starts fails before any process runs.
fn loaded(window: WindowHandle<Shell>, id: i64, cx: &mut TestAppContext) {
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&id].update(cx, |v, cx| {
                v.summary.provider = "invalid-test-provider".into();
                v.load(cx);
            });
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            assert!(
                shell.sessions[&id].read(cx).ready_for_external(cx),
                "idle and loaded"
            );
        })
        .unwrap();
}

#[gpui::test]
fn sending_if_idle_refuses_and_keeps_the_persons_draft(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (_, b) = sessions(window, cx);
    let target = add_session(window, HERE, false, cx);
    // No provider ever starts: this one does not exist.
    loaded(window, target, cx);
    let refused = |cx: &mut TestAppContext| {
        let error = by(window, b, if_idle(target, "Take over"), cx).unwrap_err();
        assert_eq!(
            (error.code, error.message.as_str()),
            (ErrorCode::SessionBusy, BUSY)
        );
    };
    set_busy(window, target, true, cx);
    refused(cx);
    set_busy(window, target, false, cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&target].update(cx, |v, cx| {
                v.input
                    .update(cx, |i, cx| i.set_text("My own words".into(), cx));
            });
        })
        .unwrap();
    refused(cx);
    assert_eq!(draft_of(window, target, cx), ("My own words".into(), None));
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&target].update(cx, |v, cx| {
                v.input.update(cx, |i, cx| i.set_text(String::new(), cx));
            });
        })
        .unwrap();
    // A draft saved for the person is theirs too.
    let saved: SessionDraftOutput = output(by(
        window,
        b,
        json!({"operation": "session_draft", "input": {"sessionId": target, "text": "Review first"}}),
        cx,
    ));
    assert!(saved.saved, "the draft was saved");
    assert_eq!(
        draft_of(window, target, cx).1.as_deref(),
        Some("Review first")
    );
    refused(cx);
    assert_eq!(
        draft_of(window, target, cx).1.as_deref(),
        Some("Review first")
    );
    // R1: a chat that is only not loaded yet is loaded, and takes the
    // message and a draft once it has; a draft it is known to keep is still
    // the person's.
    let unloaded = add_session(window, HERE, true, cx);
    let sent: SessionSendOutput = output(by(window, b, if_idle(unloaded, "/draft Start"), cx));
    assert_eq!(sent.delivery, Delivery::Started);
    assert_eq!(draft_of(window, unloaded, cx).1.as_deref(), Some("Start"));
    let unloaded = add_session(window, HERE, true, cx);
    let saved: SessionDraftOutput = output(by(
        window,
        b,
        json!({"operation": "session_draft", "input": {"sessionId": unloaded, "text": "Look first"}}),
        cx,
    ));
    assert!(saved.saved, "saved once loaded");
    assert_eq!(
        draft_of(window, unloaded, cx).1.as_deref(),
        Some("Look first")
    );
    let drafted = add_session(window, HERE, true, cx);
    window
        .update(cx, |shell, _, _| shell.draft_sessions.insert(drafted))
        .unwrap();
    let error = by(window, b, if_idle(drafted, "Start"), cx).unwrap_err();
    assert_eq!(
        (error.code, error.message.as_str()),
        (ErrorCode::SessionBusy, BUSY)
    );
    // Mono-only notifications are refused for sessions.
    let notify = json!({"operation": "session_send", "input": {"sessionId": target, "prompt": "Hi", "notifyOnComplete": true}});
    let error = by(window, b, notify, cx).unwrap_err();
    assert_eq!(
        (error.code, error.message.as_str()),
        (
            ErrorCode::InvalidInput,
            "Completion notifications are only available in a Mono's chat"
        )
    );
    // An idle session without a draft takes it.
    let idle = add_session(window, HERE, false, cx);
    loaded(window, idle, cx);
    let sent: SessionSendOutput = output(by(window, b, if_idle(idle, "Go ahead"), cx));
    assert_eq!(sent.delivery, Delivery::Started);
    let typed = Command::SessionSend(SessionSendInput {
        session_id: target,
        prompt: "Typed".into(),
        if_idle: Some(true),
        ..SessionSendInput::default()
    });
    assert!(call(window, &session_caller_name(b), None, typed, cx).is_err());
}

#[gpui::test]
fn archive_with_stop_archives_a_working_session_and_closes_its_pane(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (a, b) = sessions(window, cx);
    set_busy(window, a, true, cx);
    let archive = |stop: bool| json!({"operation": "session_archive", "input": {"sessionId": a, "stop": stop}});
    let refused = by(window, b, archive(false), cx).unwrap_err();
    assert_eq!(
        refused.code,
        ErrorCode::SessionBusy,
        "without stop, as before"
    );
    let archived: SessionArchiveOutput = output(by(window, b, archive(true), cx));
    assert!(archived.archived);
    window
        .update(cx, |shell, _, cx| {
            assert!(shell.session_summaries[&a].archived);
            let view = shell.sessions[&a].read(cx);
            assert!(
                !view.busy && view.summary.archived,
                "stopped, then archived"
            );
            assert!(shell.workspace.tab_of(a).is_none(), "its pane closed");
            let stored = locked(&shell.store).unwrap().sessions().unwrap();
            assert!(stored.iter().find(|s| s.id == a).unwrap().archived);
        })
        .unwrap();
}

#[gpui::test]
fn a_refused_delete_reports_its_own_reason(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (a, b) = sessions(window, cx);
    let target = add_session(window, HERE, false, cx);
    window
        .update(cx, |shell, _, _| {
            shell.error = Some("An older, unrelated problem".into());
            shell.workspace.review_owners.insert(a, target);
        })
        .unwrap();
    let delete = json!({"operation": "session_delete", "input": {"sessionId": target}});
    let refused = by(window, b, delete, cx).unwrap_err();
    assert_eq!(
        refused.message,
        "Close this session's detached file panes before deleting it"
    );
    window
        .update(cx, |shell, _, _| {
            assert!(shell.session_summaries.contains_key(&target));
        })
        .unwrap();
}

#[gpui::test]
fn worktrees_folders_and_taking_a_session_out_of_its_folder(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (_, b) = sessions(window, cx);
    let repo = repository("to9");
    window
        .update(cx, |shell, _, cx| {
            shell.workspace.add_project(&repo);
            shell.session_summaries.get_mut(&b).unwrap().cwd = repo.clone();
            shell.sessions[&b].update(cx, |v, _| v.summary.cwd = repo.clone());
        })
        .unwrap();
    let created: WorktreeCreateOutput = output(call(
        window,
        &session_caller_name(b),
        None,
        Command::WorktreeCreate(WorktreeCreateInput {
            branch: "to9-branch".into(),
            ..WorktreeCreateInput::default()
        }),
        cx,
    ));
    let selected: SessionStartOutput = output(call(
        window,
        &session_caller_name(b),
        None,
        Command::SessionStart(SessionStartInput {
            prompt: "Work there".into(),
            draft: Some(true),
            workspace_mode: Some(WorkspaceMode::Current),
            worktree_cwd: Some(created.worktree.path.clone()),
            ..SessionStartInput::default()
        }),
        cx,
    ));
    assert_eq!(
        selected.worktree_cwd.as_deref(),
        Some(created.worktree.path.as_str())
    );
    let child = selected.session_id;
    let moved: SessionMoveOutput = output(by(
        window,
        b,
        json!({"operation": "session_move", "input": {"sessionId": child, "newFolderName": "Later"}}),
        cx,
    ));
    assert_eq!(moved.folder_name.as_deref(), Some("Later"));
    let folder = moved.folder_id.clone().unwrap();
    let both = json!({"operation": "session_move", "input": {"sessionId": child, "folderId": folder, "removeFromFolder": true}});
    assert_eq!(
        by(window, b, both, cx).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    let out: SessionMoveOutput = output(by(
        window,
        b,
        json!({"operation": "session_move", "input": {"sessionId": child, "removeFromFolder": true}}),
        cx,
    ));
    assert_eq!((out.folder_id, out.folder_name), (None, None));
    let folders: FolderListOutput = output(call(
        window,
        &session_caller_name(b),
        None,
        Command::FolderList(ProjectInput::default()),
        cx,
    ));
    assert!(folders.folders.is_empty(), "the emptied folder went too");
    window
        .update(cx, |shell, _, _| {
            let saved = locked(&shell.store).unwrap().groups(&repo).unwrap();
            assert!(saved.folders.is_empty(), "and the store agrees");
        })
        .unwrap();
}

#[gpui::test]
fn a_sessions_launches_split_beside_its_own_pane(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (a, b) = sessions(window, cx);
    // Its project compares by key: a trailing slash is the same project.
    window
        .update(cx, |shell, window, cx| {
            shell.session_summaries.get_mut(&b).unwrap().cwd = format!("{HERE}/");
            shell.sessions[&b].update(cx, |v, _| v.summary.cwd = format!("{HERE}/"));
            shell.open(a, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let place = |placement: &str, beside: Option<i64>| {
        let mut input = json!({"prompt": "Help out", "draft": true, "placement": placement});
        if let Some(beside) = beside {
            input["besideSessionId"] = json!(beside);
        }
        json!({"operation": "session_start", "input": input})
    };
    let right: SessionStartOutput = output(by(window, b, place("right", None), cx));
    let down: SessionStartOutput = output(by(window, b, place("down", Some(right.session_id)), cx));
    window
        .update(cx, |shell, _, _| {
            let tab = shell.workspace.tab_of(b).unwrap();
            let layout = shell.workspace.layout(tab);
            assert!(layout.contains(right.session_id) && layout.contains(down.session_id));
            assert_eq!(
                layout.leaf_ids().len(),
                3,
                "beside the caller, not the focus"
            );
            assert_eq!(shell.workspace.active, Some(down.session_id));
        })
        .unwrap();
    // A caller whose chat is loaded but not open has no pane to split.
    let closed = add_session(window, HERE, false, cx);
    window
        .update(cx, |shell, _, _| {
            assert!(shell.workspace.tab_of(closed).is_none())
        })
        .unwrap();
    let refused = by(window, closed, place("right", None), cx).unwrap_err();
    assert_eq!(
        refused.message,
        "besideSessionId must be an open session pane in this project"
    );
}

/// R8: stopping a session whose chat is not loaded loads it first, as
/// Operator's stop did, so its saved queue is paused: once the chat opens,
/// the queued message waits for the person instead of running.
#[gpui::test]
fn stopping_an_unloaded_session_pauses_its_saved_queue(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (_, b) = sessions(window, cx);
    let target = add_session(window, HERE, true, cx);
    window
        .update(cx, |shell, _, _| {
            let row = |role: &str, text: &str| monocode_core::transcript::Row {
                role: role.into(),
                text: text.into(),
                first: true,
                sent_at: None,
            };
            let mut store = locked(&shell.store).unwrap();
            store
                .write_rows(target, 0, &[row("user", "First"), row("assistant", "One")])
                .unwrap();
            let next = monocode_core::follow_up::Message {
                id: "next".into(),
                text: "Then this".into(),
                ..Default::default()
            };
            store
                .save_follow_ups(
                    target,
                    &monocode_core::follow_up::State {
                        messages: vec![next],
                        ..Default::default()
                    },
                )
                .unwrap();
            // As the store lists it now; no provider ever starts.
            let summary = shell.session_summaries.get_mut(&target).unwrap();
            summary.has_messages = true;
            summary.provider = "invalid-test-provider".into();
        })
        .unwrap();
    let stop = json!({"operation": "session_stop", "input": {"sessionId": target}});
    let stopped = by(window, b, stop, cx);
    assert!(stopped.is_ok(), "{stopped:?}");
    window
        .update(cx, |shell, _, cx| {
            let view = shell.sessions.get(&target).expect("loaded to stop it");
            let view = view.read(cx);
            let (messages, paused, _) = view.queued();
            assert!(paused, "the queue waits for the person");
            assert_eq!(messages.len(), 1, "and keeps its message");
            assert!(!view.busy, "nothing ran");
            let saved = locked(&shell.store).unwrap().follow_ups(target).unwrap();
            assert!(saved.paused && saved.messages.len() == 1, "{saved:?}");
        })
        .unwrap();
}
