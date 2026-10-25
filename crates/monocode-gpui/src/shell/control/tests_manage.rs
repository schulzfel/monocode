//! `session_configure`, `session_draft`, session organising, the project
//! rail and worktrees against the fixture shell.
use super::tests::{CLIENT, approve, repository, run, two_sessions};
use super::*;
use gpui::TestAppContext;
use monocode_control::{
    AccessMode, Command, Scope,
    ops::{
        configure::{
            SessionConfigureInput, SessionConfigureOutput, SessionDraftInput, SessionDraftOutput,
        },
        organize::{
            FolderListOutput, ProjectInput, SessionArchiveOutput, SessionMoveInput,
            SessionMoveOutput, SessionRenameInput, SessionRenameOutput,
        },
        rail::{
            ProjectArchiveOutput, ProjectReorderInput, ProjectReorderOutput, ProjectTarget,
            ProjectUpdateInput, ProjectUpdateOutput,
        },
        session::SessionInput,
        worktree::{WorktreeCreateInput, WorktreeCreateOutput, WorktreeListOutput},
    },
};

const HERE: &str = "/work/monocode";
const OTHER: &str = "/work/another-project";

fn configure(id: i64, access: AccessMode) -> Command {
    Command::SessionConfigure(SessionConfigureInput {
        session_id: id,
        access: Some(access),
        ..SessionConfigureInput::default()
    })
}

#[gpui::test]
fn session_configure_changes_access_and_never_while_busy(cx: &mut TestAppContext) {
    let (window, _, b) = two_sessions(&[Scope::Read, Scope::Dispatch, Scope::Manage], cx);
    let full: SessionConfigureOutput =
        run(window, configure(b, AccessMode::FullAccess), cx).unwrap();
    assert_eq!(full.access, AccessMode::FullAccess, "there is no ceiling");
    let set: SessionConfigureOutput = run(window, configure(b, AccessMode::ReadOnly), cx).unwrap();
    assert_eq!(
        (set.access, set.harness.as_str()),
        (AccessMode::ReadOnly, "claude")
    );
    window
        .update(cx, |shell, _, cx| {
            let view = shell.sessions[&b].read(cx);
            assert_eq!(view.settings.access.id(), "read-only", "the chat uses it");
        })
        .unwrap();
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&b].update(cx, |view, _| view.busy = true);
        })
        .unwrap();
    let busy = run::<SessionConfigureOutput>(window, configure(b, AccessMode::Supervised), cx);
    let busy = busy.unwrap_err();
    assert_eq!((busy.code, busy.retryable), (ErrorCode::SessionBusy, true));
}

#[gpui::test]
fn session_draft_leaves_a_draft_once_and_keeps_the_persons(cx: &mut TestAppContext) {
    let (window, a, _) = two_sessions(&[Scope::Read, Scope::Dispatch, Scope::Manage], cx);
    let draft = |text: &str| {
        Command::SessionDraft(SessionDraftInput {
            session_id: a,
            text: text.into(),
        })
    };
    // Its history is still loading: not yet, but worth a retry.
    let loading = run::<SessionDraftOutput>(window, draft("Too early"), cx).unwrap_err();
    assert_eq!(
        (loading.code, loading.retryable),
        (ErrorCode::SessionBusy, true)
    );
    cx.run_until_parked();
    let saved: SessionDraftOutput = run(window, draft("Look at the flaky test"), cx).unwrap();
    assert!(saved.saved);
    window
        .update(cx, |shell, _, cx| {
            assert!(shell.sessions[&a].read(cx).has_saved_draft());
        })
        .unwrap();
    let again = run::<SessionDraftOutput>(window, draft("Something else"), cx).unwrap_err();
    assert_eq!(again.code, ErrorCode::SessionNotIdle);
}

#[gpui::test]
fn sessions_are_renamed_archived_restored_and_moved_into_folders(cx: &mut TestAppContext) {
    let (window, a, b) = two_sessions(&[Scope::Read, Scope::Dispatch, Scope::Manage], cx);
    let renamed: SessionRenameOutput = run(
        window,
        Command::SessionRename(SessionRenameInput {
            session_id: b,
            title: "  Docs pass ".into(),
        }),
        cx,
    )
    .unwrap();
    assert_eq!(renamed.title, "Docs pass");
    let long = Command::SessionRename(SessionRenameInput {
        session_id: b,
        title: "x".repeat(201),
    });
    assert_eq!(
        run::<SessionRenameOutput>(window, long, cx)
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    let archived: SessionArchiveOutput = run(
        window,
        Command::SessionArchive(SessionInput { session_id: a }),
        cx,
    )
    .unwrap();
    assert!(archived.archived);
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(shell.session_summaries[&b].title, "Docs pass");
            assert!(shell.session_metadata(a, cx).summary.archived);
            assert!(!shell.workspace.tabs.contains(&a), "its tab closed");
            let db = locked(&shell.store).unwrap();
            assert!(
                db.sessions()
                    .unwrap()
                    .iter()
                    .any(|s| s.id == a && s.archived)
            );
        })
        .unwrap();
    let restored: SessionArchiveOutput = run(
        window,
        Command::SessionRestore(SessionInput { session_id: a }),
        cx,
    )
    .unwrap();
    assert!(!restored.archived);
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.session_metadata(a, cx).summary.archived);
        })
        .unwrap();
    let moved: SessionMoveOutput = run(
        window,
        Command::SessionMove(SessionMoveInput {
            session_id: a,
            new_folder_name: Some("Later".into()),
            ..SessionMoveInput::default()
        }),
        cx,
    )
    .unwrap();
    assert_eq!(moved.folder_name, "Later");
    let folders: FolderListOutput = run(
        window,
        Command::FolderList(ProjectInput {
            project: Some(OTHER.into()),
        }),
        cx,
    )
    .unwrap();
    assert_eq!(folders.folders.len(), 1);
    assert_eq!(folders.folders[0].session_ids, [a]);
    for (folder, name, code) in [
        (Some("nope"), None, ErrorCode::NotFound),
        (
            Some(moved.folder_id.as_str()),
            Some("Both"),
            ErrorCode::InvalidInput,
        ),
        (None, None, ErrorCode::InvalidInput),
    ] {
        let wrong = Command::SessionMove(SessionMoveInput {
            session_id: a,
            folder_id: folder.map(str::to_owned),
            new_folder_name: name.map(str::to_owned),
        });
        assert_eq!(
            run::<SessionMoveOutput>(window, wrong, cx)
                .unwrap_err()
                .code,
            code
        );
    }
}

#[gpui::test]
fn the_rail_is_renamed_coloured_archived_and_reordered(cx: &mut TestAppContext) {
    let (window, _, _) = two_sessions(&[Scope::Read, Scope::Dispatch, Scope::Manage], cx);
    let update = |color: &str, mascot: &str| {
        Command::ProjectUpdate(ProjectUpdateInput {
            project: HERE.into(),
            name: Some("MonoCode".into()),
            color: Some(color.into()),
            mascot: Some(mascot.into()),
        })
    };
    let shown: ProjectUpdateOutput = run(window, update("#EA603E", "crab"), cx).unwrap();
    assert_eq!(
        (
            shown.name.as_str(),
            shown.color.as_str(),
            shown.mascot.as_str()
        ),
        ("MonoCode", "#ea603e", "crab")
    );
    for bad in [update("red", "crab"), update("", "unicorn")] {
        let error = run::<ProjectUpdateOutput>(window, bad, cx).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
    let order = |order: &[&str]| {
        Command::ProjectReorder(ProjectReorderInput {
            order: order.iter().map(|p| (*p).to_owned()).collect(),
        })
    };
    let reordered: ProjectReorderOutput = run(window, order(&[HERE, OTHER]), cx).unwrap();
    assert_eq!(reordered.order, [HERE, OTHER]);
    for (wrong, code) in [
        (order(&[HERE]), ErrorCode::InvalidInput),
        (order(&[HERE, HERE]), ErrorCode::InvalidInput),
        (order(&[HERE, "/work/nowhere"]), ErrorCode::ProjectNotOpen),
    ] {
        assert_eq!(
            run::<ProjectReorderOutput>(window, wrong, cx)
                .unwrap_err()
                .code,
            code
        );
    }
    let target = || ProjectTarget {
        project: OTHER.into(),
    };
    let archived: ProjectArchiveOutput =
        run(window, Command::ProjectArchive(target()), cx).unwrap();
    assert!(archived.archived);
    let again: ProjectArchiveOutput = run(window, Command::ProjectArchive(target()), cx).unwrap();
    assert!(again.archived, "archiving twice is a no-op");
    window
        .update(cx, |shell, _, _| {
            assert_eq!(shell.workspace.projects, [HERE]);
            assert_eq!(shell.workspace.archived_projects, [OTHER]);
        })
        .unwrap();
    let back: ProjectArchiveOutput = run(window, Command::ProjectUnarchive(target()), cx).unwrap();
    assert!(!back.archived);
    window
        .update(cx, |shell, _, _| {
            assert!(shell.workspace.projects.contains(&OTHER.to_owned()));
        })
        .unwrap();
}

#[gpui::test]
fn worktrees_are_listed_and_created_beside_the_repository(cx: &mut TestAppContext) {
    let (window, _, _) = two_sessions(&[Scope::Read, Scope::Dispatch, Scope::Manage], cx);
    let repo = repository("g6");
    window
        .update(cx, |shell, _, _| shell.workspace.add_project(&repo))
        .unwrap();
    let project = || Some(repo.clone());
    let listed: WorktreeListOutput = run(
        window,
        Command::WorktreeList(ProjectInput { project: project() }),
        cx,
    )
    .unwrap();
    assert_eq!(listed.worktrees.len(), 1);
    assert!(listed.worktrees[0].main);
    let created: WorktreeCreateOutput = run(
        window,
        Command::WorktreeCreate(WorktreeCreateInput {
            project: project(),
            branch: "feature-x".into(),
            ..WorktreeCreateInput::default()
        }),
        cx,
    )
    .unwrap();
    assert_eq!(created.worktree.branch.as_deref(), Some("feature-x"));
    assert!(
        created
            .worktree
            .path
            .starts_with(&format!("{repo}-worktrees"))
    );
    let twice = run::<WorktreeCreateOutput>(
        window,
        Command::WorktreeCreate(WorktreeCreateInput {
            project: project(),
            branch: "feature-x".into(),
            ..WorktreeCreateInput::default()
        }),
        cx,
    );
    assert_eq!(twice.unwrap_err().code, ErrorCode::InvalidInput);
    let listed: WorktreeListOutput = run(
        window,
        Command::WorktreeList(ProjectInput { project: project() }),
        cx,
    )
    .unwrap();
    assert_eq!(listed.worktrees.len(), 2);
    let remote = run::<WorktreeListOutput>(
        window,
        Command::WorktreeList(ProjectInput {
            project: Some("/work/nowhere".into()),
        }),
        cx,
    );
    assert_eq!(remote.unwrap_err().code, ErrorCode::ProjectNotOpen);
}

#[gpui::test]
fn organising_needs_the_manage_scope(cx: &mut TestAppContext) {
    let (window, _, b) = two_sessions(&[Scope::Read, Scope::Dispatch, Scope::Manage], cx);
    approve(window, &[(CLIENT, &[Scope::Read, Scope::Dispatch])], cx);
    let denied = run::<SessionArchiveOutput>(
        window,
        Command::SessionArchive(SessionInput { session_id: b }),
        cx,
    );
    assert_eq!(denied.unwrap_err().code, ErrorCode::ScopeDenied);
}
