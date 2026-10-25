//! A client the person denied is not asked about again for a while: its
//! hellos are refused at once, without a prompt or bringing the window
//! forward. Settings' Allow approves it and Revoke forgets the denial.
use super::tests::{approve, hello, setup};
use super::*;
use crate::settings::ExternalControlEdit;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::Scope;
use std::time::Duration;

fn edit(window: WindowHandle<Shell>, edit: ExternalControlEdit, cx: &mut TestAppContext) {
    window
        .update(cx, |shell, _, cx| shell.edit_control(&edit, cx))
        .unwrap();
    cx.run_until_parked();
}

fn denied(window: WindowHandle<Shell>, cx: &mut TestAppContext) -> Vec<String> {
    window
        .update(cx, |shell, _, _| shell.control_status().denied)
        .unwrap()
}

/// Says hello as `name` and denies the prompt.
fn deny(window: WindowHandle<Shell>, name: &str, cx: &mut TestAppContext) {
    let answer = hello(window, name, cx);
    assert!(cx.has_pending_prompt(), "{name} is asked");
    cx.simulate_prompt_answer("Deny");
    cx.run_until_parked();
    assert_eq!(
        answer.try_recv().unwrap().unwrap_err().code,
        ErrorCode::UserDeclined
    );
}

#[gpui::test]
fn a_denied_client_is_refused_without_asking_until_the_memory_ends(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[], cx);
    let memory = window
        .update(cx, |shell, _, _| {
            assert_eq!(
                shell.control.timeouts.denial_memory,
                Duration::from_secs(600)
            );
            // Ten minutes in the app; a simulated second here, since the
            // fixture's periodic timers all run while the clock advances.
            shell.control.timeouts.denial_memory = Duration::from_secs(1);
            shell.control.timeouts.denial_memory
        })
        .unwrap();
    deny(window, "Codex", cx);
    for _ in 0..3 {
        let again = hello(window, "Codex", cx);
        assert!(!cx.has_pending_prompt(), "a remembered denial asks nothing");
        let refused = again.try_recv().unwrap().unwrap_err();
        assert_eq!(refused.code, ErrorCode::UserDeclined);
        assert!(refused.message.contains("Settings"), "{}", refused.message);
    }
    assert_eq!(denied(window, cx), ["Codex"]);
    let decisions = window
        .update(cx, |shell, _, _| shell.control.activity.entries().count())
        .unwrap();
    assert_eq!(decisions, 1, "only the person's own answer is logged");
    cx.executor().advance_clock(memory * 2);
    cx.run_until_parked();
    deny(window, "Codex", cx);
}

#[gpui::test]
fn settings_allow_approves_a_denied_client_and_revoke_forgets_the_denial(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[], cx);
    deny(window, "Codex", cx);
    edit(window, ExternalControlEdit::Allow("Codex".into()), cx);
    assert!(denied(window, cx).is_empty());
    let welcomed = hello(window, "Codex", cx);
    assert!(!cx.has_pending_prompt(), "an allowed client is not asked");
    assert_eq!(
        welcomed.try_recv().unwrap().unwrap().scopes,
        Scope::DEFAULT_GRANT
    );
    deny(window, "Cursor", cx);
    edit(window, ExternalControlEdit::Revoke("Cursor".into()), cx);
    assert!(denied(window, cx).is_empty());
    let asked = hello(window, "Cursor", cx);
    assert!(
        cx.has_pending_prompt(),
        "after Revoke the person is asked again"
    );
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    assert!(asked.try_recv().unwrap().is_ok());
}
