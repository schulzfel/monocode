//! Confirmation prompts for every operation that deletes, discards or
//! publishes: confirmed it runs; declined or unanswered it fails with
//! `user_declined`, changes nothing and keeps no receipt.
use super::tests::{CLIENT, approve, scratch, setup};
use super::tests_automation::{seed, service};
use super::tests_vcs::{git, local_repo};
use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    Command, Scope,
    ops::{
        automation::AutomationInput, organize::ProjectInput, rail::ProjectTarget,
        session::SessionInput, vcs::GitPathsInput,
    },
};
use std::{path::Path, sync::mpsc::Receiver, time::Duration};

const ALL: [Scope; 7] = Scope::ALL;

type Answer = Receiver<Result<Value, ControlError>>;
type Check = Box<dyn Fn(WindowHandle<Shell>, &mut TestAppContext) -> bool>;

/// One confirmed call: what it asks and whether it changed anything yet.
struct Case {
    command: Command,
    asks: String,
    button: &'static str,
    changed: Check,
}

fn window(cx: &mut TestAppContext) -> WindowHandle<Shell> {
    let window = setup(cx);
    approve(window, &[(CLIENT, &ALL)], cx);
    window
}

fn send(
    window: WindowHandle<Shell>,
    request: Option<&str>,
    command: Command,
    cx: &mut TestAppContext,
) -> Answer {
    send_as(window, CLIENT, request, command, cx)
}

fn send_as(
    window: WindowHandle<Shell>,
    client: &str,
    request: Option<&str>,
    command: Command,
    cx: &mut TestAppContext,
) -> Answer {
    let (reply, answer) = std::sync::mpsc::sync_channel(1);
    window
        .update(cx, |shell, window, cx| {
            shell.handle_control(
                Request::Call {
                    caller: client.into(),
                    request_id: request.map(str::to_owned),
                    command,
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

fn session_case(window: WindowHandle<Shell>, cx: &mut TestAppContext) -> Case {
    let id = window
        .update(cx, |shell, _, _| {
            *shell.session_summaries.keys().min().unwrap()
        })
        .unwrap();
    Case {
        command: Command::SessionDelete(SessionInput { session_id: id }),
        asks: format!("“{CLIENT}” wants to delete the session"),
        button: "Delete",
        changed: Box::new(move |window, cx| {
            window
                .update(cx, |shell, _, _| !shell.session_summaries.contains_key(&id))
                .unwrap()
        }),
    }
}

/// A project folder on disk with one saved session.
fn project_case(name: &str, window: WindowHandle<Shell>, cx: &mut TestAppContext) -> Case {
    let folder = scratch(name).join("project");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("notes.txt"), "keep me\n").unwrap();
    let path = folder.display().to_string();
    window
        .update(cx, |shell, _, cx| {
            shell.workspace.add_project(&path);
            let summary = locked(&shell.store)
                .unwrap()
                .create("codex", &path)
                .unwrap();
            shell.add(summary, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let project = path.clone();
    Case {
        command: Command::ProjectDelete(ProjectTarget { project }),
        asks: format!("“{CLIENT}” wants to delete the project “project” from MonoCode"),
        button: "Delete",
        changed: Box::new(move |window, cx| {
            assert!(
                Path::new(&path).join("notes.txt").is_file(),
                "the folder on disk is never touched"
            );
            window
                .update(cx, |shell, _, _| !shell.workspace.projects.contains(&path))
                .unwrap()
        }),
    }
}

fn automation_case(window: WindowHandle<Shell>, cx: &mut TestAppContext) -> Case {
    let id = seed(window, "Weekly digest", "supervised", cx);
    Case {
        command: Command::AutomationDelete(AutomationInput {
            automation_id: id.clone(),
        }),
        asks: format!("“{CLIENT}” wants to delete the automation “Weekly digest”"),
        button: "Delete",
        changed: Box::new(move |window, cx| {
            let all = service(window, cx).lock().unwrap().list().unwrap();
            !all.iter().any(|a| a.id == id)
        }),
    }
}

fn discard_case(name: &str, window: WindowHandle<Shell>, cx: &mut TestAppContext) -> Case {
    let repo = local_repo(name);
    std::fs::write(Path::new(&repo).join("README.md"), "unsaved work\n").unwrap();
    window
        .update(cx, |shell, _, _| shell.workspace.add_project(&repo))
        .unwrap();
    Case {
        command: Command::GitDiscard(GitPathsInput {
            project: Some(repo.clone()),
            paths: vec!["README.md".into()],
            all: None,
        }),
        asks: format!("“{CLIENT}” wants to discard changes in"),
        button: "Discard",
        changed: Box::new(move |_, _| {
            std::fs::read_to_string(Path::new(&repo).join("README.md")).unwrap() == "hello\n"
        }),
    }
}

/// A repository with a bare remote beside it, both under `target/c/`.
fn push_case(name: &str, window: WindowHandle<Shell>, cx: &mut TestAppContext) -> Case {
    let repo = local_repo(name);
    let remote = Path::new(&repo).parent().unwrap().join("remote.git");
    let remote = remote.to_str().unwrap().to_owned();
    git(&repo, &["init", "-q", "--bare", &remote]);
    git(&repo, &["remote", "add", "origin", &remote]);
    window
        .update(cx, |shell, _, _| shell.workspace.add_project(&repo))
        .unwrap();
    Case {
        command: Command::GitPush(ProjectInput {
            project: Some(repo.clone()),
        }),
        asks: format!("“{CLIENT}” wants to push the branch “main” of"),
        button: "Push",
        changed: Box::new(move |_, _| {
            !git(
                &remote,
                &["for-each-ref", "--format=%(refname)", "refs/heads"],
            )
            .is_empty()
        }),
    }
}

/// Every confirmed operation, each in its own scratch folders.
fn cases(round: &str, window: WindowHandle<Shell>, cx: &mut TestAppContext) -> Vec<Case> {
    vec![
        session_case(window, cx),
        project_case(&format!("q4{round}"), window, cx),
        automation_case(window, cx),
        discard_case(&format!("q5{round}"), window, cx),
        push_case(&format!("q6{round}"), window, cx),
    ]
}

fn asked(case: &Case, cx: &TestAppContext) {
    let (message, _) = cx
        .pending_prompt()
        .expect("the person is asked in the window");
    assert!(message.contains(&case.asks), "{message}");
}

fn declined(answer: &Answer, case: &Case) {
    let error = answer
        .try_recv()
        .expect("the call has settled")
        .unwrap_err();
    assert_eq!(
        error.code,
        ErrorCode::UserDeclined,
        "{:?}: {}",
        case.command.name(),
        error.message
    );
}

fn activity(window: WindowHandle<Shell>, cx: &mut TestAppContext) -> Vec<(String, String)> {
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

#[gpui::test]
fn a_declined_call_changes_nothing_and_keeps_no_receipt(cx: &mut TestAppContext) {
    let window = window(cx);
    for (ix, case) in cases("a", window, cx).into_iter().enumerate() {
        let request = format!("decline-{ix}");
        let answer = send(window, Some(&request), case.command.clone(), cx);
        asked(&case, cx);
        assert!(answer.try_recv().is_err(), "nothing runs before the answer");
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        declined(&answer, &case);
        assert!(
            !case.changed.as_ref()(window, cx),
            "{}",
            case.command.name()
        );
        let key = receipts::key(CLIENT, &request);
        let kept = window
            .update(cx, |shell, _, _| {
                locked(&shell.store).unwrap().setting(&key).unwrap()
            })
            .unwrap();
        assert!(kept.is_none(), "a declined call keeps no receipt");
        let log = activity(window, cx);
        assert_eq!(
            log[log.len() - 2..],
            [
                ("decline".to_owned(), "user_declined".to_owned()),
                (case.command.name().to_owned(), "user_declined".to_owned())
            ]
        );
        // The same request ID asks again, and confirming it now runs it.
        let answer = send(window, Some(&request), case.command.clone(), cx);
        asked(&case, cx);
        cx.simulate_prompt_answer(case.button);
        cx.run_until_parked();
        let done = answer.try_recv().expect("the call has settled");
        assert!(done.is_ok(), "{}: {done:?}", case.command.name());
        assert!(case.changed.as_ref()(window, cx), "{}", case.command.name());
        assert_eq!(
            activity(window, cx).iter().rev().nth(1).unwrap().0,
            "confirm"
        );
    }
}

#[gpui::test]
fn no_answer_in_two_minutes_declines_and_a_late_answer_does_nothing(cx: &mut TestAppContext) {
    let window = window(cx);
    let timeout = window
        .update(cx, |shell, _, _| {
            assert_eq!(shell.control.timeouts.confirm, Duration::from_secs(120));
            shell.control.timeouts.confirm = Duration::from_secs(1);
            shell.control.timeouts.confirm
        })
        .unwrap();
    for case in cases("b", window, cx) {
        let answer = send(window, None, case.command.clone(), cx);
        asked(&case, cx);
        cx.executor().advance_clock(timeout / 2);
        cx.run_until_parked();
        assert!(
            answer.try_recv().is_err(),
            "still waiting before the timeout"
        );
        cx.executor().advance_clock(timeout);
        cx.run_until_parked();
        declined(&answer, &case);
        cx.simulate_prompt_answer(case.button);
        cx.run_until_parked();
        assert!(
            !case.changed.as_ref()(window, cx),
            "{}",
            case.command.name()
        );
    }
}

#[gpui::test]
fn one_prompt_per_client_and_turning_control_off_refuses_the_wait(cx: &mut TestAppContext) {
    let window = window(cx);
    let first = session_case(window, cx);
    let second = automation_case(window, cx);
    let waiting = send(window, Some("first"), first.command.clone(), cx);
    asked(&first, cx);
    let refused = send(window, Some("second"), second.command.clone(), cx)
        .try_recv()
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::RequestConflict);
    assert!(refused.retryable);
    window
        .update(cx, |shell, _, cx| {
            shell.edit_control(&crate::settings::ExternalControlEdit::Enabled(false), cx);
        })
        .unwrap();
    cx.run_until_parked();
    let off = waiting.try_recv().unwrap().unwrap_err();
    assert_eq!(off.code, ErrorCode::ControlDisabled);
    cx.simulate_prompt_answer("Delete");
    cx.run_until_parked();
    assert!(
        !first.changed.as_ref()(window, cx),
        "a stale prompt deletes nothing"
    );
    assert!(!second.changed.as_ref()(window, cx));
}

#[gpui::test]
fn deletes_need_the_destructive_scope_and_are_asked_only_for_real_targets(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &Scope::DEFAULT_GRANT)], cx);
    let case = session_case(window, cx);
    let refused = send(window, None, case.command.clone(), cx)
        .try_recv()
        .unwrap();
    assert_eq!(refused.unwrap_err().code, ErrorCode::ScopeDenied);
    assert!(!cx.has_pending_prompt(), "a refused scope asks nothing");
    approve(window, &[(CLIENT, &ALL)], cx);
    let missing = send(
        window,
        None,
        Command::SessionDelete(SessionInput { session_id: 999 }),
        cx,
    )
    .try_recv()
    .unwrap();
    assert_eq!(missing.unwrap_err().code, ErrorCode::NotFound);
    let missing = send(
        window,
        None,
        Command::ProjectDelete(ProjectTarget {
            project: "/work/nowhere".into(),
        }),
        cx,
    )
    .try_recv()
    .unwrap();
    assert_eq!(missing.unwrap_err().code, ErrorCode::ProjectNotOpen);
    assert!(!cx.has_pending_prompt(), "nothing to delete, nothing asked");
}

#[gpui::test]
fn a_session_caller_runs_every_confirmed_call_without_a_prompt(cx: &mut TestAppContext) {
    // External control off and nobody approved: the session's provider has
    // its own permission system, so MonoCode asks nothing.
    let window = setup(cx);
    let caller = window
        .update(cx, |shell, _, _| {
            *shell.session_summaries.keys().max().unwrap()
        })
        .unwrap();
    let client = monocode_control::session_caller_name(caller);
    for (ix, case) in cases("s", window, cx).into_iter().enumerate() {
        let name = case.command.name();
        let request = format!("as-session-{ix}");
        let answer = send_as(window, &client, Some(&request), case.command.clone(), cx);
        assert!(!cx.has_pending_prompt(), "{name}: nobody is asked");
        let done = answer.try_recv().expect("the call settles by itself");
        assert!(done.is_ok(), "{name}: {done:?}");
        assert!(case.changed.as_ref()(window, cx), "{name}");
    }
    let log = activity(window, cx);
    assert!(log.iter().all(|(op, _)| op != "confirm"), "{log:?}");
}
