//! `session_start`, `session_send`, `session_wait`, `session_stop` and the
//! session reads against the fixture shell. Sessions are made busy by hand
//! and launches are drafts, so no provider process ever starts.
use super::tests::{CLIENT, approve, call, set_busy, two_sessions};
use super::*;
use gpui::TestAppContext;
use monocode_control::{
    AccessMode, Command, Scope,
    ops::{
        dispatch::{
            Delivery, SendMode, SessionSendInput, SessionSendOutput, SessionStartInput,
            SessionStartOutput, SessionWaitInput, SessionWaitOutput,
        },
        session::{
            SessionInput, SessionListInput, SessionListOutput, SessionStatus as Status,
            SessionStatusOutput,
        },
    },
};
use std::{sync::mpsc, time::Duration};

const OTHER: &str = "/work/another-project";

fn start(
    window: gpui::WindowHandle<Shell>,
    request: Option<&str>,
    input: SessionStartInput,
    cx: &mut TestAppContext,
) -> Result<SessionStartOutput, ControlError> {
    let input = SessionStartInput {
        draft: Some(true),
        ..input
    };
    call(window, CLIENT, request, Command::SessionStart(input), cx)
        .map(|v| serde_json::from_value(v).unwrap())
}

fn review_prompt(project: Option<&str>) -> SessionStartInput {
    SessionStartInput {
        prompt: "Review this project".into(),
        project: project.map(str::to_owned),
        ..SessionStartInput::default()
    }
}

/// Sends a call that may settle later; returns where its answer arrives.
fn send(
    window: gpui::WindowHandle<Shell>,
    command: Command,
    cx: &mut TestAppContext,
) -> mpsc::Receiver<Result<Value, ControlError>> {
    let (reply, answer) = mpsc::sync_channel(1);
    window
        .update(cx, |shell, window, cx| {
            shell.handle_control(
                Request::Call {
                    caller: CLIENT.into(),
                    request_id: None,
                    command: Box::new(command),
                    reply,
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();
    answer
}

#[gpui::test]
fn session_start_launches_in_the_target_project_and_records_the_client(cx: &mut TestAppContext) {
    let (window, _, b) = two_sessions(&[Scope::Read, Scope::Dispatch], cx);
    let started = start(window, Some("start-1"), review_prompt(Some(OTHER)), cx).unwrap();
    assert_eq!(started.project, OTHER);
    assert!(started.draft && !started.submitted);
    assert_eq!(started.access, AccessMode::Supervised, "the app's default");
    let id = started.session_id;
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(shell.workspace.active, Some(b), "not revealed");
            assert_eq!(shell.session_metadata(id, cx).summary.cwd, OTHER);
            assert_eq!(shell.control.started[&id], "mcp:Claude Code");
            assert!(!shell.started_by.contains_key(&id), "no caller session");
            let view = shell.sessions[&id].read(cx);
            assert!(!view.settings.operator && !view.settings.operator_enabled);
            assert!(!view.settings.orchestrator && !view.settings.plan);
            assert!(view.has_saved_draft());
            let db = locked(&shell.store).unwrap();
            assert_eq!(db.started_clients().unwrap()[&id], "mcp:Claude Code");
            assert_eq!(
                db.follow_ups(id).unwrap().draft.unwrap().text,
                "Review this project"
            );
        })
        .unwrap();
    let count = |cx: &mut TestAppContext| {
        window
            .update(cx, |shell, _, _| shell.session_summaries.len())
            .unwrap()
    };
    let before = count(cx);
    let replay = start(window, Some("start-1"), review_prompt(Some(OTHER)), cx).unwrap();
    assert_eq!(replay, started, "a retry returns the first session");
    assert_eq!(count(cx), before, "and starts nothing");
    let conflict = start(window, Some("start-1"), review_prompt(None), cx).unwrap_err();
    assert_eq!(conflict.code, ErrorCode::RequestConflict);
    let fresh = start(window, None, review_prompt(Some(OTHER)), cx).unwrap();
    assert_ne!(fresh.session_id, id, "without a request ID it starts anew");
    let list: SessionListOutput = serde_json::from_value(
        call(
            window,
            CLIENT,
            None,
            Command::SessionList(SessionListInput {
                project: Some(OTHER.into()),
                ..SessionListInput::default()
            }),
            cx,
        )
        .unwrap(),
    )
    .unwrap();
    let listed = list.sessions.iter().find(|s| s.id == id).unwrap();
    assert_eq!(listed.started_by.as_deref(), Some("mcp:Claude Code"));
    assert_eq!(listed.status, Status::Idle);
    assert!(list.sessions.iter().all(|s| s.project == OTHER));
}

#[gpui::test]
fn session_start_gets_the_access_it_asks_for_else_the_default(cx: &mut TestAppContext) {
    let (window, _, _) = two_sessions(&[Scope::Read, Scope::Dispatch], cx);
    let full = start(
        window,
        None,
        SessionStartInput {
            access: Some(AccessMode::FullAccess),
            ..review_prompt(None)
        },
        cx,
    )
    .unwrap();
    assert_eq!(full.access, AccessMode::FullAccess, "there is no ceiling");
    let read_only = start(
        window,
        None,
        SessionStartInput {
            access: Some(AccessMode::ReadOnly),
            ..review_prompt(None)
        },
        cx,
    )
    .unwrap();
    assert_eq!(read_only.access, AccessMode::ReadOnly);
    // Without one it gets the app's default for new chats, as it is.
    cx.update(|cx| {
        let mut preferences = cx
            .try_global::<crate::settings::Preferences>()
            .cloned()
            .unwrap_or_default();
        preferences.default_access = Some("auto".into());
        cx.set_global(preferences);
    });
    let default = start(window, None, review_prompt(None), cx).unwrap();
    assert_eq!(default.access, AccessMode::Auto);
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(
                shell.sessions[&default.session_id].read(cx).settings.access,
                monocode_core::agent::AccessMode::Auto
            );
            assert_eq!(
                shell.sessions[&full.session_id].read(cx).settings.access,
                monocode_core::agent::AccessMode::FullAccess
            );
        })
        .unwrap();
}

#[gpui::test]
fn session_wait_resolves_when_the_turn_ends(cx: &mut TestAppContext) {
    let (window, _, b) = two_sessions(&[Scope::Read, Scope::Dispatch], cx);
    set_busy(window, b, true, cx);
    let wait = send(
        window,
        Command::SessionWait(SessionWaitInput {
            session_id: b,
            timeout_ms: Some(5_000),
        }),
        cx,
    );
    assert!(
        wait.try_recv().is_err(),
        "a working session keeps it waiting"
    );
    set_busy(window, b, false, cx);
    let done: SessionWaitOutput =
        serde_json::from_value(wait.try_recv().unwrap().unwrap()).unwrap();
    assert_eq!(done.status, Status::Idle);
    assert_eq!(done.status_text, "Ready");
    assert!(!done.timed_out);
}

#[gpui::test]
fn session_wait_times_out_while_the_session_works(cx: &mut TestAppContext) {
    let (window, _, b) = two_sessions(&[Scope::Read, Scope::Dispatch], cx);
    set_busy(window, b, true, cx);
    let wait = send(
        window,
        Command::SessionWait(SessionWaitInput {
            session_id: b,
            timeout_ms: Some(1_000),
        }),
        cx,
    );
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    assert!(wait.try_recv().is_err(), "still waiting before the timeout");
    cx.executor().advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
    let waited: SessionWaitOutput =
        serde_json::from_value(wait.try_recv().unwrap().unwrap()).unwrap();
    assert!(waited.timed_out);
    assert_eq!(waited.status, Status::Working);
    let invalid = call(
        window,
        CLIENT,
        None,
        Command::SessionWait(SessionWaitInput {
            session_id: b,
            timeout_ms: Some(60_001),
        }),
        cx,
    )
    .unwrap_err();
    assert_eq!(invalid.code, ErrorCode::InvalidInput);
}

#[gpui::test]
fn session_send_queues_or_steers_while_busy_and_stop_pauses(cx: &mut TestAppContext) {
    let (window, _, b) = two_sessions(&[Scope::Read, Scope::Dispatch], cx);
    set_busy(window, b, true, cx);
    let message = |mode, cx: &mut TestAppContext| {
        call(
            window,
            CLIENT,
            None,
            Command::SessionSend(SessionSendInput {
                session_id: b,
                prompt: "Also update the docs".into(),
                mode,
                ..SessionSendInput::default()
            }),
            cx,
        )
    };
    let queued: SessionSendOutput = serde_json::from_value(message(None, cx).unwrap()).unwrap();
    assert_eq!(queued.delivery, Delivery::Queued);
    assert_eq!(queued.queue_length, 1);
    let steered: SessionSendOutput =
        serde_json::from_value(message(Some(SendMode::Steer), cx).unwrap()).unwrap();
    assert_eq!(steered.delivery, Delivery::Steered);
    window
        .update(cx, |shell, _, _| {
            let queue = locked(&shell.store).unwrap().follow_ups(b).unwrap();
            let steer: Vec<_> = queue.messages.iter().map(|m| m.steer).collect();
            assert_eq!(steer, [false, true], "queue and steer are what was asked");
        })
        .unwrap();
    let status: SessionStatusOutput = serde_json::from_value(
        call(
            window,
            CLIENT,
            None,
            Command::SessionStatus(SessionInput { session_id: b }),
            cx,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(status.status, Status::Working);
    assert_eq!(status.queue_length, 2);
    assert_eq!(status.harness, "claude");
    let stopped = call(
        window,
        CLIENT,
        Some("stop-1"),
        Command::SessionStop(SessionInput { session_id: b }),
        cx,
    )
    .unwrap();
    assert_eq!(stopped["wasBusy"], true);
    window
        .update(cx, |shell, _, cx| {
            let queue = locked(&shell.store).unwrap().follow_ups(b).unwrap();
            assert!(queue.paused, "stopping pauses the queue");
            shell.sessions[&b].update(cx, |v, _| v.summary.provider = "grok".into());
        })
        .unwrap();
    let refused = message(Some(SendMode::Steer), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput, "Grok cannot steer");
    let missing = call(
        window,
        CLIENT,
        None,
        Command::SessionStatus(SessionInput { session_id: 999 }),
        cx,
    )
    .unwrap_err();
    assert_eq!(missing.code, ErrorCode::NotFound);
}

#[gpui::test]
fn dispatch_needs_its_scope_and_remote_work_is_refused(cx: &mut TestAppContext) {
    let (window, a, _) = two_sessions(&[Scope::Read, Scope::Dispatch], cx);
    approve(window, &[("Reader", &[Scope::Read])], cx);
    let denied = call(
        window,
        "Reader",
        None,
        Command::SessionStart(review_prompt(None)),
        cx,
    )
    .unwrap_err();
    assert_eq!(denied.code, ErrorCode::ScopeDenied);
    approve(window, &[(CLIENT, &[Scope::Read, Scope::Dispatch])], cx);
    window
        .update(cx, |shell, _, cx| {
            shell.workspace.add_project("remote://box/srv/app");
            shell.sessions[&a].update(cx, |v, _| v.summary.cwd = "remote://box/srv/app".into());
        })
        .unwrap();
    let remote = start(
        window,
        None,
        review_prompt(Some("remote://box/srv/app")),
        cx,
    )
    .unwrap_err();
    assert_eq!(remote.code, ErrorCode::RemoteUnsupported);
    let send = call(
        window,
        CLIENT,
        None,
        Command::SessionSend(SessionSendInput {
            session_id: a,
            prompt: "Hello".into(),
            ..SessionSendInput::default()
        }),
        cx,
    )
    .unwrap_err();
    assert_eq!(send.code, ErrorCode::RemoteUnsupported);
    let stop = call(
        window,
        CLIENT,
        None,
        Command::SessionStop(SessionInput { session_id: a }),
        cx,
    )
    .unwrap_err();
    assert_eq!(stop.code, ErrorCode::RemoteUnsupported);
    window
        .update(cx, |shell, _, _| {
            let log: Vec<_> = shell
                .control
                .activity
                .entries()
                .map(|e| (e.operation.as_str(), e.outcome.as_str()))
                .collect();
            assert_eq!(
                log,
                [
                    ("session_start", "scope_denied"),
                    ("session_start", "remote_unsupported"),
                    ("session_send", "remote_unsupported"),
                    ("session_stop", "remote_unsupported"),
                ]
            );
        })
        .unwrap();
}
