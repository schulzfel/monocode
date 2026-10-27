//! A Mono's old app actions, pinned where no other test covers them (MO-90)
//! and ported from the bridge they were first written against: each old
//! action is translated as the `app` command translates it
//! (`app_cli::legacy`) and called through the control pipeline as the
//! Mono's session. The assertions are the bridge's facts; `shape: C-8` (or
//! C-7) marks output JSON whose shape changed while the asserted facts
//! stayed, and where the answer differs from the bridge's a comment names
//! the accepted change or fix of the spec (§13, §14) or the rule that makes
//! it. The actions only a Mono had, on its own memory, soul, habits,
//! artifacts and cards, are pinned in `tests_own`.
use super::tests::{Setup, setup};
use super::*;
use crate::app_cli::legacy;
use gpui::TestAppContext;
use monocode_control::{Command, ControlError, ErrorCode, ops::organize::SessionArchiveInput};
use monocode_core::{mono::Launch, time::now_ms, transcript::Row};
use serde_json::{Value, json};

/// The session an action comes from: the Mono's main chat or a habit run.
#[derive(Clone, Copy)]
pub(crate) enum Caller {
    Chat,
    Run(i64),
}

/// The session `caller` calls from.
pub(super) fn session_of(s: &Setup, caller: Caller) -> i64 {
    match caller {
        Caller::Chat => s.session,
        Caller::Run(id) => id,
    }
}

/// `command` through the control pipeline, called as `caller`'s session's
/// relay calls it.
pub(crate) fn control_command(
    s: &Setup,
    caller: Caller,
    command: Command,
    request_id: Option<&str>,
    cx: &mut TestAppContext,
) -> Result<Value, ControlError> {
    let session = session_of(s, caller);
    let reply = s
        .window
        .update(cx, |shell, window, cx| {
            shell.control_call_as(session, request_id, command, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    reply
        .try_recv()
        .expect("the call settles once its writes are done")
}

/// Old action `action` through the control pipeline: translated as the
/// `app` command translates it, then called as `caller`'s session.
pub(super) fn control_call(
    s: &Setup,
    caller: Caller,
    action: &str,
    input: Value,
    request_id: &str,
    cx: &mut TestAppContext,
) -> Result<Value, ControlError> {
    let wire = legacy::translate(action, &input).map_err(ControlError::invalid_input)?;
    let command =
        serde_json::from_value(wire).map_err(|e| ControlError::invalid_input(e.to_string()))?;
    control_command(s, caller, command, Some(request_id), cx)
}

/// Old action `action` through the control pipeline; a refusal as its
/// message.
pub(super) fn act(
    s: &Setup,
    caller: Caller,
    action: &str,
    input: Value,
    request_id: &str,
    cx: &mut TestAppContext,
) -> Result<Value, String> {
    control_call(s, caller, action, input, request_id, cx).map_err(|e| e.message)
}

/// An unsent launch, so no provider starts.
pub(super) fn start(
    s: &Setup,
    caller: Caller,
    request: &str,
    input: Value,
    cx: &mut TestAppContext,
) -> i64 {
    let mut input = input;
    input["project"] = json!(s.project);
    input["draft"] = json!(true);
    if input.get("prompt").is_none() {
        input["prompt"] = json!("Review the project");
    }
    act(s, caller, "sessions.start", input, request, cx).unwrap()["sessionId"]
        .as_i64()
        .unwrap()
}

/// A habit run recorded as `start_habit` records one, without its first
/// message. It sits in the project so only the Mono filters can hide it.
pub(super) fn habit_run(s: &Setup, cx: &mut TestAppContext) -> i64 {
    let summary = locked(&s.store)
        .unwrap()
        .create("codex", &s.project)
        .unwrap();
    let session = summary.id;
    s.manager.update(cx, |m, cx| {
        let view = cx.new(|cx| Conversation::new(summary, m.store.clone(), cx));
        m.running.insert(
            session,
            RunningHabit {
                mono: s.id.clone(),
                habit: "habit".into(),
                view,
                started: now_ms(),
                launched: now_ms(),
                failure: None,
                waiting_since: None,
                cards: Vec::new(),
                mailbox: Vec::new(),
            },
        );
        m.roster.hidden_sessions.push(session);
        m.roster
            .get_mut(&s.id)
            .unwrap()
            .habit_sessions
            .push(session);
    });
    cx.run_until_parked();
    session
}

pub(crate) fn chat(s: &Setup, cx: &mut TestAppContext) -> Entity<Conversation> {
    s.manager.update(cx, |m, cx| m.ensure_chat(&s.id, cx));
    cx.run_until_parked();
    s.manager.read_with(cx, |m, _| m.chats[&s.id].clone())
}

pub(super) fn view(s: &Setup, session: i64, cx: &mut TestAppContext) -> Entity<Conversation> {
    let owner = s.manager.read_with(cx, |m, _| m.owner.upgrade().unwrap());
    owner
        .update(cx, |shell, cx| shell.mono_ensure_session(session, cx))
        .unwrap()
}

/// Session `session`'s chat, loaded and settled, as a chat the person
/// opened is, so its composer can hold their words (C-10).
fn loaded(s: &Setup, session: i64, cx: &mut TestAppContext) -> Entity<Conversation> {
    let view = view(s, session, cx);
    view.update(cx, |v, cx| v.load(cx));
    cx.run_until_parked();
    view
}

/// The sidebar session the Shell opened for the project.
pub(super) fn plain_session(s: &Setup, cx: &mut TestAppContext) -> i64 {
    s.window
        .update(cx, |shell, _, _| shell.workspace.active.unwrap())
        .unwrap()
}

/// A stored session of `cwd` that the window knows, as it knows every
/// session it loaded: outside the sidebar, as a Mono's tracked launch is.
pub(super) fn known_session(s: &Setup, cwd: &str, cx: &mut TestAppContext) -> i64 {
    let id = locked(&s.store).unwrap().create("codex", cwd).unwrap().id;
    s.window
        .update(cx, |shell, _, cx| shell.mono_track_sessions(vec![id], cx))
        .unwrap();
    cx.run_until_parked();
    id
}

pub(super) fn launches(s: &Setup, session: i64, cx: &mut TestAppContext) -> Vec<Launch> {
    s.manager.read_with(cx, |m, _| {
        m.roster
            .monos
            .iter()
            .flat_map(|m| m.launches.iter())
            .filter(|l| l.session_id == session)
            .cloned()
            .collect()
    })
}

fn groups(s: &Setup, cx: &mut TestAppContext) -> monocode_core::session_groups::Groups {
    let owner = s.manager.read_with(cx, |m, _| m.owner.upgrade().unwrap());
    owner.read_with(cx, |shell, _| shell.mono_groups(&s.project))
}

/// Cards held for the main chat: still pending, or posted as `mono-card` rows.
pub(super) fn held_cards(s: &Setup, cx: &mut TestAppContext) -> Vec<Value> {
    let pending = s.manager.read_with(cx, |m, _| {
        m.roster.get(&s.id).unwrap().pending_reports.clone()
    });
    let rows = locked(&s.store).unwrap().load(s.session).unwrap().rows;
    pending
        .iter()
        .filter_map(|r| r.strip_prefix("mono-card:").map(str::to_owned))
        .chain(
            rows.into_iter()
                .filter(|r| r.role == "mono-card")
                .map(|r| r.text),
        )
        .map(|text| serde_json::from_str(&text).unwrap())
        .collect()
}

pub(super) fn ids(list: &Value) -> Vec<Value> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].clone())
        .collect()
}

/// The project a session list is of. shape: C-8 (`cwd` became `project`).
fn listed_project(listed: &Value) -> Value {
    listed["project"].clone()
}

#[gpui::test]
fn project_scoped_actions_resolve_one_assigned_project_by_path_or_unique_name(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    let plain = plain_session(&s, cx);
    let outside = known_session(&s, "/not/assigned", cx);
    let name = std::path::Path::new(&s.project)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_uppercase();
    let mut n = 0;
    let mut call = |action: &str, input: Value, cx: &mut TestAppContext| {
        n += 1;
        act(&s, Caller::Chat, action, input, &format!("project-{n}"), cx)
    };
    let projects = |list: Vec<&str>, cx: &mut TestAppContext| {
        s.manager.update(cx, |m, _| {
            m.roster.get_mut(&s.id).unwrap().projects = list.into_iter().map(Into::into).collect()
        })
    };
    let scoped = |project: Option<&str>| {
        let with = |mut input: Value| {
            if let Some(project) = project {
                input["project"] = json!(project);
            }
            input
        };
        // session_stop and session_move take no project: the one given is
        // dropped and the session they act on decides (§9.2, §5 step 9), as
        // `on_session` checks.
        vec![
            ("sessions.list", with(json!({}))),
            (
                "sessions.start",
                with(json!({"prompt":"Check","draft":true})),
            ),
            ("folders.list", with(json!({}))),
            ("worktrees.list", with(json!({}))),
            ("worktrees.create", with(json!({"branch":"mono-check"}))),
        ]
    };
    let on_session = |session: i64| {
        [
            (
                "sessions.stop",
                json!({"sessionId":session,"project":"beta"}),
            ),
            (
                "folders.move",
                json!({"sessionId":session,"project":"beta"}),
            ),
        ]
    };
    let outside_text = Err("Session is outside the assigned project".to_string());

    assert_eq!(
        listed_project(&call("sessions.list", json!({}), cx).unwrap()),
        s.project
    );
    // A unique basename matches without regard to case.
    assert_eq!(
        listed_project(&call("sessions.list", json!({"project":name}), cx).unwrap()),
        s.project
    );
    for (action, input) in scoped(Some("/not/assigned")) {
        assert_eq!(
            call(action, input, cx),
            Err("Choose one of this Mono's assigned projects by full path".into()),
            "{action}"
        );
    }
    for (action, input) in on_session(outside) {
        assert_eq!(call(action, input, cx), outside_text, "{action}");
    }

    projects(
        vec![
            s.project.as_str(),
            "/work/beta",
            "/work/alpha",
            "/other/alpha",
        ],
        cx,
    );
    for (action, input) in scoped(None) {
        assert_eq!(
            call(action, input, cx),
            Err("This Mono has several projects; include project with its path".into()),
            "{action}"
        );
    }
    // The session's own project is assigned, so none is needed.
    for (action, input) in on_session(plain) {
        assert!(call(action, input, cx).is_ok(), "{action}");
    }
    assert_eq!(
        listed_project(&call("sessions.list", json!({"project":"beta"}), cx).unwrap()),
        "/work/beta"
    );
    assert_eq!(
        listed_project(&call("sessions.list", json!({"project":"/work/alpha"}), cx).unwrap()),
        "/work/alpha"
    );
    for (action, input) in scoped(Some("alpha")) {
        assert_eq!(
            call(action, input, cx),
            Err("Choose one of this Mono's assigned projects by full path".into()),
            "{action}"
        );
    }

    projects(Vec::new(), cx);
    for (action, input) in scoped(None) {
        assert_eq!(
            call(action, input, cx),
            Err("Add a project in this Mono's settings first".into()),
            "{action}"
        );
    }
    for (action, input) in scoped(Some("beta")) {
        assert_eq!(
            call(action, input, cx),
            Err("Choose one of this Mono's assigned projects by full path".into()),
            "{action}"
        );
    }
    for (action, input) in on_session(plain) {
        assert_eq!(call(action, input, cx), outside_text, "{action}");
    }
}

#[gpui::test]
fn mono_launches_inherit_the_main_chat_and_record_their_origin_turn(cx: &mut TestAppContext) {
    let s = setup(cx);
    let main = chat(&s, cx);
    // A read-only chat may only read (§5 step 8), so the inherited access
    // is shown with another mode than the bridge's read-only.
    main.update(cx, |v, _| v.settings.access = AccessMode::ReadOnly);
    let refused = control_call(
        &s,
        Caller::Chat,
        "sessions.start",
        json!({"project":s.project,"prompt":"Review","draft":true}),
        "read-only",
        cx,
    )
    .unwrap_err();
    assert_eq!(
        (refused.code, refused.message.as_str()),
        (
            ErrorCode::ScopeDenied,
            "This thread's access policy allows read-only app actions."
        )
    );
    let access = AccessMode::AutoAcceptEdits;
    main.update(cx, |v, _| {
        v.settings.model = Some("gpt-inherited".into());
        v.settings.effort = Some("high".into());
        v.settings.access = access;
        v.settings.worktree_cwd = Some("/elsewhere".into());
        v.mono_turn = 41;
    });
    let provider = main.read_with(cx, |v, _| v.summary.provider.clone());
    let input = json!({"project":s.project,"prompt":"Review the parser","title":"Parser review","draft":true});
    let started = act(
        &s,
        Caller::Chat,
        "sessions.start",
        input.clone(),
        "start",
        cx,
    )
    .unwrap();
    // shape: C-8
    let child = started["sessionId"].as_i64().unwrap();
    assert_eq!(started["project"], s.project);
    // fix: F-1, a retry with the same request ID finds the same session.
    let again = act(&s, Caller::Chat, "sessions.start", input, "start", cx).unwrap();
    assert_eq!(again["sessionId"], child);

    let [launch] = launches(&s, child, cx).try_into().unwrap();
    assert_eq!(launch.cwd, s.project);
    assert_eq!(launch.title, "Parser review");
    assert_eq!(launch.harness, provider);
    assert_eq!(launch.model.as_deref(), Some("gpt-inherited"));
    assert_eq!(launch.origin_turn, ((s.session as u64) << 32) | 41);
    // notifyOnComplete defaults to !draft.
    assert!(!launch.notify_on_complete);
    assert!(!launch.completed && !launch.dismissed);
    view(&s, child, cx).read_with(cx, |v, _| {
        assert_eq!(v.summary.title, "Parser review");
        assert_eq!(v.settings.model.as_deref(), Some("gpt-inherited"));
        assert_eq!(v.settings.effort.as_deref(), Some("high"));
        assert_eq!(v.settings.access, access);
        assert_eq!(v.settings.worktree_cwd, None);
        assert!(v.settings.mono.is_none());
        assert!(v.settings.environment.is_empty());
    });

    let long = "Investigate ".repeat(20);
    let untitled = start(&s, Caller::Chat, "untitled", json!({"prompt":long}), cx);
    assert_eq!(
        launches(&s, untitled, cx)[0].title,
        long.chars().take(160).collect::<String>()
    );

    // A habit run takes the main chat's settings, but its own turn.
    let run = habit_run(&s, cx);
    s.manager
        .read_with(cx, |m, _| m.running[&run].view.clone())
        .update(cx, |v, _| {
            v.settings.model = Some("habit-run-model".into());
            v.mono_turn = 7;
        });
    let delegated = start(&s, Caller::Run(run), "from-run", json!({}), cx);
    let [launch] = launches(&s, delegated, cx).try_into().unwrap();
    assert_eq!(launch.origin_turn, ((run as u64) << 32) | 7);
    assert_eq!(launch.model.as_deref(), Some("gpt-inherited"));

    cx.update(|cx| {
        let m = s.manager.read(cx);
        assert!(m.hidden_session(child) && m.hidden_session(delegated));
    });
    s.manager
        .update(cx, |m, cx| m.sidebar_preferences(&s.id, true, false, cx));
    cx.run_until_parked();
    cx.update(|cx| {
        let m = s.manager.read(cx);
        assert!(!m.hidden_session(child) && !m.hidden_session(delegated));
    });
}

#[gpui::test]
fn mono_launches_share_one_folder_in_the_mono_colour_when_both_preferences_are_on(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    s.manager.update(cx, |m, cx| {
        let mono = m.roster.get_mut(&s.id).unwrap();
        mono.name = "Scout".into();
        mono.color = mono::COLORS_MONO[2];
        m.sidebar_preferences(&s.id, true, true, cx);
    });
    cx.run_until_parked();
    let first = start(&s, Caller::Chat, "first", json!({}), cx);
    let folder = groups(&s, cx).folder_of(first).unwrap().clone();
    assert_eq!(folder.name, "Scout");
    assert_eq!(folder.color_index, Some(3));

    // The palette's last colour maps to the sixth folder colour.
    s.manager.update(cx, |m, _| {
        m.roster.get_mut(&s.id).unwrap().color = mono::COLORS_MONO[8]
    });
    let second = start(&s, Caller::Chat, "second", json!({}), cx);
    let shown = groups(&s, cx);
    let folders: Vec<_> = shown.folders.iter().filter(|f| f.name == "Scout").collect();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, folder.id);
    assert_eq!(folders[0].session_ids, vec![first, second]);
    assert_eq!(folders[0].color_index, Some(6));
    assert_eq!(locked(&s.store).unwrap().groups(&s.project).unwrap(), shown);

    s.manager
        .update(cx, |m, cx| m.sidebar_preferences(&s.id, true, false, cx));
    cx.run_until_parked();
    let loose = start(&s, Caller::Chat, "loose", json!({}), cx);
    assert!(groups(&s, cx).folder_of(loose).is_none());
}

#[gpui::test]
fn mono_session_lists_include_hidden_launches_and_archived_sessions_but_not_mono_sessions(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    let plain = plain_session(&s, cx);
    let launch = start(&s, Caller::Chat, "hidden", json!({}), cx);
    let archived = {
        let db = locked(&s.store).unwrap();
        let id = db.create("codex", &s.project).unwrap().id;
        db.set_archived(id, true).unwrap();
        id
    };
    // The window knows it, as it knows every stored session.
    s.window
        .update(cx, |shell, _, cx| {
            shell.mono_track_sessions(vec![archived], cx)
        })
        .unwrap();
    cx.run_until_parked();
    // An orchestration's worker is listed too; only changing it is refused.
    let worker = known_session(&s, &s.project, cx);
    s.window
        .update(cx, |shell, _, _| shell.control_make_worker(plain, worker))
        .unwrap();
    let run = habit_run(&s, cx);
    cx.update(|cx| assert!(s.manager.read(cx).hidden_session(launch)));
    let listed = act(&s, Caller::Chat, "sessions.list", json!({}), "list", cx).unwrap();
    // shape: C-8
    let sessions = listed["sessions"].as_array().unwrap();
    let entry = |id: i64| sessions.iter().find(|e| e["id"] == id);
    assert!(entry(plain).is_some());
    assert_eq!(entry(launch).unwrap()["archived"], false);
    assert_eq!(entry(archived).unwrap()["archived"], true);
    assert!(entry(worker).is_some());
    assert!(entry(run).is_none());
    assert!(entry(s.session).is_none());
}

#[gpui::test]
fn mono_send_refuses_busy_or_typed_targets_and_rearms_completion_reports(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx).update(cx, |v, _| v.mono_turn = 41);
    let target = plain_session(&s, cx);
    let target_view = loaded(&s, target, cx);
    let mut n = 0;
    let mut call = |action: &str, input: Value, cx: &mut TestAppContext| {
        n += 1;
        act(&s, Caller::Chat, action, input, &format!("send-{n}"), cx)
    };
    let refused = |result: Result<Value, String>, action: &str, typed: bool| {
        // C-10: Operator's readiness for a send and a draft, in its words,
        // not the bridge's "Session is busy or has an unsent draft". A draft
        // tells the person's unsent text from a busy session.
        let expected = match action {
            "sessions.send" => {
                "Session is busy, loading, or has an unsent draft. Its draft was preserved.".into()
            }
            _ if typed => format!("Session {target} already has an unsent draft; it was kept."),
            _ => format!(
                "Session {target} is working; change it once it is idle (see session_wait)."
            ),
        };
        assert_eq!(result, Err(expected), "{action}");
    };

    target_view.update(cx, |v, cx| {
        v.input
            .update(cx, |i, cx| i.set_text("Keep my words".into(), cx))
    });
    for action in ["sessions.send", "sessions.draft"] {
        let input = json!({"sessionId":target,"prompt":"Replace them"});
        refused(call(action, input, cx), action, true);
    }
    target_view.read_with(cx, |v, cx| {
        assert_eq!(v.input.read(cx).content, "Keep my words")
    });

    target_view.update(cx, |v, cx| {
        v.input.update(cx, |i, cx| i.set_text(String::new(), cx));
        v.busy = true;
    });
    for action in ["sessions.send", "sessions.draft"] {
        let input = json!({"sessionId":target,"prompt":"Replace them"});
        refused(call(action, input, cx), action, false);
    }
    target_view.update(cx, |v, _| v.busy = false);
    assert!(launches(&s, target, cx).is_empty());

    // A /draft prompt is saved by the target's composer, so no provider starts.
    let sent = call(
        "sessions.send",
        json!({"sessionId":target,"prompt":"/draft Check the failing test","notifyOnComplete":true}),
        cx,
    );
    // shape: C-8
    assert!(sent.is_ok(), "{sent:?}");
    let title = target_view.read_with(cx, |v, _| {
        assert!(!v.busy && v.has_saved_draft());
        v.summary.title.clone()
    });
    let [created] = launches(&s, target, cx).try_into().unwrap();
    assert_eq!(created.cwd, s.project);
    assert_eq!(created.title, title);
    assert!(created.notify_on_complete && !created.completed && !created.dismissed);
    assert_eq!(created.origin_turn, ((s.session as u64) << 32) | 41);

    // A reported launch is re-armed in place.
    let earlier = locked(&s.store)
        .unwrap()
        .create("codex", &s.project)
        .unwrap()
        .id;
    s.manager.update(cx, |m, cx| {
        m.roster.get_mut(&s.id).unwrap().launches.push(Launch {
            session_id: earlier,
            cwd: s.project.clone(),
            title: "Earlier work".into(),
            harness: "codex".into(),
            model: None,
            origin_turn: 3,
            notify_on_complete: false,
            completed: true,
            dismissed: true,
        });
        m.configure_chat(&s.id, cx);
    });
    cx.run_until_parked();
    loaded(&s, earlier, cx);
    let sent = call(
        "sessions.send",
        json!({"sessionId":earlier,"prompt":"/draft Look again","notifyOnComplete":true}),
        cx,
    );
    assert!(sent.is_ok(), "{sent:?}");
    let [rearmed] = launches(&s, earlier, cx).try_into().unwrap();
    assert_eq!(rearmed.title, "Earlier work");
    assert!(rearmed.notify_on_complete && !rearmed.completed && !rearmed.dismissed);
    assert_eq!(rearmed.origin_turn, ((s.session as u64) << 32) | 41);
    assert!(
        mono::Roster::load(&locked(&s.store).unwrap())
            .unwrap()
            .get(&s.id)
            .unwrap()
            .launches
            .iter()
            .any(|l| l.session_id == earlier && l.notify_on_complete)
    );
}

#[gpui::test]
fn mono_stop_archive_and_delete_dismiss_reports_and_remove_their_sessions(cx: &mut TestAppContext) {
    let s = setup(cx);
    let stopped = start(&s, Caller::Chat, "stop-child", json!({}), cx);
    let archived = start(&s, Caller::Chat, "archive-child", json!({}), cx);
    let deleted = start(&s, Caller::Chat, "delete-child", json!({}), cx);
    // Started before the delete, whose session ID the store may hand out again.
    let busy = start(&s, Caller::Chat, "busy-child", json!({}), cx);
    // Every Mono's launch of the session is dismissed, not only the caller's.
    s.manager.update(cx, |m, _| {
        let launch = m.roster.get(&s.id).unwrap().launches[0].clone();
        let other = m.roster.create(vec![s.project.clone()]).id.clone();
        m.roster.get_mut(&other).unwrap().launches.push(launch);
    });
    view(&s, stopped, cx).update(cx, |v, _| v.busy = true);
    for (n, (action, session)) in [
        ("sessions.stop", stopped),
        ("sessions.archive", archived),
        ("sessions.delete", deleted),
    ]
    .into_iter()
    .enumerate()
    {
        let result = act(
            &s,
            Caller::Chat,
            action,
            json!({"sessionId":session}),
            &format!("remove-{n}"),
            cx,
        );
        // shape: C-8
        assert!(result.is_ok(), "{action}: {result:?}");
        let records = launches(&s, session, cx);
        assert!(!records.is_empty());
        assert!(records.iter().all(|l| l.dismissed), "{action}");
    }
    assert_eq!(launches(&s, stopped, cx).len(), 2);
    assert!(!view(&s, stopped, cx).read_with(cx, |v, _| v.busy));
    let saved = locked(&s.store).unwrap().sessions().unwrap();
    assert!(saved.iter().find(|x| x.id == archived).unwrap().archived);
    assert!(!saved.iter().find(|x| x.id == stopped).unwrap().archived);
    assert!(!saved.iter().any(|x| x.id == deleted));
    // fix: F-2, a refused archive dismisses nothing.
    view(&s, busy, cx).update(cx, |v, _| v.busy = true);
    let archive = Command::SessionArchive(SessionArchiveInput {
        session_id: busy,
        stop: None,
    });
    let refused = control_command(&s, Caller::Chat, archive, Some("archive-busy"), cx);
    assert_eq!(refused.unwrap_err().code, ErrorCode::SessionBusy);
    assert!(launches(&s, busy, cx).iter().all(|l| !l.dismissed));
    // Any app caller's stop dismisses the Mono's report (§7.2): here a
    // thread's, once the Mono shows its launches in the sidebar.
    s.manager
        .update(cx, |m, cx| m.sidebar_preferences(&s.id, true, false, cx));
    cx.run_until_parked();
    let thread = plain_session(&s, cx);
    let stop = json!({"operation":"session_stop","input":{"sessionId":busy}});
    let reply = s
        .window
        .update(cx, |shell, window, cx| {
            let command = serde_json::from_value(stop).unwrap();
            shell.control_call_as(thread, None, command, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    assert!(reply.try_recv().unwrap().is_ok());
    assert!(launches(&s, busy, cx).iter().all(|l| l.dismissed));
}

#[gpui::test]
fn mono_folder_moves_prefer_a_new_folder_name_and_otherwise_ungroup(cx: &mut TestAppContext) {
    let s = setup(cx);
    let first = start(&s, Caller::Chat, "first", json!({}), cx);
    let second = start(&s, Caller::Chat, "second", json!({}), cx);
    let mut n = 0;
    let mut call = |input: Value, cx: &mut TestAppContext| {
        n += 1;
        let request = format!("move-{n}");
        control_call(&s, Caller::Chat, "folders.move", input, &request, cx)
            .map_err(|e| (e.code, e.message))
    };
    let name_of = |session: i64, cx: &mut TestAppContext| {
        groups(&s, cx).folder_of(session).map(|f| f.name.clone())
    };

    // shape: C-8
    assert!(call(json!({"sessionId":first,"newFolderName":"Review"}), cx).is_ok());
    let review = groups(&s, cx).folder_of(first).unwrap().id.clone();
    assert!(
        call(
            json!({"sessionId":second,"newFolderName":"Later","folderId":review}),
            cx
        )
        .is_ok()
    );
    assert_eq!(name_of(second, cx).as_deref(), Some("Later"));
    assert_eq!(name_of(first, cx).as_deref(), Some("Review"));
    assert!(call(json!({"sessionId":second,"folderId":review}), cx).is_ok());
    assert_eq!(name_of(second, cx).as_deref(), Some("Review"));
    let missing = call(json!({"sessionId":second,"folderId":"missing"}), cx).unwrap_err();
    let outside = call(json!({"sessionId":s.session}), cx).unwrap_err();
    // C-1: the operation's words, not the bridge's "Folder not found" and
    // "Session is outside this project"; a Mono's own chat is no session it
    // can reach (§7.4).
    assert_eq!(
        missing,
        (
            ErrorCode::NotFound,
            "There is no such folder in the session's project; see folder_list.".into()
        )
    );
    assert_eq!(
        outside,
        (
            ErrorCode::NotFound,
            format!("There is no session {}; see session_list.", s.session)
        )
    );
    assert!(call(json!({"sessionId":first}), cx).is_ok());
    assert_eq!(name_of(first, cx), None);

    let shown = groups(&s, cx);
    assert_eq!(locked(&s.store).unwrap().groups(&s.project).unwrap(), shown);
    let listed = act(&s, Caller::Chat, "folders.list", json!({}), "list", cx).unwrap();
    // shape: C-8 (a bare array of snake_case folders became `folders`)
    let (folders, sessions) = (listed["folders"].as_array().unwrap().clone(), "sessionIds");
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0]["id"], review);
    assert_eq!(folders[0]["name"], "Review");
    assert_eq!(folders[0][sessions], json!([second]));
}

#[gpui::test]
fn mono_completion_reports_wait_for_the_whole_group_and_arrive_once(cx: &mut TestAppContext) {
    let s = setup(cx);
    let main = chat(&s, cx);
    // A usage limit holds the provider boundary: the report stays queued. The
    // Mono is closed first because its usage notice sets a hover style twice,
    // which gpui's debug assertion rejects when the chat is drawn.
    s.manager.update(cx, |m, cx| m.close(cx));
    main.update(cx, |v, _| {
        v.settings.usage_limit = Some(monocode_core::usage::Limit::default())
    });
    let group = ((s.session as u64) << 32) | 9;
    let mut children = Vec::new();
    for (title, reply) in [
        ("Fix the tests", "All tests pass now."),
        ("Audit the specs", "Two specs still fail."),
    ] {
        let id = {
            let mut db = locked(&s.store).unwrap();
            let id = db.create("codex", &s.project).unwrap().id;
            db.write_rows(
                id,
                0,
                &[
                    Row {
                        role: "user".into(),
                        text: title.into(),
                        first: true,
                        sent_at: None,
                    },
                    Row {
                        role: "assistant".into(),
                        text: reply.into(),
                        first: true,
                        sent_at: None,
                    },
                ],
            )
            .unwrap();
            id
        };
        children.push(id);
        s.manager.update(cx, |m, _| {
            m.roster.get_mut(&s.id).unwrap().launches.push(Launch {
                session_id: id,
                cwd: s.project.clone(),
                title: title.into(),
                harness: "codex".into(),
                model: None,
                origin_turn: group,
                notify_on_complete: true,
                completed: false,
                dismissed: false,
            })
        });
    }
    s.manager.update(cx, |m, cx| m.configure_chat(&s.id, cx));
    cx.run_until_parked();
    let reports = |cx: &mut TestAppContext| {
        s.manager.read_with(cx, |m, _| {
            m.roster
                .get(&s.id)
                .unwrap()
                .outbox
                .iter()
                .filter(|q| q.text.starts_with("[Delegated work completed]"))
                .map(|q| (q.text.clone(), q.internal))
                .collect::<Vec<_>>()
        })
    };
    let check = |cx: &mut TestAppContext| {
        s.manager.update(cx, |m, cx| m.check_completions(cx));
        cx.run_until_parked();
    };

    let busy_child = view(&s, children[1], cx);
    busy_child.update(cx, |v, _| v.busy = true);
    check(cx);
    assert!(reports(cx).is_empty(), "a group waits for every child");
    busy_child.update(cx, |v, _| v.busy = false);
    main.update(cx, |v, _| v.busy = true);
    check(cx);
    assert!(reports(cx).is_empty(), "a group waits for the Mono's turn");
    main.update(cx, |v, _| v.busy = false);
    check(cx);

    let expected = format!(
        "[Delegated work completed]\nAll sessions in this group have stopped. Inspect their results and give an integrated update.\n\nSession {}: Fix the tests\nAll tests pass now.\n\nSession {}: Audit the specs\nTwo specs still fail.",
        children[0], children[1]
    );
    assert_eq!(reports(cx), vec![(expected.clone(), true)]);
    assert!(
        !main.read_with(cx, |v, _| v.busy),
        "the report is queued, not run"
    );
    for child in &children {
        assert!(launches(&s, *child, cx)[0].completed);
    }
    check(cx);
    assert_eq!(reports(cx).len(), 1, "a group is reported once");
    let saved = mono::Roster::load(&locked(&s.store).unwrap()).unwrap();
    let mono = saved.get(&s.id).unwrap();
    assert_eq!(
        mono.outbox
            .iter()
            .filter(|q| q.text == expected && q.internal)
            .count(),
        1
    );
    assert!(mono.launches.iter().all(|l| l.completed));
}

/// A stored session of the Mono's project with an earlier turn, known to
/// the window by its summary only: its chat is not loaded.
fn unloaded_session(s: &Setup, cx: &mut TestAppContext) -> i64 {
    let id = {
        let mut db = locked(&s.store).unwrap();
        let id = db.create("codex", &s.project).unwrap().id;
        let row = |role: &str, text: &str| Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        };
        db.write_rows(
            id,
            0,
            &[row("user", "Earlier"), row("assistant", "Done before")],
        )
        .unwrap();
        id
    };
    s.window
        .update(cx, |shell, _, cx| shell.mono_track_sessions(vec![id], cx))
        .unwrap();
    cx.run_until_parked();
    let owner = s.manager.read_with(cx, |m, _| m.owner.upgrade().unwrap());
    owner.read_with(cx, |shell, _| {
        assert!(shell.mono_session_entity(id).is_none(), "not loaded");
    });
    id
}

/// R1: a Mono's send or draft to a chat that is not loaded yet, but idle,
/// loads it and goes in once it has, as the bridge held it for `load()`;
/// a saved draft does not refuse a Mono's send, as it never did.
#[gpui::test]
fn mono_sends_and_drafts_wait_for_an_unloaded_chat(cx: &mut TestAppContext) {
    let s = setup(cx);
    chat(&s, cx);
    let target = unloaded_session(&s, cx);
    let input = json!({"sessionId": target, "prompt": "/draft Check the build"});
    let sent = act(&s, Caller::Chat, "sessions.send", input, "unloaded-1", cx);
    assert!(sent.is_ok(), "{sent:?}");
    let target_view = view(&s, target, cx);
    target_view.read_with(cx, |v, _| {
        assert!(v.history_loaded() && !v.busy, "loaded, and the prompt ran");
        assert!(v.has_saved_draft(), "the /draft prompt went in");
    });
    let input = json!({"sessionId": target, "prompt": "/draft Check it again"});
    let again = act(&s, Caller::Chat, "sessions.send", input, "unloaded-2", cx);
    assert!(
        again.is_ok(),
        "a saved draft does not refuse a Mono: {again:?}"
    );

    let target = unloaded_session(&s, cx);
    let input = json!({"sessionId": target, "prompt": "Review the diff"});
    let drafted = act(&s, Caller::Chat, "sessions.draft", input, "unloaded-3", cx);
    // shape: C-8
    assert_eq!(drafted.unwrap()["saved"], true);
    view(&s, target, cx).read_with(cx, |v, _| assert!(v.has_saved_draft()));
}

/// R14: while a Mono's send waits for its target to load, the target counts
/// as busy, so the Mono hears back once that send has run, not from the
/// target's previous turn.
#[gpui::test]
fn a_waiting_send_is_not_reported_before_it_runs(cx: &mut TestAppContext) {
    let s = setup(cx);
    let main = chat(&s, cx);
    // A usage limit keeps the report queued in the Mono's chat, closed so
    // its usage notice is never drawn (see the completion test above).
    s.manager.update(cx, |m, cx| m.close(cx));
    main.update(cx, |v, _| {
        v.mono_turn = 7;
        v.settings.usage_limit = Some(monocode_core::usage::Limit::default());
    });
    let target = unloaded_session(&s, cx);
    let send = Command::SessionSend(monocode_control::ops::dispatch::SessionSendInput {
        session_id: target,
        prompt: "/draft Look again".into(),
        notify_on_complete: Some(true),
        ..Default::default()
    });
    let reply = s
        .window
        .update(cx, |shell, window, cx| {
            shell.control_call_as(s.session, None, send, window, cx)
        })
        .unwrap();
    assert!(reply.try_recv().unwrap().is_ok(), "taken before it loads");
    s.manager.update(cx, |m, cx| {
        m.check_completions(cx);
        assert!(
            m.completion_work.is_empty(),
            "the target's earlier turn is not this send's"
        );
    });
    cx.run_until_parked();
    view(&s, target, cx).read_with(cx, |v, _| assert!(v.has_saved_draft(), "it ran"));
    s.manager.update(cx, |m, cx| m.check_completions(cx));
    cx.run_until_parked();
    let [launch] = launches(&s, target, cx).try_into().unwrap();
    assert!(launch.completed, "reported once it ran");
    let reported = s.manager.read_with(cx, |m, _| {
        m.roster.get(&s.id).unwrap().outbox.iter().any(|q| {
            q.text.starts_with("[Delegated work completed]")
                && q.text.contains(&format!("Session {target}:"))
        })
    });
    assert!(reported);
}

/// R10: an agent session reads a launch a Mono keeps out of the sidebar, as
/// Operator's reads reached every session of its project (OP-79); changing
/// it stays refused, and a Mono's own chat stays out of reach.
#[gpui::test]
fn a_thread_reads_a_monos_hidden_launch_but_does_not_change_it(cx: &mut TestAppContext) {
    let s = setup(cx);
    let thread = plain_session(&s, cx);
    let launch = start(&s, Caller::Chat, "hidden", json!({}), cx);
    cx.update(|cx| assert!(s.manager.read(cx).hidden_session(launch)));
    let by_thread = |command: Value, cx: &mut TestAppContext| {
        let command = serde_json::from_value(command).unwrap();
        let reply = s
            .window
            .update(cx, |shell, window, cx| {
                shell.control_call_as(thread, None, command, window, cx)
            })
            .unwrap();
        cx.run_until_parked();
        reply.try_recv().unwrap()
    };
    for (op, input) in [
        ("session_read", json!({"sessionId": launch})),
        ("session_status", json!({"sessionId": launch})),
        ("session_wait", json!({"sessionId": launch, "timeoutMs": 0})),
        ("queue_list", json!({"sessionId": launch})),
    ] {
        let read = by_thread(json!({"operation": op, "input": input}), cx);
        assert!(read.is_ok(), "{op}: {read:?}");
    }
    let send = json!({"operation": "session_send", "input": {"sessionId": launch, "prompt": "Hi"}});
    assert_eq!(by_thread(send, cx).unwrap_err().code, ErrorCode::NotFound);
    let own = json!({"operation": "session_read", "input": {"sessionId": s.session}});
    assert_eq!(by_thread(own, cx).unwrap_err().code, ErrorCode::NotFound);
}
