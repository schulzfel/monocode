//! An agent session calls as Operator's threads did: its calls default to
//! its own project, its launches inherit its settings, and `model_list`
//! lists its own provider with its account.
use super::tests::{CLIENT, approve, call, repository, sessions, setup};
use super::tests_gate::with_view;
use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    AccessMode, Command, Scope,
    ops::{
        dispatch::{SessionStartInput, SessionStartOutput},
        model::{ModelListInput, ModelListOutput},
        notes::{NoteOutput, NotesListInput, NotesListOutput, NotesWriteInput},
        organize::{FolderListOutput, ProjectInput},
        session::{SessionListInput, SessionListOutput},
        worktree::{WorktreeCreateInput, WorktreeCreateOutput, WorktreeListOutput},
    },
    session_caller_name,
};
use monocode_core::accounts::Profile;
use serde_json::json;
use std::sync::Mutex;

const HERE: &str = "/work/monocode";
const OTHER: &str = "/work/another-project";

/// Calls `command` as session `id` and reads its output as `T`.
fn by<T: serde::de::DeserializeOwned>(
    window: WindowHandle<Shell>,
    id: i64,
    request: Option<&str>,
    command: Command,
    cx: &mut TestAppContext,
) -> Result<T, ControlError> {
    call(window, &session_caller_name(id), request, command, cx)
        .map(|v| serde_json::from_value(v).unwrap())
}

fn draft(prompt: &str) -> SessionStartInput {
    SessionStartInput {
        prompt: prompt.into(),
        draft: Some(true),
        ..SessionStartInput::default()
    }
}

fn start(input: SessionStartInput) -> Command {
    Command::SessionStart(input)
}

#[gpui::test]
fn a_sessions_calls_default_to_its_own_project(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &Scope::ALL)], cx);
    let (a, _) = sessions(window, cx);
    let repo = repository("td1");
    // Session a works in the repository; the window shows another project.
    window
        .update(cx, |shell, _, cx| {
            shell.workspace.add_project(&repo);
            shell.session_summaries.get_mut(&a).unwrap().cwd = repo.clone();
            shell.sessions[&a].update(cx, |v, _| v.summary.cwd = repo.clone());
            assert_eq!(shell.workspace.project, HERE);
        })
        .unwrap();
    let list: SessionListOutput = by(
        window,
        a,
        None,
        Command::SessionList(SessionListInput::default()),
        cx,
    )
    .unwrap();
    assert_eq!(list.project, repo);
    assert!(list.sessions.iter().any(|s| s.id == a));
    let started: SessionStartOutput = by(window, a, None, start(draft("Look around")), cx).unwrap();
    assert_eq!(started.project, repo);
    let folders: FolderListOutput = by(
        window,
        a,
        None,
        Command::FolderList(ProjectInput::default()),
        cx,
    )
    .unwrap();
    assert_eq!(folders.project, repo);
    let trees: WorktreeListOutput = by(
        window,
        a,
        None,
        Command::WorktreeList(ProjectInput::default()),
        cx,
    )
    .unwrap();
    assert_eq!(trees.project, repo);
    let created: WorktreeCreateOutput = by(
        window,
        a,
        None,
        Command::WorktreeCreate(WorktreeCreateInput {
            branch: "from-a".into(),
            ..WorktreeCreateInput::default()
        }),
        cx,
    )
    .unwrap();
    assert!(
        created
            .worktree
            .path
            .starts_with(&format!("{repo}-worktrees"))
    );
    let write = |project: Option<&str>| {
        Command::NotesWrite(NotesWriteInput {
            project: project.map(str::to_owned),
            body: Some("# Plan".into()),
            ..NotesWriteInput::default()
        })
    };
    let note: NoteOutput = by(window, a, Some("note-1"), write(None), cx).unwrap();
    assert_eq!(note.project.as_deref(), Some(repo.as_str()));
    // The receipt records the call as it ran: naming the default is the same call.
    let again: NoteOutput = by(window, a, Some("note-1"), write(Some(&repo)), cx).unwrap();
    assert_eq!(again, note);
    // Naming another project works, and a client keeps the window's project.
    let here: SessionListOutput = by(
        window,
        a,
        None,
        Command::SessionList(SessionListInput {
            project: Some(HERE.into()),
            ..SessionListInput::default()
        }),
        cx,
    )
    .unwrap();
    assert_eq!(here.project, HERE);
    let theirs = call(
        window,
        CLIENT,
        None,
        Command::SessionList(SessionListInput::default()),
        cx,
    )
    .unwrap();
    assert_eq!(theirs["project"], HERE);
}

#[gpui::test]
fn a_session_sees_its_own_projects_notes_and_those_of_none(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &Scope::ALL)], cx);
    let (a, _) = sessions(window, cx);
    for (project, body) in [(HERE, "Here"), (OTHER, "Other")] {
        let write = Command::NotesWrite(NotesWriteInput {
            project: Some(project.into()),
            body: Some(body.into()),
            ..NotesWriteInput::default()
        });
        call(window, CLIENT, None, write, cx).unwrap();
    }
    window
        .update(cx, |shell, _, _| {
            locked(&shell.store)
                .unwrap()
                .upsert_note(&monocode_core::notes::NoteUpsert {
                    id: "loose".into(),
                    title: "Loose".into(),
                    body: "Anywhere".into(),
                    tags: Vec::new(),
                    source_session_id: None,
                    source_cwd: None,
                    finalize_slug: true,
                })
                .unwrap();
        })
        .unwrap();
    let list = |caller: &str, cx: &mut TestAppContext| {
        let listed = call(
            window,
            caller,
            None,
            Command::NotesList(NotesListInput::default()),
            cx,
        )
        .unwrap();
        let listed: NotesListOutput = serde_json::from_value(listed).unwrap();
        let mut titles: Vec<_> = listed.notes.into_iter().map(|n| n.title).collect();
        titles.sort();
        titles
    };
    assert_eq!(list(&session_caller_name(a), cx), ["Loose", "Other"]);
    assert_eq!(list(CLIENT, cx), ["Here", "Loose", "Other"]);
    let other_id = window
        .update(cx, |shell, _, _| {
            locked(&shell.store)
                .unwrap()
                .list_notes()
                .unwrap()
                .into_iter()
                .find(|n| n.title == "Here")
                .unwrap()
                .id
        })
        .unwrap();
    let read = json!({"operation": "notes_read", "input": {"id": other_id}});
    let refused = call(
        window,
        &session_caller_name(a),
        None,
        serde_json::from_value(read).unwrap(),
        cx,
    );
    assert_eq!(refused.unwrap_err().code, ErrorCode::NotFound);
}

/// The accounts the last model discovery asked each provider with.
static ASKED: Mutex<Vec<(Kind, Profile)>> = Mutex::new(Vec::new());

/// 150 models per provider, each with an effort setting, and two access
/// modes; remembers the accounts it was asked with.
fn many_models(profiles: Vec<(Kind, Profile)>) -> Result<Value, String> {
    ASKED.lock().unwrap().clone_from(&profiles);
    let model = |kind: Kind, n: usize| {
        json!({
            "id": format!("{}-{n}", kind.id()),
            "name": format!("Model {n}"),
            "settings": [{
                "id": "effort",
                "label": "Effort",
                "defaultValue": "medium",
                "options": [
                    {"value": "low", "label": "Low"},
                    {"value": "medium", "label": "Medium"},
                ],
            }],
        })
    };
    let harnesses: Vec<_> = profiles
        .iter()
        .map(|(kind, _)| {
            json!({
                "id": kind.id(),
                "available": true,
                "error": Value::Null,
                "models": (0..150).map(|n| model(*kind, n)).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({"harnesses": harnesses, "runtimeModes": [
        {"id": "read-only", "label": "Read-only", "description": "Inspect without changing files"},
        {"id": "auto", "label": "Auto", "description": "Review approvals automatically"},
        {"id": "sideways", "label": "Unknown", "description": "Not a mode"},
    ]}))
}

#[gpui::test]
fn model_list_shows_settings_access_modes_and_the_callers_account(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[(CLIENT, &Scope::ALL)], cx);
    let (_, b) = sessions(window, cx);
    let work = Profile {
        id: "work".into(),
        root: None,
    };
    let account = work.clone();
    window
        .update(cx, |shell, _, _| shell.control.models = many_models)
        .unwrap();
    with_view(window, b, move |v| v.settings.account = account, cx);
    let all = Command::ModelList(ModelListInput {
        limit: Some(1000),
        ..ModelListInput::default()
    });
    let listed: ModelListOutput = by(window, b, None, all.clone(), cx).unwrap();
    for provider in &listed.providers {
        assert_eq!(
            (provider.models.len(), provider.omitted),
            (150, 0),
            "{}",
            provider.id
        );
    }
    let effort = &listed.providers[0].models[0].settings[0];
    assert_eq!(
        (
            effort.id.as_str(),
            effort.label.as_str(),
            effort.default_value.as_str()
        ),
        ("effort", "Effort", "medium")
    );
    let values: Vec<_> = effort
        .options
        .iter()
        .map(|o| (o.value.as_str(), o.label.as_str()))
        .collect();
    assert_eq!(values, [("low", "Low"), ("medium", "Medium")]);
    let modes: Vec<_> = listed
        .access_modes
        .iter()
        .map(|m| (m.id, m.label.as_str(), m.description.as_str()))
        .collect();
    assert_eq!(
        modes,
        [
            (
                AccessMode::ReadOnly,
                "Read-only",
                "Inspect without changing files"
            ),
            (AccessMode::Auto, "Auto", "Review approvals automatically"),
        ]
    );
    let asked = ASKED.lock().unwrap().clone();
    let account = |kind: Kind| asked.iter().find(|(k, _)| *k == kind).unwrap().1.clone();
    assert_eq!(
        account(Kind::Claude),
        work,
        "its own provider, its own account"
    );
    assert_ne!(
        account(Kind::Codex),
        work,
        "another provider, the project's account"
    );
    // A client lists with the project's accounts, 20 models by default.
    let theirs = call(
        window,
        CLIENT,
        None,
        Command::ModelList(ModelListInput::default()),
        cx,
    );
    let theirs: ModelListOutput = serde_json::from_value(theirs.unwrap()).unwrap();
    assert_eq!(
        (
            theirs.providers[0].models.len(),
            theirs.providers[0].omitted
        ),
        (20, 130)
    );
    assert_ne!(ASKED.lock().unwrap()[0].1, work);
}

#[gpui::test]
fn a_sessions_launch_inherits_its_settings_and_strips_its_capabilities(cx: &mut TestAppContext) {
    let window = setup(cx);
    let (_, b) = sessions(window, cx);
    with_view(
        window,
        b,
        |v| {
            v.settings.account = Profile {
                id: "work".into(),
                root: None,
            };
            v.settings.access = monocode_core::agent::AccessMode::AutoAcceptEdits;
            v.settings.model = Some("claude-x".into());
            v.settings.worktree_cwd = Some("/work/monocode-wt".into());
            v.settings.operator = true;
            v.settings.operator_enabled = true;
            v.settings.orchestrator = true;
            v.settings.environment.insert("SECRET".into(), "1".into());
        },
        cx,
    );
    let input = SessionStartInput {
        title: Some("  Parser fix  ".into()),
        ..draft("Fix the parser")
    };
    let started: SessionStartOutput = by(window, b, None, start(input), cx).unwrap();
    assert_eq!(
        (
            started.project.as_str(),
            started.harness.as_str(),
            started.access
        ),
        (HERE, "claude", AccessMode::AutoAcceptEdits)
    );
    assert_eq!(started.model.as_deref(), Some("claude-x"));
    assert_eq!(started.worktree_cwd.as_deref(), Some("/work/monocode-wt"));
    let child = started.session_id;
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(shell.started_by.get(&child), Some(&b));
            let view = shell.sessions[&child].read(cx);
            assert_eq!(view.summary.title, "Parser fix");
            let s = &view.settings;
            assert_eq!(s.account.id, "work");
            assert!(!s.operator && !s.operator_enabled && !s.orchestrator && !s.plan);
            assert!(s.environment.is_empty(), "no capability travels");
        })
        .unwrap();
    // Another harness, access or project changes what is inherited.
    let codex = SessionStartInput {
        harness: Some("codex".into()),
        access: Some(AccessMode::FullAccess),
        project: Some(OTHER.into()),
        ..draft("Port it")
    };
    let other: SessionStartOutput = by(window, b, None, start(codex), cx).unwrap();
    assert_eq!(
        (other.harness.as_str(), other.access),
        ("codex", AccessMode::FullAccess)
    );
    assert_ne!(
        other.model.as_deref(),
        Some("claude-x"),
        "another provider's model"
    );
    assert_eq!(
        other.worktree_cwd, None,
        "the worktree stays in its project"
    );
    window
        .update(cx, |shell, _, cx| {
            let view = shell.sessions[&other.session_id].read(cx);
            assert_ne!(view.settings.account.id, "work", "the project's account");
        })
        .unwrap();
    // Refusals start nothing.
    let count = |cx: &mut TestAppContext| {
        window
            .update(cx, |shell, _, _| shell.session_summaries.len())
            .unwrap()
    };
    let before = count(cx);
    let long = SessionStartInput {
        title: Some("t".repeat(161)),
        ..draft("Too long")
    };
    let notify = |unsent: bool| SessionStartInput {
        notify_on_complete: Some(true),
        draft: Some(unsent),
        ..draft("Report back")
    };
    for (input, message) in [
        (long, "Provide title (up to 160 bytes)."),
        (
            notify(false),
            "Completion notifications are only available in a Mono's chat",
        ),
        (
            notify(true),
            "An unsent draft cannot send a completion notification",
        ),
    ] {
        let refused = by::<SessionStartOutput>(window, b, None, start(input), cx).unwrap_err();
        assert_eq!(
            (refused.code, refused.message.as_str()),
            (ErrorCode::InvalidInput, message)
        );
    }
    assert_eq!(count(cx), before);
}
