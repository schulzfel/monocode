//! A Mono on the control pipeline: its main chat and habit runs call as the
//! Mono for as long as it owns them (T-M1), keep to a Mono's operations and
//! projects (T-D2, T-M3), and their writes keep receipts (C-5). Only they
//! may call a Mono's own operations, and a habit run may not change habits
//! (T-M2). Calls go in as the Mono's session's relay sends them
//! (`Shell::control_call_as`).
use super::tests::{Setup, setup};
use super::tests_parity_mono::{
    Caller, act, chat, control_call, control_command, habit_run, known_session, plain_session, view,
};
use super::*;
use gpui::TestAppContext;
use monocode_control::{Command, ControlError, ErrorCode, Grant, Scope, Topic};
use serde_json::{Value, json};

const GONE: &str = "This Mono no longer owns an active app capability";

fn hello(s: &Setup, session: i64, cx: &mut TestAppContext) -> Result<Grant, ControlError> {
    let reply = s
        .window
        .update(cx, |shell, window, cx| {
            shell.control_hello_as(session, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    reply
        .try_recv()
        .expect("a session's hello is answered at once")
}

fn command(operation: &str, input: Value) -> Command {
    serde_json::from_value(json!({"operation": operation, "input": input})).unwrap()
}

/// The activity log's newest entry: who, what, and how it ended.
fn last_activity(s: &Setup, cx: &mut TestAppContext) -> (String, String, String) {
    s.window
        .update(cx, |shell, _, _| shell.control_activity().pop().unwrap())
        .unwrap()
}

fn mono_label(s: &Setup, cx: &mut TestAppContext) -> String {
    s.manager
        .read_with(cx, |m, _| m.roster.get(&s.id).unwrap().label())
}

#[gpui::test]
fn a_monos_chat_and_habit_run_call_as_the_mono_until_it_no_longer_owns_them(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    chat(&s, cx);
    let run = habit_run(&s, cx);
    s.manager.update(cx, |m, cx| {
        m.running[&run]
            .view
            .update(cx, |v, _| v.summary.title = "Scout · Digest".into());
    });
    for session in [s.session, run] {
        let grant = hello(&s, session, cx).unwrap();
        assert_eq!(grant.scopes, Scope::ALL.to_vec(), "every scope");
    }
    let label = mono_label(&s, cx);
    for (caller, named) in [
        (Caller::Chat, label.as_str()),
        (Caller::Run(run), "Scout · Digest"),
    ] {
        let refused = control_command(&s, caller, command("settings_update", json!({})), None, cx)
            .unwrap_err();
        assert_eq!(
            (refused.code, refused.message.as_str()),
            (ErrorCode::ScopeDenied, "Monos cannot use settings_update")
        );
        assert_eq!(
            last_activity(&s, cx),
            (
                named.to_owned(),
                "settings_update".into(),
                "scope_denied".into()
            )
        );
    }

    // A reset chat and an ended run are no longer the Mono's.
    s.manager.update(cx, |m, _| {
        m.roster.get_mut(&s.id).unwrap().session_id = Some(i64::from(u32::MAX));
        m.running.remove(&run);
    });
    for (caller, session) in [(Caller::Chat, s.session), (Caller::Run(run), run)] {
        let refused = hello(&s, session, cx).unwrap_err();
        assert_eq!(
            (refused.code, refused.message.as_str()),
            (ErrorCode::NotFound, GONE)
        );
        let list = command("session_list", json!({}));
        let refused = control_command(&s, caller, list, None, cx).unwrap_err();
        assert_eq!(
            (refused.code, refused.message.as_str()),
            (ErrorCode::NotFound, GONE)
        );
    }
}

#[gpui::test]
fn a_mono_keeps_to_its_operations_and_a_session_is_not_refused_them(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let run = habit_run(&s, cx);
    let thread = plain_session(&s, cx);
    // Whether a session calls it here too: a message to the Mono or a new
    // tab would start or show things this test does not need.
    let ops = [
        ("settings_update", json!({}), true),
        ("project_add", json!({"path": s.project}), true),
        ("mono_send", json!({"monoId": s.id, "text": "Hi"}), false),
        ("tab_open", json!({"sessionId": thread}), false),
    ];
    for (operation, input, thread_too) in ops {
        for caller in [Caller::Chat, Caller::Run(run)] {
            let refused = control_command(&s, caller, command(operation, input.clone()), None, cx)
                .unwrap_err();
            assert_eq!(
                (refused.code, refused.message),
                (
                    ErrorCode::ScopeDenied,
                    format!("Monos cannot use {operation}")
                )
            );
        }
        if !thread_too {
            continue;
        }
        let reply = s
            .window
            .update(cx, |shell, window, cx| {
                shell.control_call_as(thread, None, command(operation, input), window, cx)
            })
            .unwrap();
        cx.run_until_parked();
        if let Err(error) = reply.try_recv().unwrap() {
            assert!(!error.message.starts_with("Monos cannot use"), "{error:?}");
        }
    }
}

#[gpui::test]
fn a_monos_calls_default_to_its_one_project_and_say_when_they_cannot(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let call = |operation: &str, input: Value, cx: &mut TestAppContext| {
        control_command(&s, Caller::Chat, command(operation, input), None, cx)
    };
    assert_eq!(
        call("session_list", json!({}), cx).unwrap()["project"],
        s.project
    );
    assert_eq!(
        call("folder_list", json!({}), cx).unwrap()["project"],
        s.project
    );
    let started = call("session_start", json!({"prompt":"Look","draft":true}), cx).unwrap();
    assert_eq!(started["project"], s.project);
    let note = call("notes_write", json!({"title":"Plan","body":"Ship"}), cx).unwrap();
    assert_eq!(note["project"], s.project);

    // A Mono sees its projects' notes and those of no project.
    {
        let db = locked(&s.store).unwrap();
        for (title, projects) in [("Loose", vec![]), ("Elsewhere", vec!["/elsewhere".into()])] {
            let write = monocode_core::app_access::NoteWrite {
                id: None,
                title: Some(title.into()),
                body: Some("Body".into()),
                tags: None,
            };
            monocode_core::app_access::write_note(&db, None, title, write, &projects).unwrap();
        }
    }
    let listed = call("notes_list", json!({}), cx).unwrap();
    let mut titles: Vec<_> = listed["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["title"].as_str().unwrap().to_owned())
        .collect();
    titles.sort();
    assert_eq!(titles, ["Loose", "Plan"]);

    // `Mono::project`'s three refusals, in its words.
    s.manager.update(cx, |m, _| {
        m.roster
            .get_mut(&s.id)
            .unwrap()
            .projects
            .push("/work/beta".into());
    });
    let refused = call("session_list", json!({}), cx).unwrap_err();
    assert_eq!(
        (refused.code, refused.message.as_str()),
        (
            ErrorCode::InvalidInput,
            "This Mono has several projects; include project with its path"
        )
    );
    let refused = call(
        "session_start",
        json!({"prompt":"Look","project":"/nope"}),
        cx,
    )
    .unwrap_err();
    assert_eq!(
        (refused.code, refused.message.as_str()),
        (
            ErrorCode::ScopeDenied,
            "Choose one of this Mono's assigned projects by full path"
        )
    );
    s.manager
        .update(cx, |m, _| m.roster.get_mut(&s.id).unwrap().projects.clear());
    let refused = call("worktree_list", json!({}), cx).unwrap_err();
    assert_eq!(
        (refused.code, refused.message.as_str()),
        (
            ErrorCode::InvalidInput,
            "Add a project in this Mono's settings first"
        )
    );
}

#[gpui::test]
fn a_mono_follows_only_its_projects_sessions(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let plain = plain_session(&s, cx);
    let outside = known_session(&s, "/not/assigned", cx);
    let follow = |topic: Topic, cx: &mut TestAppContext| {
        s.window
            .update(cx, |shell, _, cx| {
                shell.control_follow_as(s.session, vec![topic], cx)
            })
            .unwrap()
    };
    let project = Topic::ProjectSessions {
        project: s.project.clone(),
    };
    assert!(follow(project, cx).is_ok());
    assert!(follow(Topic::Session { id: plain }, cx).is_ok());
    let refused = follow(Topic::SessionTranscript { id: outside }, cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ScopeDenied, "{}", refused.message);
}

#[gpui::test]
fn a_monos_retry_returns_its_first_result_and_other_input_conflicts(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let write = |body: &str, cx: &mut TestAppContext| {
        let input = json!({"title":"Receipt","body":body});
        control_call(&s, Caller::Chat, "artifacts.write", input, "write", cx)
    };
    let first = write("Written once", cx).unwrap();
    assert_eq!(write("Written once", cx).unwrap(), first);
    let documents = s
        .manager
        .read_with(cx, |m, _| m.roster.get(&s.id).unwrap().documents.len());
    assert_eq!(documents, 1);
    // C-5: the control pipeline's receipts, and its words for a conflict.
    let refused = write("Different", cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::RequestConflict);
}

/// The bridge's `notes.write` put a new note in the Mono's first assigned
/// project however many it has (`write_note` with its projects), where
/// every other call without a project must name one.
#[gpui::test]
fn a_monos_new_note_belongs_to_its_first_project(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    s.manager.update(cx, |m, _| {
        m.roster
            .get_mut(&s.id)
            .unwrap()
            .projects
            .push("/work/beta".into());
    });
    let call = |operation: &str, input: Value, cx: &mut TestAppContext| {
        control_command(&s, Caller::Chat, command(operation, input), None, cx)
    };
    let note = call("notes_write", json!({"title":"Plan","body":"Ship"}), cx).unwrap();
    assert_eq!(note["project"], s.project, "the first of two projects");
    let legacy = control_call(
        &s,
        Caller::Chat,
        "notes.write",
        json!({"title":"Legacy","body":"Ship"}),
        "legacy-note",
        cx,
    )
    .unwrap();
    assert_eq!(legacy["project"], s.project);
    let beta = call(
        "notes_write",
        json!({"title":"Beta","body":"Ship","project":"/work/beta"}),
        cx,
    )
    .unwrap();
    assert_eq!(beta["project"], "/work/beta", "a named project is kept");
    let refused = call("session_list", json!({}), cx).unwrap_err();
    assert_eq!(
        refused.message,
        "This Mono has several projects; include project with its path"
    );
}

/// The bridge's `notes.write` from a Mono without projects wrote a note of
/// no project (`write_note` with none); so do `notes_write` and the old
/// action on the control pipeline. Other calls still need a project.
#[gpui::test]
fn a_mono_without_projects_writes_notes_of_no_project(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    s.manager
        .update(cx, |m, _| m.roster.get_mut(&s.id).unwrap().projects.clear());
    let call = |operation: &str, input: Value, cx: &mut TestAppContext| {
        control_command(&s, Caller::Chat, command(operation, input), None, cx)
    };
    let note = call("notes_write", json!({"title":"Plan","body":"Ship"}), cx).unwrap();
    assert_eq!(note["project"], Value::Null);
    let input = json!({"title":"Legacy","body":"Ship"});
    let legacy = control_call(&s, Caller::Chat, "notes.write", input, "legacy", cx).unwrap();
    assert_eq!(legacy["project"], Value::Null);
    let notes = locked(&s.store).unwrap().list_notes().unwrap();
    for title in ["Plan", "Legacy"] {
        let note = notes.iter().find(|n| n.title == title).unwrap();
        assert_eq!(note.source_cwd, None, "{title}");
        assert!(note.source_session_id.is_some(), "{title}: from the chat");
    }
    let refused = call("worktree_list", json!({}), cx).unwrap_err();
    assert_eq!(
        refused.message,
        "Add a project in this Mono's settings first"
    );
}

/// Only a Mono's chat and habit runs may call a Mono's own operations: a
/// project session is refused them whatever its relay lists (the role in
/// its environment never reaches the app), and a habit run may list its
/// Mono's habits but not add, change, run or remove them (MO-8).
#[gpui::test]
fn only_a_mono_calls_its_own_operations_and_a_habit_run_only_lists_habits(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let run = habit_run(&s, cx);
    let thread = plain_session(&s, cx);
    let soul = command("mono_soul_read", json!({}));
    let reply = s
        .window
        .update(cx, |shell, window, cx| {
            shell.control_call_as(thread, None, soul.clone(), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    let refused = reply.try_recv().unwrap().unwrap_err();
    assert_eq!(
        (refused.code, refused.message.as_str()),
        (
            ErrorCode::ScopeDenied,
            "mono_soul_read is only available in a Mono's chat"
        )
    );
    assert!(control_command(&s, Caller::Chat, soul, None, cx).is_ok());

    let daily = json!({"kind":"daily"});
    for (operation, input) in [
        (
            "mono_habit_add",
            json!({"name":"Nested","instructions":"Go","schedule":daily}),
        ),
        ("mono_habit_update", json!({"id":"h1","enabled":false})),
        ("mono_habit_run", json!({"id":"h1"})),
        ("mono_habit_remove", json!({"id":"h1"})),
    ] {
        let refused =
            control_command(&s, Caller::Run(run), command(operation, input), None, cx).unwrap_err();
        assert_eq!(
            (refused.code, refused.message.as_str()),
            (
                ErrorCode::ScopeDenied,
                "A habit run cannot schedule more runs"
            ),
            "{operation}"
        );
    }
    let listed = control_command(
        &s,
        Caller::Run(run),
        command("mono_habit_list", json!({})),
        None,
        cx,
    )
    .unwrap();
    assert_eq!(listed["habits"], json!([]));
}

/// A draft launch's chat card shows the state the bridge's launch showed,
/// "done": the launch is installed with `Shell::mono_install_app`, and
/// `cards::launched_state` reads the live chat. A sent launch is left out:
/// it would start the provider.
#[gpui::test]
fn a_draft_launch_shows_the_bridges_state_on_its_card(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let input = json!({"project":s.project,"prompt":"Review the project","draft":true});
    let started = act(&s, Caller::Chat, "sessions.start", input, "draft", cx).unwrap();
    let id = started["sessionId"].as_i64().unwrap();
    let view = view(&s, id, cx);
    assert_eq!(view.read_with(cx, cards::launched_state), "done");
}
