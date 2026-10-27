//! What a call may change and what it leaves behind: `session_configure`
//! changes one session and no preference, reads leave no trace in the
//! activity log, scheduling a prompt needs `dispatch` as well as `manage`,
//! removing a queued message is a confirmed destructive call, and a working
//! session is not archived.
use super::tests::{CLIENT, approve, call, sessions, setup};
use super::tests_automation::{seed, service};
use super::*;
use crate::settings::Preferences;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    AccessMode, Command, Scope,
    ops::{
        app::AppInfoInput,
        automation::{AutomationSetEnabledInput, AutomationUpsertInput},
        configure::SessionConfigureInput,
        dispatch::SessionSendInput,
        organize::{SessionArchiveInput, SessionRenameInput},
        project::ProjectListInput,
        queue::QueueItemInput,
        session::{SessionInput, SessionListInput},
    },
};
use std::sync::mpsc::Receiver;

fn log(window: WindowHandle<Shell>, cx: &mut TestAppContext) -> Vec<(String, String)> {
    window
        .update(cx, |shell, _, _| {
            shell
                .control
                .activity
                .entries()
                .map(|e| (e.operation.clone(), e.outcome.clone()))
                .collect()
        })
        .unwrap()
}

/// Sends a call that may wait for the person; returns where it answers.
fn ask(
    window: WindowHandle<Shell>,
    command: Command,
    cx: &mut TestAppContext,
) -> Receiver<Result<Value, ControlError>> {
    let (reply, answer) = std::sync::mpsc::sync_channel(1);
    window
        .update(cx, |shell, window, cx| {
            let request = Request::Call {
                caller: CLIENT.into(),
                request_id: None,
                command: Box::new(command),
                reply,
            };
            shell.handle_control(request, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    answer
}

#[gpui::test]
fn session_configure_changes_that_session_and_no_preference(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &[Scope::Read, Scope::Dispatch])], cx);
    let (_, b) = sessions(window, cx);
    let before = cx.update(|cx| {
        let preferences = Preferences {
            default_access: Some("auto".into()),
            ..cx.try_global::<Preferences>().cloned().unwrap_or_default()
        };
        cx.set_global(preferences.clone());
        preferences
    });
    let configure = Command::SessionConfigure(SessionConfigureInput {
        session_id: b,
        model: Some("claude-sonnet-4-5".into()),
        effort: Some("high".into()),
        model_settings: Some([("fast".to_owned(), "true".to_owned())].into()),
        access: Some(AccessMode::ReadOnly),
    });
    call(window, CLIENT, None, configure, cx).unwrap();
    window
        .update(cx, |shell, _, cx| {
            let view = shell.sessions[&b].read(cx);
            assert_eq!(
                view.settings.access,
                monocode_core::agent::AccessMode::ReadOnly
            );
            assert_eq!(view.settings.effort.as_deref(), Some("high"));
            assert!(view.settings.fast);
            assert_eq!(view.mobile_model(), Some("claude-sonnet-4-5"));
        })
        .unwrap();
    let after = cx.update(|cx| cx.global::<Preferences>().clone());
    assert_eq!(
        after, before,
        "no default access, effort or recent model was remembered for the person"
    );
}

#[gpui::test]
fn reads_leave_no_trace_in_the_activity_log(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(
        window,
        &[
            (CLIENT, &[Scope::Read, Scope::Manage]),
            ("Navigator", &[Scope::Navigate]),
        ],
        cx,
    );
    let (a, _) = sessions(window, cx);
    for command in [
        Command::AppInfo(AppInfoInput {}),
        Command::ProjectList(ProjectListInput::default()),
        Command::SessionList(SessionListInput::default()),
        Command::SessionStatus(SessionInput { session_id: a }),
    ] {
        call(window, CLIENT, None, command, cx).unwrap();
    }
    let denied = call(
        window,
        "Navigator",
        None,
        Command::ProjectList(ProjectListInput::default()),
        cx,
    );
    assert_eq!(denied.unwrap_err().code, ErrorCode::ScopeDenied);
    assert!(log(window, cx).is_empty(), "{:?}", log(window, cx));
    let saved = window
        .update(cx, |shell, _, _| {
            locked(&shell.store).unwrap().setting(ACTIVITY_KEY).unwrap()
        })
        .unwrap();
    assert_eq!(saved, None, "the stored log was never rewritten");
    let rename = Command::SessionRename(SessionRenameInput {
        session_id: a,
        title: "Parser fix".into(),
    });
    call(window, CLIENT, None, rename, cx).unwrap();
    assert_eq!(log(window, cx), [("session_rename".into(), "ok".into())]);
}

#[gpui::test]
fn scheduling_a_prompt_needs_dispatch_as_well_as_manage(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &[Scope::Read, Scope::Manage])], cx);
    let id = seed(window, "Nightly review", "supervised", cx);
    let enabled = |window, cx: &mut TestAppContext| {
        let all = service(window, cx).lock().unwrap().list().unwrap();
        all.iter().find(|a| a.id == id).unwrap().enabled
    };
    let set = |on| {
        Command::AutomationSetEnabled(AutomationSetEnabledInput {
            automation_id: id.clone(),
            enabled: on,
        })
    };
    let denied = call(window, CLIENT, None, set(true), cx).unwrap_err();
    assert_eq!(denied.code, ErrorCode::ScopeDenied);
    assert!(denied.message.contains("dispatch"), "{}", denied.message);
    assert!(!enabled(window, cx), "it stays off");
    call(window, CLIENT, None, set(false), cx).unwrap();
    let rename = AutomationUpsertInput {
        automation_id: Some(id.clone()),
        name: Some("Nightly digest".into()),
        ..AutomationUpsertInput::default()
    };
    call(
        window,
        CLIENT,
        None,
        Command::AutomationUpsert(rename.clone()),
        cx,
    )
    .unwrap();
    for input in [
        AutomationUpsertInput {
            prompt: Some("Delete the build folder".into()),
            ..rename.clone()
        },
        AutomationUpsertInput {
            enabled: Some(true),
            ..rename.clone()
        },
        AutomationUpsertInput {
            automation_id: None,
            prompt: Some("Review the open pull requests".into()),
            harness: Some("codex".into()),
            model: Some("gpt-5".into()),
            ..rename
        },
    ] {
        let denied = call(window, CLIENT, None, Command::AutomationUpsert(input), cx);
        assert_eq!(denied.unwrap_err().code, ErrorCode::ScopeDenied);
    }
    assert!(!enabled(window, cx));
    approve(
        window,
        &[(CLIENT, &[Scope::Read, Scope::Manage, Scope::Dispatch])],
        cx,
    );
    call(window, CLIENT, None, set(true), cx).unwrap();
    assert!(enabled(window, cx), "with both scopes it is turned on");
}

#[gpui::test]
fn removing_a_queued_message_is_destructive_and_confirmed(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &Scope::DEFAULT_GRANT)], cx);
    let (_, b) = sessions(window, cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&b].update(cx, |view, _| view.busy = true);
        })
        .unwrap();
    let send = Command::SessionSend(SessionSendInput {
        session_id: b,
        prompt: "Also update the docs".into(),
        ..SessionSendInput::default()
    });
    call(window, CLIENT, None, send, cx).unwrap();
    let message = window
        .update(cx, |shell, _, cx| {
            let (messages, _, _) = shell.sessions[&b].read(cx).queued();
            messages[0].id.clone()
        })
        .unwrap();
    let remove = Command::QueueRemove(QueueItemInput {
        session_id: b,
        message_id: message,
    });
    let denied = call(window, CLIENT, None, remove.clone(), cx).unwrap_err();
    assert_eq!(denied.code, ErrorCode::ScopeDenied);
    assert!(denied.message.contains("destructive"), "{}", denied.message);
    approve(window, &[(CLIENT, &Scope::ALL)], cx);
    let answer = ask(window, remove, cx);
    let (asked, quoted) = cx.pending_prompt().expect("the person is asked");
    assert!(
        asked.contains("wants to remove a queued message"),
        "{asked}"
    );
    assert!(quoted.contains("Also update the docs"), "{quoted}");
    cx.simulate_prompt_answer("Remove");
    cx.run_until_parked();
    answer.try_recv().unwrap().unwrap();
    let left = window
        .update(cx, |shell, _, cx| {
            shell.sessions[&b].read(cx).queue_length()
        })
        .unwrap();
    assert_eq!(left, 0);
}

#[gpui::test]
fn a_working_session_is_not_archived(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &[Scope::Read, Scope::Manage])], cx);
    let (_, b) = sessions(window, cx);
    let busy = |on, cx: &mut TestAppContext| {
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&b].update(cx, |view, _| view.busy = on);
            })
            .unwrap();
    };
    let archived = |cx: &mut TestAppContext| {
        window
            .update(cx, |shell, _, cx| {
                shell.session_metadata(b, cx).summary.archived
            })
            .unwrap()
    };
    busy(true, cx);
    let archive = Command::SessionArchive(SessionArchiveInput {
        session_id: b,
        stop: None,
    });
    let refused = call(window, CLIENT, None, archive.clone(), cx).unwrap_err();
    assert_eq!(
        (refused.code, refused.retryable),
        (ErrorCode::SessionBusy, true)
    );
    assert!(
        refused.message.contains("session_stop"),
        "{}",
        refused.message
    );
    assert!(!archived(cx), "its turn keeps running");
    busy(false, cx);
    call(window, CLIENT, None, archive, cx).unwrap();
    assert!(archived(cx));
}
