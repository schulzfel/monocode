//! `workspace_read`, `project_select`, `project_add` and the tab
//! operations change the fixture window as the person's own clicks would.
use super::tests::{CLIENT, approve, call, run, setup};
use super::*;
use gpui::TestAppContext;
use monocode_control::{
    Command, Scope,
    ops::{
        dispatch::Placement,
        project::{ProjectAddInput, ProjectAddOutput, ProjectSelectInput, ProjectSelectOutput},
        session::{SessionReadInput, SessionReadOutput},
        tab::{AppView, TabCloseOutput, TabOpenInput, TabOutput, TabTarget},
        workspace::{LayoutStyle, PaneNode, TabKind, WorkspaceReadInput, WorkspaceReadOutput},
    },
};

const OTHER: &str = "/work/another-project";
const HERE: &str = env!("CARGO_MANIFEST_DIR");

fn navigator(cx: &mut TestAppContext) -> (gpui::WindowHandle<Shell>, i64, i64) {
    let window = setup(cx);
    approve(
        window,
        &[(CLIENT, &[Scope::Read, Scope::Navigate, Scope::Manage])],
        cx,
    );
    let (a, b) = window
        .update(cx, |shell, _, _| {
            let mut ids: Vec<_> = shell.session_summaries.keys().copied().collect();
            ids.sort_unstable();
            (ids[0], ids[1])
        })
        .unwrap();
    (window, a, b)
}

fn workspace(window: gpui::WindowHandle<Shell>, cx: &mut TestAppContext) -> WorkspaceReadOutput {
    run(window, Command::WorkspaceRead(WorkspaceReadInput {}), cx).unwrap()
}

fn tab(id: &str) -> TabTarget {
    TabTarget {
        tab: Some(id.into()),
        pane: None,
    }
}

fn pane(id: i64) -> TabTarget {
    TabTarget {
        tab: None,
        pane: Some(id),
    }
}

#[gpui::test]
fn workspace_read_reports_the_strip_panes_and_layout(cx: &mut TestAppContext) {
    let (window, a, b) = navigator(cx);
    let seen = workspace(window, cx);
    assert_eq!(seen.project, "/work/monocode");
    assert_eq!(seen.layout, LayoutStyle::Focused);
    let session = format!("session:{b}");
    let active: Vec<_> = seen.tabs.iter().filter(|t| t.active).collect();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, session);
    assert_eq!(active[0].kind, TabKind::Session);
    assert_eq!(
        seen.panes,
        Some(PaneNode::Pane {
            pane: b,
            session_id: b,
            file: false
        })
    );
    assert_eq!(seen.focused_pane, Some(b));
    // A split shows as a tree, and the ids work with tab_focus.
    window
        .update(cx, |shell, _, _| shell.workspace.place(a, b, Edge::Right))
        .unwrap();
    let split = workspace(window, cx);
    let Some(PaneNode::Split { children, .. }) = split.panes else {
        unreachable!("a split: {:?}", split.panes)
    };
    assert_eq!(children.len(), 2);
    let focused: TabOutput = run(window, Command::TabFocus(pane(a)), cx).unwrap();
    assert_eq!(focused.pane, Some(a));
    window
        .update(cx, |shell, _, _| {
            assert_eq!(shell.workspace.active, Some(a))
        })
        .unwrap();
}

#[gpui::test]
fn tab_open_shows_sessions_views_and_search_and_tab_close_closes_them(cx: &mut TestAppContext) {
    let (window, a, b) = navigator(cx);
    let open = |input: TabOpenInput, cx: &mut TestAppContext| {
        run::<TabOutput>(window, Command::TabOpen(input), cx)
    };
    let other = open(
        TabOpenInput {
            session_id: Some(a),
            ..TabOpenInput::default()
        },
        cx,
    )
    .unwrap();
    assert_eq!(other.project, OTHER, "its project becomes current");
    assert_eq!(other.tab, Some(format!("session:{a}")));
    let placed = open(
        TabOpenInput {
            session_id: Some(b),
            placement: Some(Placement::Right),
            beside_session_id: Some(a),
            ..TabOpenInput::default()
        },
        cx,
    );
    // b belongs to another project than a: placement keeps to one project.
    assert_eq!(placed.unwrap_err().code, ErrorCode::InvalidInput);
    let notes = open(
        TabOpenInput {
            view: Some(AppView::Notes),
            ..TabOpenInput::default()
        },
        cx,
    )
    .unwrap();
    assert_eq!(notes.tab.as_deref(), Some("app:notes"));
    let closed: TabCloseOutput = run(window, Command::TabClose(tab("app:notes")), cx).unwrap();
    assert!(closed.closed);
    window
        .update(cx, |shell, _, _| assert_eq!(shell.active_app_view, None))
        .unwrap();
    let again: TabCloseOutput = run(window, Command::TabClose(tab("app:notes")), cx).unwrap();
    assert!(!again.closed, "closing twice changes nothing");
    let search = open(
        TabOpenInput {
            search: Some(true),
            ..TabOpenInput::default()
        },
        cx,
    )
    .unwrap();
    assert_eq!(search.tab.as_deref(), Some("search"));
    let two = open(
        TabOpenInput {
            session_id: Some(a),
            search: Some(true),
            ..TabOpenInput::default()
        },
        cx,
    );
    assert_eq!(two.unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(
        run::<TabOutput>(window, Command::TabFocus(tab("nonsense")), cx)
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        run::<TabOutput>(window, Command::TabFocus(tab("session:999")), cx)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[gpui::test]
fn tab_close_closes_one_pane_of_a_split(cx: &mut TestAppContext) {
    let (window, a, b) = navigator(cx);
    window
        .update(cx, |shell, w, cx| {
            shell.workspace.place(a, b, Edge::Right);
            shell.activate_session(b, w, cx);
        })
        .unwrap();
    let closed: TabCloseOutput = run(window, Command::TabClose(pane(a)), cx).unwrap();
    assert!(closed.closed);
    window
        .update(cx, |shell, _, _| {
            let tab = shell.workspace.tab_of(b).unwrap();
            assert_eq!(shell.workspace.layout(tab).leaf_ids(), [b]);
            assert_eq!(shell.workspace.tab_of(a), None);
            assert!(shell.session_summaries.contains_key(&a), "never deleted");
        })
        .unwrap();
    let again: TabCloseOutput = run(window, Command::TabClose(pane(a)), cx).unwrap();
    assert!(!again.closed);
}

#[gpui::test]
fn tab_open_opens_a_project_file_at_its_line_and_refuses_outside_paths(cx: &mut TestAppContext) {
    let (window, _, b) = navigator(cx);
    // Make this crate's folder the project of the active session.
    window
        .update(cx, |shell, _, cx| {
            shell.workspace.add_project(HERE);
            shell.workspace.project = HERE.into();
            shell.sessions[&b].update(cx, |v, _| v.summary.cwd = HERE.into());
        })
        .unwrap();
    let open = |path: &str, cx: &mut TestAppContext| {
        run::<TabOutput>(
            window,
            Command::TabOpen(TabOpenInput {
                path: Some(path.into()),
                ..TabOpenInput::default()
            }),
            cx,
        )
    };
    open("Cargo.toml:3:2", cx).unwrap();
    let file = std::path::Path::new(HERE)
        .join("Cargo.toml")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    window
        .update(cx, |shell, _, cx| {
            let review = shell
                .reviews
                .get(&b)
                .expect("the file opens beside b")
                .read(cx);
            assert!(
                review.state.tabs.iter().any(
                    |t| matches!(t, crate::review::ReviewTab::File { path, .. } if *path == file)
                ),
                "{:?}",
                review.state.tabs
            );
        })
        .unwrap();
    let outside = open("/etc/hosts", cx).unwrap_err();
    assert_eq!(outside.code, ErrorCode::InvalidInput);
    assert!(
        outside.message.contains("not inside"),
        "{}",
        outside.message
    );
    assert_eq!(
        open("no-such-file.rs", cx).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        open("remote://box/a.rs", cx).unwrap_err().code,
        ErrorCode::RemoteUnsupported
    );
}

#[gpui::test]
fn project_select_and_add_never_show_the_folder_picker(cx: &mut TestAppContext) {
    let (window, _, _) = navigator(cx);
    let select = |project: &str, cx: &mut TestAppContext| {
        run::<ProjectSelectOutput>(
            window,
            Command::ProjectSelect(ProjectSelectInput {
                project: project.into(),
            }),
            cx,
        )
    };
    let moved = select(OTHER, cx).unwrap();
    assert!(moved.changed);
    window
        .update(cx, |shell, _, _| assert_eq!(shell.workspace.project, OTHER))
        .unwrap();
    assert!(!select(OTHER, cx).unwrap().changed, "already current");
    assert_eq!(
        select("/work/unknown", cx).unwrap_err().code,
        ErrorCode::ProjectNotOpen
    );
    let add = |path: &str, select: Option<bool>, cx: &mut TestAppContext| {
        run::<ProjectAddOutput>(
            window,
            Command::ProjectAdd(ProjectAddInput {
                path: path.into(),
                select,
            }),
            cx,
        )
    };
    let added = add(HERE, None, cx).unwrap();
    assert!(added.added && !added.current);
    let folder = std::path::Path::new(HERE).canonicalize().unwrap();
    assert_eq!(added.project, folder.to_string_lossy());
    let again = add(&format!("{HERE}/"), Some(true), cx).unwrap();
    assert!(!again.added, "a registered folder is not added twice");
    assert!(again.current);
    window
        .update(cx, |shell, _, _| {
            let count = shell
                .workspace
                .projects
                .iter()
                .filter(|p| **p == added.project)
                .count();
            assert_eq!(count, 1);
            assert_eq!(shell.workspace.project, added.project);
        })
        .unwrap();
    for (path, code) in [
        ("relative/folder", ErrorCode::InvalidInput),
        (&format!("{HERE}/Cargo.toml"), ErrorCode::InvalidInput),
        (&format!("{HERE}/no-such-folder"), ErrorCode::NotFound),
        ("remote://box/srv", ErrorCode::RemoteUnsupported),
    ] {
        assert_eq!(add(path, None, cx).unwrap_err().code, code, "{path}");
    }
    window
        .update(cx, |shell, _, _| {
            shell.workspace.add_project("remote://box/srv/app")
        })
        .unwrap();
    assert_eq!(
        select("remote://box/srv/app", cx).unwrap_err().code,
        ErrorCode::RemoteUnsupported
    );
}

#[gpui::test]
fn navigation_needs_the_navigate_scope(cx: &mut TestAppContext) {
    let (window, a, _) = navigator(cx);
    approve(window, &[("Reader", &[Scope::Read])], cx);
    let refused = call(window, "Reader", None, Command::TabFocus(pane(a)), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ScopeDenied);
    let add = call(
        window,
        "Reader",
        None,
        Command::ProjectAdd(ProjectAddInput {
            path: HERE.into(),
            select: None,
        }),
        cx,
    )
    .unwrap_err();
    assert!(add.message.contains("manage scope"), "{}", add.message);
    assert!(
        call(
            window,
            "Reader",
            None,
            Command::WorkspaceRead(WorkspaceReadInput {}),
            cx
        )
        .is_ok()
    );
}

#[gpui::test]
fn session_read_pages_exchanges_and_tab_open_places_a_session_beside_another(
    cx: &mut TestAppContext,
) {
    let (window, a, b) = navigator(cx);
    let row = |role: &str, text: &str| monocode_core::transcript::Row {
        role: role.into(),
        text: text.into(),
        first: true,
        sent_at: None,
    };
    window
        .update(cx, |shell, _, _| {
            locked(&shell.store)
                .unwrap()
                .write_rows(
                    b,
                    0,
                    &[row("user", "Fix the build"), row("assistant", "Fixed it.")],
                )
                .unwrap();
        })
        .unwrap();
    let read: SessionReadOutput = run(
        window,
        Command::SessionRead(SessionReadInput {
            session_id: b,
            ..SessionReadInput::default()
        }),
        cx,
    )
    .unwrap();
    assert_eq!(read.turns.len(), 1);
    assert_eq!(read.turns[0].user.text, "Fix the build");
    assert_eq!(read.turns[0].assistant.as_ref().unwrap().text, "Fixed it.");
    assert_eq!(read.next_before, None);
    let too_many = run::<SessionReadOutput>(
        window,
        Command::SessionRead(SessionReadInput {
            session_id: b,
            limit: Some(4),
            ..SessionReadInput::default()
        }),
        cx,
    );
    assert_eq!(too_many.unwrap_err().code, ErrorCode::InvalidInput);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&a].update(cx, |v, _| v.summary.cwd = "/work/monocode".into());
        })
        .unwrap();
    let placed: TabOutput = run(
        window,
        Command::TabOpen(TabOpenInput {
            session_id: Some(a),
            placement: Some(Placement::Down),
            ..TabOpenInput::default()
        }),
        cx,
    )
    .unwrap();
    assert_eq!(placed.pane, Some(a));
    window
        .update(cx, |shell, _, _| {
            let tab = shell.workspace.tab_of(b).unwrap();
            let layout = shell.workspace.layout(tab);
            assert!(
                layout.contains(a) && layout.contains(b),
                "below the focused pane"
            );
        })
        .unwrap();
}
