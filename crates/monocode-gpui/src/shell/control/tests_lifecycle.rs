//! `session_compact` and `session_handoff` against the fixture shell, and
//! the sessions `session_delete` refuses before asking. Compaction is only
//! refused here: starting it would launch the provider's CLI.
use super::tests::{CLIENT, run, set_busy, two_sessions};
use super::*;
use gpui::TestAppContext;
use monocode_control::{
    AccessMode, Command, Scope,
    ops::{
        lifecycle::{SessionCompactOutput, SessionHandoffInput, SessionHandoffOutput},
        session::SessionInput,
    },
};

fn compact(id: i64) -> Command {
    Command::SessionCompact(SessionInput { session_id: id })
}

#[gpui::test]
fn compaction_is_refused_when_the_chat_cannot_compact(cx: &mut TestAppContext) {
    let (window, _, b) = two_sessions(&Scope::ALL, cx);
    let cursor = window
        .update(cx, |shell, _, cx| {
            let summary = locked(&shell.store)
                .unwrap()
                .create("cursor", "/work/monocode")
                .unwrap();
            let id = summary.id;
            shell.add(summary, cx);
            shell.ensure_session(id, cx);
            id
        })
        .unwrap();
    cx.run_until_parked();
    let unsupported = run::<SessionCompactOutput>(window, compact(cursor), cx).unwrap_err();
    assert_eq!(unsupported.code, ErrorCode::InvalidInput);
    assert!(
        unsupported.message.contains("not supported"),
        "{}",
        unsupported.message
    );

    let fresh = run::<SessionCompactOutput>(window, compact(b), cx).unwrap_err();
    assert_eq!(fresh.code, ErrorCode::InvalidInput);
    assert!(
        fresh.message.contains("Send a message"),
        "{}",
        fresh.message
    );

    set_busy(window, b, true, cx);
    let busy = run::<SessionCompactOutput>(window, compact(b), cx).unwrap_err();
    assert_eq!(busy.code, ErrorCode::SessionBusy);
    assert!(busy.retryable);
    window
        .update(cx, |shell, _, cx| {
            let view = shell.sessions[&b].read(cx);
            assert!(view.busy, "a refusal starts nothing");
            assert_eq!(view.queue_length(), 0);
        })
        .unwrap();
    let missing = run::<SessionCompactOutput>(window, compact(999), cx).unwrap_err();
    assert_eq!(missing.code, ErrorCode::NotFound);
}

fn handoff(id: i64, access: Option<AccessMode>) -> Command {
    Command::SessionHandoff(SessionHandoffInput {
        session_id: id,
        harness: "claude".into(),
        model: Some("claude-sonnet-4-5".into()),
        access,
        ..SessionHandoffInput::default()
    })
}

#[gpui::test]
fn handoff_starts_a_new_session_with_the_brief_and_the_access_asked_for(cx: &mut TestAppContext) {
    let (window, a, _) = two_sessions(&Scope::ALL, cx);
    let count = |window: gpui::WindowHandle<Shell>, cx: &mut TestAppContext| {
        window
            .update(cx, |shell, _, _| shell.session_summaries.len())
            .unwrap()
    };
    let before = count(window, cx);
    let unknown = run::<SessionHandoffOutput>(
        window,
        Command::SessionHandoff(SessionHandoffInput {
            session_id: a,
            harness: "nope".into(),
            ..SessionHandoffInput::default()
        }),
        cx,
    );
    assert_eq!(unknown.unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(count(window, cx), before, "a refusal creates nothing");

    let out: SessionHandoffOutput =
        run(window, handoff(a, Some(AccessMode::ReadOnly)), cx).unwrap();
    assert_eq!(out.from_session_id, a);
    assert_eq!(out.harness, "claude");
    assert_eq!(out.access, AccessMode::ReadOnly);
    assert_eq!(count(window, cx), before + 1);
    window
        .update(cx, |shell, _, cx| {
            let view = shell.sessions[&out.session_id].read(cx);
            assert_eq!(view.summary.provider, "claude");
            assert_eq!(view.summary.cwd, "/work/another-project");
            assert_eq!(view.settings.model.as_deref(), Some("claude-sonnet-4-5"));
            assert_eq!(
                view.settings.access,
                monocode_core::agent::AccessMode::ReadOnly
            );
            let brief = &view
                .settings
                .handoff
                .as_ref()
                .expect("a handoff brief")
                .brief;
            assert!(brief.contains("History loaded"), "{brief}");
            assert!(!view.busy, "the new session waits for its first message");
            assert_eq!(
                shell
                    .control
                    .started
                    .get(&out.session_id)
                    .map(String::as_str),
                Some("mcp:Claude Code")
            );
        })
        .unwrap();

    // Any mode asked for is used; without one, the app's default for new
    // chats.
    let full: SessionHandoffOutput =
        run(window, handoff(a, Some(AccessMode::FullAccess)), cx).unwrap();
    assert_eq!(full.access, AccessMode::FullAccess);
    let default: SessionHandoffOutput = run(window, handoff(a, None), cx).unwrap();
    assert_eq!(default.access, AccessMode::Supervised);

    set_busy(window, a, true, cx);
    let busy = run::<SessionHandoffOutput>(window, handoff(a, None), cx);
    assert_eq!(busy.unwrap_err().code, ErrorCode::SessionBusy);
}

#[gpui::test]
fn orchestration_workers_are_refused_before_anyone_is_asked(cx: &mut TestAppContext) {
    let (window, a, b) = two_sessions(&Scope::ALL, cx);
    window
        .update(cx, |shell, _, _| {
            shell.orchestration_summaries.insert(
                a,
                monocode_core::orchestration::Summary {
                    tasks: vec![monocode_core::orchestration::SummaryTask {
                        session: Some(b),
                        title: "Worker".into(),
                        ..Default::default()
                    }],
                    live: false,
                },
            );
        })
        .unwrap();
    let (reply, answer) = std::sync::mpsc::sync_channel(1);
    window
        .update(cx, |shell, window, cx| {
            shell.handle_control(
                Request::Call {
                    caller: CLIENT.into(),
                    request_id: None,
                    command: Command::SessionDelete(SessionInput { session_id: b }),
                    reply,
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();
    let refused = answer.try_recv().unwrap().unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput);
    assert!(
        refused.message.contains("orchestration worker"),
        "{}",
        refused.message
    );
    assert!(!cx.has_pending_prompt());
    window
        .update(cx, |shell, _, _| {
            assert!(shell.session_summaries.contains_key(&b))
        })
        .unwrap();
}
