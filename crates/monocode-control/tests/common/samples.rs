//! One call of every operation, in registry order.
use monocode_control::{
    AccessMode, Command,
    ops::{
        app::AppInfoInput,
        automation::{
            AutomationInput, AutomationListInput, AutomationReadInput, AutomationSetEnabledInput,
            AutomationUpsertInput, Schedule, ScheduleKind,
        },
        changes::ChangesReadInput,
        configure::{SessionConfigureInput, SessionDraftInput},
        dispatch::{SessionSendInput, SessionStartInput, SessionWaitInput},
        layout::{Edge, LayoutSetInput, PaneSplitInput, SidebarTab, TabMoveInput},
        lifecycle::SessionHandoffInput,
        model::ModelListInput,
        mono::{MonoInput, MonoListInput, MonoSendInput},
        mono_artifact::{
            ArtifactInput, ArtifactKind, MonoArtifactListInput, MonoArtifactWriteInput,
        },
        mono_card::MonoCardPostInput,
        mono_habit::{
            HabitInput, HabitSchedule, MonoHabitAddInput, MonoHabitListInput, MonoHabitUpdateInput,
        },
        mono_memory::{
            MonoMemoryAddInput, MonoMemoryReadInput, MonoMemoryRemoveInput, MonoMemoryReplaceInput,
            MonoMemorySearchInput,
        },
        mono_soul::{MonoSoulReadInput, MonoSoulUpdateInput},
        notes::{NotesListInput, NotesReadInput, NotesWriteInput},
        organize::{ProjectInput, SessionArchiveInput, SessionMoveInput, SessionRenameInput},
        project::{ProjectAddInput, ProjectListInput, ProjectSelectInput},
        question::{ItemAnswer, QuestionAnswerInput, QuestionListInput},
        queue::{QueueEditInput, QueueItemInput},
        rail::{ProjectReorderInput, ProjectTarget, ProjectUpdateInput},
        search::{SearchProjectInput, SearchSessionsInput},
        session::{SessionInput, SessionListInput, SessionReadInput},
        settings::{AlertsUpdate, SettingsReadInput, SettingsUpdateInput, Theme},
        tab::{TabOpenInput, TabTarget},
        vcs::{GitCommitInput, GitPathsInput},
        workspace::WorkspaceReadInput,
        worktree::WorktreeCreateInput,
    },
};

/// Every operation's sample call, in registry order. Calls that name a
/// session name session 3.
#[must_use]
pub fn commands() -> Vec<Command> {
    let mut commands = app_commands();
    commands.extend(session_commands());
    commands.extend(workspace_commands());
    commands.extend(lifecycle_commands());
    commands.extend(mono_commands());
    commands
}

/// The app, project, workspace, session and tab calls.
fn app_commands() -> Vec<Command> {
    vec![
        Command::AppInfo(AppInfoInput {}),
        Command::ModelList(ModelListInput {
            provider: Some("claude".into()),
            search: Some("sonnet".into()),
            limit: Some(5),
        }),
        Command::ProjectList(ProjectListInput {
            cursor: Some("50".into()),
            limit: Some(10),
        }),
        Command::ProjectSelect(ProjectSelectInput {
            project: "/work/monocode".into(),
        }),
        Command::ProjectAdd(ProjectAddInput {
            path: "/work/new".into(),
            select: Some(true),
        }),
        Command::WorkspaceRead(WorkspaceReadInput {}),
        Command::SessionList(SessionListInput::default()),
        Command::SessionRead(SessionReadInput {
            session_id: 3,
            before: Some("row-4".into()),
            limit: Some(2),
            max_chars: Some(500),
            ..SessionReadInput::default()
        }),
        Command::SessionStatus(SessionInput { session_id: 3 }),
        Command::SessionStart(SessionStartInput {
            prompt: "Fix the tests".into(),
            access: Some(AccessMode::ReadOnly),
            ..SessionStartInput::default()
        }),
        Command::SessionSend(SessionSendInput {
            session_id: 3,
            prompt: "And the docs".into(),
            ..SessionSendInput::default()
        }),
        Command::SessionWait(SessionWaitInput {
            session_id: 3,
            timeout_ms: Some(1000),
        }),
        Command::SessionStop(SessionInput { session_id: 3 }),
        Command::TabOpen(TabOpenInput {
            path: Some("src/main.rs:3:1".into()),
            ..TabOpenInput::default()
        }),
        Command::TabFocus(TabTarget {
            tab: Some("session:3".into()),
            pane: None,
        }),
        Command::TabClose(TabTarget {
            tab: None,
            pane: Some(-2),
        }),
    ]
}

/// The configure, organize, question and queue calls.
fn session_commands() -> Vec<Command> {
    let session = SessionInput { session_id: 3 };
    let item = QueueItemInput {
        session_id: 3,
        message_id: "m1".into(),
    };
    vec![
        Command::SessionConfigure(SessionConfigureInput {
            session_id: 3,
            effort: Some("high".into()),
            access: Some(AccessMode::ReadOnly),
            ..SessionConfigureInput::default()
        }),
        Command::SessionDraft(SessionDraftInput {
            session_id: 3,
            text: "Try again".into(),
        }),
        Command::SessionRename(SessionRenameInput {
            session_id: 3,
            title: "Docs".into(),
        }),
        Command::SessionArchive(SessionArchiveInput {
            session_id: 3,
            stop: Some(true),
        }),
        Command::SessionRestore(session.clone()),
        Command::FolderList(ProjectInput::default()),
        Command::SessionMove(SessionMoveInput {
            session_id: 3,
            new_folder_name: Some("Later".into()),
            ..SessionMoveInput::default()
        }),
        Command::QuestionList(QuestionListInput::default()),
        Command::QuestionAnswer(QuestionAnswerInput {
            session_id: 3,
            question_id: "q".into(),
            answers: vec![ItemAnswer {
                item_id: "one".into(),
                options: vec!["first".into()],
                text: None,
            }],
        }),
        Command::QueueList(session.clone()),
        Command::QueueRemove(item.clone()),
        Command::QueueSteer(item),
        Command::QueueResume(session),
        Command::QueueEdit(QueueEditInput {
            session_id: 3,
            message_id: "m1".into(),
            text: "Shorter".into(),
        }),
    ]
}

/// The worktree, rail, layout, settings, search, notes and changes calls.
fn workspace_commands() -> Vec<Command> {
    vec![
        Command::WorktreeList(ProjectInput::default()),
        Command::WorktreeCreate(WorktreeCreateInput {
            branch: "feature".into(),
            ..WorktreeCreateInput::default()
        }),
        Command::ProjectUpdate(ProjectUpdateInput {
            project: "/work/monocode".into(),
            mascot: Some("crab".into()),
            ..ProjectUpdateInput::default()
        }),
        Command::ProjectArchive(ProjectTarget {
            project: "/work/old".into(),
        }),
        Command::ProjectUnarchive(ProjectTarget {
            project: "/work/old".into(),
        }),
        Command::ProjectReorder(ProjectReorderInput {
            order: vec!["/work/b".into(), "/work/a".into()],
        }),
        Command::TabMove(TabMoveInput {
            session_id: 3,
            beside_session_id: Some(4),
            edge: Some(Edge::Bottom),
            index: None,
        }),
        Command::PaneSplit(PaneSplitInput {
            session_id: 3,
            ..PaneSplitInput::default()
        }),
        Command::LayoutSet(LayoutSetInput {
            sidebar_tab: Some(SidebarTab::Files),
            ..LayoutSetInput::default()
        }),
        Command::SettingsRead(SettingsReadInput {}),
        Command::SettingsUpdate(SettingsUpdateInput {
            theme: Some(Theme::Light),
            alerts: Some(AlertsUpdate {
                sounds: Some(false),
                notifications: None,
            }),
            ..SettingsUpdateInput::default()
        }),
        Command::SearchSessions(SearchSessionsInput {
            query: "flaky".into(),
            ..SearchSessionsInput::default()
        }),
        Command::SearchProject(SearchProjectInput {
            query: "fn main".into(),
            limit: Some(10),
            ..SearchProjectInput::default()
        }),
        Command::NotesList(NotesListInput::default()),
        Command::NotesRead(NotesReadInput { id: "n1".into() }),
        Command::NotesWrite(NotesWriteInput {
            body: Some("# Plan".into()),
            ..NotesWriteInput::default()
        }),
        Command::ChangesRead(ChangesReadInput {
            session_id: Some(3),
            ..ChangesReadInput::default()
        }),
    ]
}

/// A source control call on `src/main.rs` in `/work/repo`.
#[must_use]
pub fn paths() -> GitPathsInput {
    GitPathsInput {
        project: Some("/work/repo".into()),
        paths: vec!["src/main.rs".into()],
        all: None,
    }
}

/// The compaction, handoff, delete, source control, automation and Mono
/// calls.
fn lifecycle_commands() -> Vec<Command> {
    let session = SessionInput { session_id: 3 };
    let automation = AutomationInput {
        automation_id: "a1".into(),
    };
    let project = ProjectInput {
        project: Some("/work/repo".into()),
    };
    vec![
        Command::SessionCompact(session.clone()),
        Command::SessionHandoff(SessionHandoffInput {
            session_id: 3,
            harness: "claude".into(),
            access: Some(AccessMode::ReadOnly),
            ..SessionHandoffInput::default()
        }),
        Command::SessionDelete(session),
        Command::ProjectDelete(ProjectTarget {
            project: "/work/repo".into(),
        }),
        Command::GitStage(paths()),
        Command::GitUnstage(GitPathsInput {
            all: Some(true),
            paths: Vec::new(),
            ..paths()
        }),
        Command::GitCommit(GitCommitInput {
            project: None,
            message: "Fix the parser".into(),
        }),
        Command::GitDiscard(paths()),
        Command::GitPush(project),
        Command::AutomationList(AutomationListInput::default()),
        Command::AutomationRead(AutomationReadInput {
            automation_id: "a1".into(),
            runs: Some(5),
        }),
        Command::AutomationRun(automation.clone()),
        Command::AutomationSetEnabled(AutomationSetEnabledInput {
            automation_id: "a1".into(),
            enabled: true,
        }),
        Command::AutomationUpsert(AutomationUpsertInput {
            name: Some("Nightly".into()),
            runtime_mode: Some(AccessMode::Supervised),
            schedules: Some(vec![Schedule {
                kind: ScheduleKind::Weekly,
                minute: None,
                time: Some("08:00".into()),
                day_of_week: Some(1),
            }]),
            ..AutomationUpsertInput::default()
        }),
        Command::AutomationDelete(automation),
        Command::MonoList(MonoListInput {}),
        Command::MonoOpen(MonoInput {
            mono_id: "m1".into(),
        }),
        Command::MonoSend(MonoSendInput {
            mono_id: "m1".into(),
            text: "Hello".into(),
        }),
    ]
}

/// The calls only a Mono makes, on its own memory, soul, habits, artifacts
/// and chat.
fn mono_commands() -> Vec<Command> {
    let daily = HabitSchedule {
        schedule_kind: "daily".into(),
        time: Some("09:00".into()),
        ..HabitSchedule::default()
    };
    let habit = HabitInput { id: "h1".into() };
    vec![
        Command::MonoMemoryRead(MonoMemoryReadInput {
            topic: Some("Tooling".into()),
        }),
        Command::MonoMemorySearch(MonoMemorySearchInput {
            query: "release checklist".into(),
            since: Some("7d".into()),
        }),
        Command::MonoMemoryAdd(MonoMemoryAddInput {
            fact: "Releases ship on Fridays".into(),
            until: Some("2026-12-31".into()),
            ..MonoMemoryAddInput::default()
        }),
        Command::MonoMemoryReplace(MonoMemoryReplaceInput {
            find: "Fridays".into(),
            fact: "Releases ship on Thursdays".into(),
            ..MonoMemoryReplaceInput::default()
        }),
        Command::MonoMemoryRemove(MonoMemoryRemoveInput {
            find: "Thursdays".into(),
            topic: None,
        }),
        Command::MonoSoulRead(MonoSoulReadInput {}),
        Command::MonoSoulUpdate(MonoSoulUpdateInput {
            text: "# Soul\nBe brief.\n".into(),
            expected_hash: "e3b0c442".into(),
        }),
        Command::MonoHabitList(MonoHabitListInput {}),
        Command::MonoHabitAdd(MonoHabitAddInput {
            name: "Morning digest".into(),
            instructions: "Summarize yesterday's changes".into(),
            schedule: daily.clone(),
        }),
        Command::MonoHabitUpdate(MonoHabitUpdateInput {
            id: "h1".into(),
            enabled: Some(false),
            ..MonoHabitUpdateInput::default()
        }),
        Command::MonoHabitRun(habit.clone()),
        Command::MonoHabitRemove(habit),
        Command::MonoArtifactList(MonoArtifactListInput {
            kind: Some(ArtifactKind::Html),
            limit: Some(10),
            cursor: Some("10".into()),
        }),
        Command::MonoArtifactRead(ArtifactInput { id: "a1".into() }),
        Command::MonoArtifactWrite(MonoArtifactWriteInput {
            title: "Release notes".into(),
            body: "# 0.16\nShipped.".into(),
            summary: Some("What shipped".into()),
            ..MonoArtifactWriteInput::default()
        }),
        // A card that names no session: posting one is not an action on it.
        Command::MonoCardPost(MonoCardPostInput::Choices {
            options: vec!["Ship it".into(), "Wait".into()],
        }),
    ]
}
