//! Which results a request ID keeps: a failure worth retrying keeps none, so
//! the retry runs; and each client keeps a bounded number of receipts.
use super::tests::{CLIENT, approve, call, setup};
use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    AccessMode, Command, Scope,
    ops::{configure::SessionConfigureInput, project::ProjectListInput},
};

fn kept(window: WindowHandle<Shell>, client: &str, request: &str, cx: &mut TestAppContext) -> bool {
    let key = receipts::key(client, request);
    window
        .update(cx, |shell, _, _| {
            locked(&shell.store)
                .unwrap()
                .setting(&key)
                .unwrap()
                .is_some()
        })
        .unwrap()
}

#[gpui::test]
fn a_retryable_failure_keeps_no_receipt_so_the_retry_runs(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &[Scope::Read, Scope::Dispatch])], cx);
    let b = window
        .update(cx, |shell, _, _| {
            *shell.session_summaries.keys().max().unwrap()
        })
        .unwrap();
    let busy = |on, cx: &mut TestAppContext| {
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&b].update(cx, |view, _| view.busy = on);
            })
            .unwrap();
    };
    let configure = Command::SessionConfigure(SessionConfigureInput {
        session_id: b,
        access: Some(AccessMode::ReadOnly),
        ..SessionConfigureInput::default()
    });
    busy(true, cx);
    let refused = call(window, CLIENT, Some("cfg-1"), configure.clone(), cx).unwrap_err();
    assert_eq!(
        (refused.code, refused.retryable),
        (ErrorCode::SessionBusy, true)
    );
    assert!(
        !kept(window, CLIENT, "cfg-1", cx),
        "a busy session is worth a retry"
    );
    busy(false, cx);
    let done = call(window, CLIENT, Some("cfg-1"), configure, cx).unwrap();
    assert_eq!(done["access"], "read-only", "the retry ran");
    assert!(kept(window, CLIENT, "cfg-1", cx), "and its result is kept");
}

#[gpui::test]
fn each_client_keeps_its_newest_receipts_only(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(
        window,
        &[(CLIENT, &[Scope::Read]), ("Codex", &[Scope::Read])],
        cx,
    );
    let list = || Command::ProjectList(ProjectListInput::default());
    call(window, "Codex", Some("r0"), list(), cx).unwrap();
    let over = receipts::MAX_PER_CLIENT + 5;
    for n in 0..over {
        call(window, CLIENT, Some(&format!("r{n}")), list(), cx).unwrap();
    }
    for n in 0..5 {
        assert!(
            !kept(window, CLIENT, &format!("r{n}"), cx),
            "r{n} is pruned"
        );
    }
    for n in [5, over - 1] {
        assert!(kept(window, CLIENT, &format!("r{n}"), cx), "r{n} is kept");
    }
    assert!(
        kept(window, "Codex", "r0", cx),
        "another client's receipts are its own"
    );
}

/// R9: the grant is checked again once the receipt lookup is done, as
/// Operator authorized again after it (OP-33): a turn that became read-only
/// meanwhile is refused, keeps no receipt, and a retry runs.
#[gpui::test]
fn a_grant_lost_during_the_receipt_lookup_refuses_without_a_receipt(cx: &mut TestAppContext) {
    let window = setup(cx);
    let b = window
        .update(cx, |shell, _, _| {
            *shell.session_summaries.keys().max().unwrap()
        })
        .unwrap();
    let caller = CallerKind::Session(b).name();
    let write = || -> Command {
        serde_json::from_value(serde_json::json!({
            "operation": "notes_write",
            "input": {"title": "Plan", "body": "Ship"}
        }))
        .unwrap()
    };
    let plans = |cx: &mut TestAppContext| {
        window
            .update(cx, |shell, _, _| {
                let notes = locked(&shell.store).unwrap().list_notes().unwrap();
                notes.iter().filter(|n| n.title == "Plan").count()
            })
            .unwrap()
    };
    let reply = window
        .update(cx, |shell, window, cx| {
            let reply = shell.control_call_as(b, Some("plan-1"), write(), window, cx);
            // The person turns plan mode on while the lookup runs.
            shell.sessions[&b].update(cx, |view, _| view.settings.plan = true);
            reply
        })
        .unwrap();
    cx.run_until_parked();
    let refused = reply.try_recv().unwrap().unwrap_err();
    assert_eq!(
        (refused.code, refused.message.as_str()),
        (ErrorCode::ScopeDenied, gate::READ_ONLY)
    );
    assert!(!kept(window, &caller, "plan-1", cx), "a refusal keeps none");
    assert_eq!(plans(cx), 0, "nothing was written");
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&b].update(cx, |view, _| view.settings.plan = false);
        })
        .unwrap();
    let reply = window
        .update(cx, |shell, window, cx| {
            shell.control_call_as(b, Some("plan-1"), write(), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    assert!(reply.try_recv().unwrap().is_ok(), "the retry runs");
    assert_eq!(plans(cx), 1);
    assert!(kept(window, &caller, "plan-1", cx));
}
