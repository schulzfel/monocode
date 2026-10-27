//! An agent session's old Operator actions, ported from the tests of the
//! old in-app dispatch they were first written against: each action is
//! translated as the `app` command translates it (`app_cli::legacy`) and
//! called through the control pipeline as the session's relay calls it. The assertions are
//! the old tests'; where the answer differs, a comment names the accepted
//! change of the spec (§13). Worktrees, sends, stops and archives are in
//! `tests_operator_ops`.
use super::*;
use crate::app_cli::legacy;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{Command, session_caller_name};
use monocode_core::agent::Settings;
use serde_json::{Value, json};

/// The fixture window, with `caller` as an Operator thread was set up.
pub(super) fn setup(cx: &mut TestAppContext) -> (WindowHandle<Shell>, i64, i64) {
    let (window, other, caller) = crate::shell::tests::window(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |v, cx| {
                v.install_transfer(
                    Settings {
                        operator_enabled: true,
                        ..Default::default()
                    },
                    None,
                    cx,
                );
            });
        })
        .unwrap();
    cx.run_until_parked();
    (window, other, caller)
}

/// Old action `action` as session `caller` calls it on the control
/// pipeline; a refusal as its message.
pub(super) fn call(
    window: WindowHandle<Shell>,
    caller: i64,
    request: &str,
    action: &str,
    input: Value,
    cx: &mut TestAppContext,
) -> Result<Value, String> {
    let wire = legacy::translate(action, &input)?;
    let command: Command = serde_json::from_value(wire).map_err(|e| e.to_string())?;
    super::tests::call(
        window,
        &session_caller_name(caller),
        Some(request),
        command,
        cx,
    )
    .map_err(|e| e.message)
}

/// An unsent launch from `caller`, so no provider starts.
pub(super) fn launch(
    window: WindowHandle<Shell>,
    caller: i64,
    request: &str,
    input: Value,
    cx: &mut TestAppContext,
) -> i64 {
    let mut input = input;
    input["draft"] = json!(true);
    input["prompt"] = json!("Review this project");
    call(window, caller, request, "sessions.start", input, cx).unwrap()["sessionId"]
        .as_i64()
        .unwrap()
}

#[gpui::test]
fn submitted_operator_access_survives_the_composer_reset(cx: &mut TestAppContext) {
    let (window, _, caller) = crate::shell::tests::window(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |view, cx| {
                // Save the turn, then fail without starting a real provider.
                view.summary.provider = "fixture".into();
                view.quick_submit(
                    "/operator Start a test session".into(),
                    Settings::default(),
                    cx,
                );
                assert!(view.busy);
                assert!(!view.settings.operator);
                assert!(view.settings.operator_enabled);
            });
        })
        .unwrap();
    assert!(
        call(
            window,
            caller,
            "after-submit",
            "sessions.list",
            json!({}),
            cx
        )
        .is_ok()
    );
    assert!(call(window, caller, "later", "sessions.list", json!({}), cx).is_ok());
    window
        .update(cx, |shell, _, cx| {
            let saved = locked(&shell.store)
                .unwrap()
                .agent_settings(caller)
                .unwrap()
                .unwrap();
            assert!(!saved.operator);
            assert!(saved.operator_enabled);
            shell.sessions[&caller].update(cx, |view, cx| {
                view.quick_submit("List my sessions".into(), saved, cx);
                assert!(!view.settings.operator);
                assert!(view.settings.operator_enabled);
            });
        })
        .unwrap();
    assert!(call(window, caller, "next-turn", "sessions.list", json!({}), cx).is_ok());
    let child = launch(
        window,
        caller,
        "after-reset",
        json!({"harness":"codex"}),
        cx,
    );
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.sessions[&child].read(cx).settings.operator_enabled);
        })
        .unwrap();
}

/// C-2: every session may call without `/operator`, so an Operator mode
/// picked but never sent is no longer refused ("Operator is not enabled for
/// this thread.").
#[gpui::test]
fn an_unsent_operator_selection_is_not_needed_for_app_access(cx: &mut TestAppContext) {
    let (window, _, caller) = crate::shell::tests::window(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |view, cx| view.enable_mode("operator", cx));
        })
        .unwrap();
    assert!(call(window, caller, "unsent", "sessions.list", json!({}), cx).is_ok());
}

#[gpui::test]
fn operator_planning_turns_keep_read_only_app_access_after_submission(cx: &mut TestAppContext) {
    for mode in ["plan", "orchestrator"] {
        let (window, _, caller) = setup(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&caller].update(cx, |view, cx| {
                    view.summary.provider = "fixture".into();
                    view.quick_submit(
                        format!("/{mode} Inspect my sessions"),
                        view.settings.clone(),
                        cx,
                    );
                    assert!(view.busy);
                    assert!(!view.settings.plan && !view.settings.orchestrator);
                });
            })
            .unwrap();
        assert!(call(window, caller, "plan-read", "sessions.list", json!({}), cx).is_ok());
        assert!(
            call(
                window,
                caller,
                "plan-write",
                "notes.write",
                json!({"title":"Do not save", "body":"Plan only"}),
                cx,
            )
            .unwrap_err()
            .contains("read-only")
        );
    }
}

#[gpui::test]
fn operator_scopes_sessions_and_preserves_an_unsent_draft(cx: &mut TestAppContext) {
    let (window, other, caller) = setup(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |v, cx| {
                v.input
                    .update(cx, |i, cx| i.set_text("keep my draft".into(), cx))
            })
        })
        .unwrap();
    let list = call(window, caller, "list", "sessions.list", json!({}), cx).unwrap();
    assert!(
        list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == caller)
    );
    assert!(
        !list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == other)
    );
    // C-2: a session may address another project's sessions; the bridge
    // refused them ("Session is outside this project.").
    assert!(
        call(
            window,
            caller,
            "outside",
            "sessions.read",
            json!({"sessionId":other}),
            cx
        )
        .is_ok()
    );
    assert!(
        call(
            window,
            caller,
            "self-draft",
            "sessions.draft",
            json!({"sessionId":caller,"prompt":"replace"}),
            cx
        )
        .is_err()
    );
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(
                shell.sessions[&caller].read(cx).input.read(cx).content,
                "keep my draft"
            );
            shell.sessions[&caller].update(cx, |v, _| {
                v.settings.access = monocode_core::agent::AccessMode::ReadOnly
            });
        })
        .unwrap();
    assert!(
        call(
            window,
            caller,
            "readonly",
            "notes.write",
            json!({"title":"Do not save","body":"Read only"}),
            cx
        )
        .unwrap_err()
        .contains("read-only")
    );
}

#[gpui::test]
fn operator_children_are_visible_durable_and_do_not_navigate_by_default(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let child = launch(
        window,
        caller,
        "draft",
        json!({"runtimeMode":"read-only"}),
        cx,
    );
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(shell.workspace.active, Some(caller));
            assert!(shell.sidebar_session_ids(cx).contains(&child));
            assert!(!shell.sidebar_hidden_sessions.contains(&child));
            assert!(!shell.monos.as_ref().unwrap().read(cx).hidden_session(child));
            let view = shell.sessions[&child].read(cx);
            assert_eq!(
                view.settings.access,
                monocode_core::agent::AccessMode::ReadOnly
            );
            assert!(view.has_saved_draft());
            assert!(!view.busy);
            let db = locked(&shell.store).unwrap();
            assert_eq!(
                db.follow_ups(child).unwrap().draft.unwrap().text,
                "Review this project"
            );
            assert_eq!(db.started_links().unwrap()[&child], caller);
            assert!(db.draft_sessions().unwrap().contains(&child));
        })
        .unwrap();
    let opened = launch(window, caller, "visible", json!({"reveal":true}), cx);
    window
        .update(cx, |shell, _, _| {
            assert_eq!(shell.workspace.active, Some(opened))
        })
        .unwrap();
    assert!(
        call(
            window,
            caller,
            "hidden",
            "sessions.start",
            json!({"prompt":"Do not hide","draft":true,"sidebarHidden":true}),
            cx
        )
        .is_err()
    );
}

#[gpui::test]
fn operator_launches_can_build_nested_splits(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let right = launch(window, caller, "right", json!({"placement":"right"}), cx);
    let down = launch(
        window,
        caller,
        "down",
        json!({"placement":"down","besideSessionId":right}),
        cx,
    );
    window
        .update(cx, |shell, _, _| {
            let tab = shell.workspace.tab_of(caller).unwrap();
            let layout = shell.workspace.layout(tab);
            assert!(layout.contains(right) && layout.contains(down));
            assert_eq!(layout.leaf_ids().len(), 3);
            assert_eq!(shell.workspace.active, Some(down));
        })
        .unwrap();
}

#[gpui::test]
fn operator_retries_use_durable_receipts_and_reject_changed_input(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let child = launch(window, caller, "repeat", json!({}), cx);
    // The bridge's results in memory were cleared here, as after a restart;
    // the control pipeline keeps its receipts in the store alone (C-5).
    let retry = launch(window, caller, "repeat", json!({}), cx);
    assert_eq!(retry, child);
    assert!(
        call(
            window,
            caller,
            "repeat",
            "sessions.start",
            json!({"draft":true,"prompt":"Different input"}),
            cx
        )
        .unwrap_err()
        .contains("different input")
    );
    window
        .update(cx, |shell, _, _| {
            assert_eq!(
                shell
                    .started_by
                    .values()
                    .filter(|id| **id == caller)
                    .count(),
                1
            )
        })
        .unwrap();
}

#[gpui::test]
fn operator_notes_and_conversation_paging_use_the_project_api(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let child = launch(window, caller, "read-child", json!({}), cx);
    let note = call(
        window,
        caller,
        "note",
        "notes.write",
        json!({"body":"# Review notes\n\nKeep this","tags":["work"]}),
        cx,
    )
    .unwrap();
    let changed = call(
        window,
        caller,
        "note-edit",
        "notes.write",
        json!({"id":note["id"],"tags":["done"]}),
        cx,
    )
    .unwrap();
    assert_eq!(changed["body"], note["body"]);
    assert_eq!(changed["tags"], json!(["done"]));
    let page = call(
        window,
        caller,
        "note-page",
        "notes.list",
        json!({"limit":1,"offset":0}),
        cx,
    )
    .unwrap();
    assert_eq!(page["total"], 1);
    assert_eq!(page["notes"][0]["id"], note["id"]);
    let read = call(
        window,
        caller,
        "read",
        "sessions.read",
        json!({"sessionId":child,"limit":1,"maxChars":200}),
        cx,
    )
    .unwrap();
    assert_eq!(read["hasDraft"], true);
    assert_eq!(read["turns"], json!([]));
    assert!(
        call(
            window,
            caller,
            "ignored-field",
            "sessions.read",
            json!({"sessionId":child,"typo":1}),
            cx
        )
        .is_err()
    );
}
