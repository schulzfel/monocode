//! `tab_move`, `pane_split`, `layout_set`, the settings allowlist, search,
//! notes and changes against the fixture shell.
use super::tests::{CLIENT, approve, call, repository, run, scratch, two_sessions};
use super::*;
use gpui::TestAppContext;
use monocode_control::{
    Command, Scope,
    ops::{
        changes::{ChangesReadInput, ChangesReadOutput},
        layout::{
            Edge, LayoutSetInput, LayoutSetOutput, PaneSplitInput, SidebarTab, SplitSide,
            TabMoveInput,
        },
        notes::{NoteOutput, NotesListInput, NotesListOutput, NotesReadInput, NotesWriteInput},
        search::{
            HitKind, SearchProjectInput, SearchProjectOutput, SearchSessionsInput,
            SearchSessionsOutput,
        },
        settings::{
            AlertsUpdate, FavoriteModel, FileSettingsUpdate, FollowUpMode, SettingsReadInput,
            SettingsUpdateInput, SettingsUpdateOutput, SettingsValues, Theme,
        },
        tab::TabOutput,
        workspace::LayoutStyle,
    },
};

const HERE: &str = "/work/monocode";

#[gpui::test]
fn panes_split_detach_and_whole_tabs_move_beside_a_pane(cx: &mut TestAppContext) {
    let (window, a, b) = two_sessions(
        &[Scope::Read, Scope::Navigate, Scope::Manage, Scope::Settings],
        cx,
    );
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&a].update(cx, |v, _| v.summary.cwd = HERE.into());
        })
        .unwrap();
    let layout = |window: gpui::WindowHandle<Shell>, cx: &mut TestAppContext| {
        window
            .update(cx, |shell, _, _| {
                let tab = shell.workspace.tab_of(b).unwrap();
                (
                    shell.workspace.layout(tab).contains(a),
                    shell.workspace.tabs.contains(&a),
                )
            })
            .unwrap()
    };
    let split: TabOutput = run(
        window,
        Command::PaneSplit(PaneSplitInput {
            session_id: a,
            beside_session_id: Some(b),
            side: Some(SplitSide::Down),
        }),
        cx,
    )
    .unwrap();
    assert_eq!(split.pane, Some(a));
    assert_eq!(layout(window, cx), (true, false), "a split of b's tab");
    let detach = |index| {
        Command::TabMove(TabMoveInput {
            session_id: a,
            index,
            ..TabMoveInput::default()
        })
    };
    run::<TabOutput>(window, detach(None), cx).unwrap();
    assert_eq!(layout(window, cx), (false, true), "its own tab again");
    let alone = run::<TabOutput>(window, detach(Some(0)), cx).unwrap_err();
    assert_eq!(alone.code, ErrorCode::InvalidInput, "already its own tab");
    run::<TabOutput>(
        window,
        Command::TabMove(TabMoveInput {
            session_id: a,
            beside_session_id: Some(b),
            edge: Some(Edge::Left),
            index: None,
        }),
        cx,
    )
    .unwrap();
    assert_eq!(
        layout(window, cx),
        (true, false),
        "the whole tab moved beside b"
    );
    let missing = run::<TabOutput>(
        window,
        Command::TabMove(TabMoveInput {
            session_id: a,
            beside_session_id: Some(999),
            ..TabMoveInput::default()
        }),
        cx,
    );
    assert_eq!(missing.unwrap_err().code, ErrorCode::NotFound);
}

#[gpui::test]
fn layout_set_sets_explicitly_and_reports_what_it_could_not(cx: &mut TestAppContext) {
    let (window, _, _) = two_sessions(
        &[Scope::Read, Scope::Navigate, Scope::Manage, Scope::Settings],
        cx,
    );
    let set = |input: LayoutSetInput, cx: &mut TestAppContext| {
        run::<LayoutSetOutput>(window, Command::LayoutSet(input), cx).unwrap()
    };
    let classic = set(
        LayoutSetInput {
            style: Some(LayoutStyle::Classic),
            sidebar_tab: Some(SidebarTab::Files),
            ..LayoutSetInput::default()
        },
        cx,
    );
    assert_eq!(
        (classic.style, classic.sidebar_tab),
        (LayoutStyle::Classic, SidebarTab::Files)
    );
    let again = set(
        LayoutSetInput {
            style: Some(LayoutStyle::Classic),
            ..LayoutSetInput::default()
        },
        cx,
    );
    assert_eq!(again.style, LayoutStyle::Classic, "a set, not a toggle");
    let focused = set(
        LayoutSetInput {
            style: Some(LayoutStyle::Focused),
            boxy: Some(true),
            sidebar_tab: Some(SidebarTab::Changes),
            right_panel_open: Some(true),
            primary_sidebar_visible: Some(false),
            ..LayoutSetInput::default()
        },
        cx,
    );
    assert_eq!((focused.style, focused.boxy), (LayoutStyle::Focused, true));
    assert!(!focused.primary_sidebar_visible);
    assert!(focused.skipped.iter().any(|s| s.starts_with("sidebarTab")));
    assert_eq!(
        focused
            .skipped
            .iter()
            .any(|s| s.starts_with("rightPanelOpen")),
        !focused.right_panel_open,
        "an open request either opens the panel or says why not: {:?}",
        focused.skipped
    );
    let closed = set(
        LayoutSetInput {
            right_panel_open: Some(false),
            ..LayoutSetInput::default()
        },
        cx,
    );
    assert!(!closed.right_panel_open && closed.skipped.is_empty());
    window
        .update(cx, |shell, _, _| {
            assert!(!shell.workspace.classic_layout && shell.workspace.new_layout_boxy);
            assert!(!shell.workspace.sidebar_visible);
        })
        .unwrap();
}

fn update() -> SettingsUpdateInput {
    SettingsUpdateInput {
        theme: Some(Theme::Light),
        ui_scale: Some(120),
        alerts: Some(AlertsUpdate {
            sounds: Some(false),
            notifications: None,
        }),
        files: Some(FileSettingsUpdate {
            autosave: Some(true),
            ..FileSettingsUpdate::default()
        }),
        follow_up: Some(FollowUpMode::Queue),
        default_provider: Some("claude".into()),
        favorite_models: Some(vec![FavoriteModel {
            provider: "claude".into(),
            model: Some("opus".into()),
        }]),
        ..SettingsUpdateInput::default()
    }
}

#[gpui::test]
fn settings_update_applies_only_allowlisted_preferences_as_settings_does(cx: &mut TestAppContext) {
    let (window, _, _) = two_sessions(
        &[Scope::Read, Scope::Navigate, Scope::Manage, Scope::Settings],
        cx,
    );
    let before = window
        .update(cx, |shell, _, _| shell.workspace.preferences.clone())
        .unwrap();
    let done: SettingsUpdateOutput = run(window, Command::SettingsUpdate(update()), cx).unwrap();
    assert_eq!(
        done.changed,
        [
            "alerts.sounds",
            "defaultProvider",
            "favoriteModels",
            "files.autosave",
            "followUp",
            "theme",
            "uiScale"
        ]
    );
    let read: SettingsValues =
        run(window, Command::SettingsRead(SettingsReadInput {}), cx).unwrap();
    assert_eq!(read, done.settings);
    window
        .update(cx, |shell, _, cx| {
            let p = &shell.workspace.preferences;
            assert_eq!(
                (p.theme.as_str(), p.ui_scale, p.sounds, p.autosave),
                ("light", 120, false, true)
            );
            assert_eq!(p.follow_up, "queue");
            assert_eq!(
                cx.global::<crate::settings::Preferences>(),
                p,
                "applied app-wide"
            );
            assert_eq!(
                (
                    &p.binary_paths,
                    p.claude_hooks,
                    &p.default_access,
                    &p.keybindings,
                    p.mask_emails
                ),
                (
                    &before.binary_paths,
                    before.claude_hooks,
                    &before.default_access,
                    &before.keybindings,
                    before.mask_emails
                ),
                "nothing outside the allowlist moves"
            );
            let saved: Value = serde_json::from_str(&shell.latest_state.lock().unwrap()).unwrap();
            assert_eq!(saved["preferences"]["ui_scale"], 120, "and saved");
        })
        .unwrap();
    let again: SettingsUpdateOutput = run(window, Command::SettingsUpdate(update()), cx).unwrap();
    assert!(again.changed.is_empty(), "idempotent");
    for bad in [
        SettingsUpdateInput {
            ui_scale: Some(125),
            ..SettingsUpdateInput::default()
        },
        SettingsUpdateInput {
            workspace_theme: Some("neon".into()),
            ..SettingsUpdateInput::default()
        },
        SettingsUpdateInput {
            accent_color: Some("blue".into()),
            ..SettingsUpdateInput::default()
        },
        SettingsUpdateInput {
            default_provider: Some("nope".into()),
            ..SettingsUpdateInput::default()
        },
    ] {
        let error =
            run::<SettingsUpdateOutput>(window, Command::SettingsUpdate(bad), cx).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
    approve(window, &[(CLIENT, &[Scope::Read, Scope::Manage])], cx);
    let denied = run::<SettingsValues>(window, Command::SettingsRead(SettingsReadInput {}), cx);
    assert_eq!(
        denied.unwrap_err().code,
        ErrorCode::ScopeDenied,
        "settings is its own scope"
    );
}

#[gpui::test]
fn search_finds_saved_messages_and_project_text(cx: &mut TestAppContext) {
    let (window, a, _) = two_sessions(
        &[Scope::Read, Scope::Navigate, Scope::Manage, Scope::Settings],
        cx,
    );
    window
        .update(cx, |shell, _, _| {
            let row = monocode_core::transcript::Row {
                role: "user".into(),
                text: "Why is the widget test flaky?".into(),
                first: true,
                sent_at: None,
            };
            locked(&shell.store)
                .unwrap()
                .write_rows(a, 1, &[row])
                .unwrap();
        })
        .unwrap();
    let search = |query: &str| {
        Command::SearchSessions(SearchSessionsInput {
            query: query.into(),
            ..SearchSessionsInput::default()
        })
    };
    let found: SearchSessionsOutput = run(window, search("widget test"), cx).unwrap();
    assert!(
        found
            .hits
            .iter()
            .any(|h| h.session_id == a && h.kind == HitKind::Message),
        "{found:?}"
    );
    let empty = run::<SearchSessionsOutput>(window, search("  "), cx);
    assert_eq!(empty.unwrap_err().code, ErrorCode::InvalidInput);
    let repo = repository("g7");
    window
        .update(cx, |shell, _, _| shell.workspace.add_project(&repo))
        .unwrap();
    let text: SearchProjectOutput = run(
        window,
        Command::SearchProject(SearchProjectInput {
            query: "hello".into(),
            project: Some(repo.clone()),
            ..SearchProjectInput::default()
        }),
        cx,
    )
    .unwrap();
    assert_eq!(text.matches.len(), 1, "{text:?}");
    assert_eq!(
        (text.matches[0].path.as_str(), text.matches[0].line),
        ("README.md", 1)
    );
    assert!(!text.timed_out);
}

#[gpui::test]
fn notes_are_written_listed_and_read_within_local_projects(cx: &mut TestAppContext) {
    let (window, _, _) = two_sessions(
        &[Scope::Read, Scope::Navigate, Scope::Manage, Scope::Settings],
        cx,
    );
    let write = |request: Option<&str>, input: NotesWriteInput, cx: &mut TestAppContext| {
        call(window, CLIENT, request, Command::NotesWrite(input), cx)
            .map(|v| serde_json::from_value::<NoteOutput>(v).unwrap())
    };
    let new = || NotesWriteInput {
        body: Some("# Plan\n\nShip phase 2".into()),
        project: Some(HERE.into()),
        ..NotesWriteInput::default()
    };
    let note = write(Some("note-1"), new(), cx).unwrap();
    assert_eq!(
        (note.title.as_str(), note.project.as_deref()),
        ("Plan", Some(HERE))
    );
    assert_eq!(
        write(Some("note-1"), new(), cx).unwrap().id,
        note.id,
        "a retry is the same note"
    );
    let renamed = write(
        None,
        NotesWriteInput {
            id: Some(note.id.clone()),
            title: Some("Phase 2".into()),
            ..NotesWriteInput::default()
        },
        cx,
    )
    .unwrap();
    assert_eq!(
        (renamed.title.as_str(), renamed.body.as_str()),
        ("Phase 2", "# Plan\n\nShip phase 2")
    );
    let list: NotesListOutput =
        run(window, Command::NotesList(NotesListInput::default()), cx).unwrap();
    assert_eq!(
        (list.total, list.notes[0].id.as_str()),
        (1, note.id.as_str())
    );
    let read: NoteOutput = run(
        window,
        Command::NotesRead(NotesReadInput {
            id: note.id.clone(),
        }),
        cx,
    )
    .unwrap();
    assert_eq!(read.title, "Phase 2");
    let missing = run::<NoteOutput>(
        window,
        Command::NotesRead(NotesReadInput { id: "nope".into() }),
        cx,
    );
    assert_eq!(missing.unwrap_err().code, ErrorCode::NotFound);
    let outside = write(
        None,
        NotesWriteInput {
            project: Some("/elsewhere".into()),
            ..new()
        },
        cx,
    );
    assert_eq!(outside.unwrap_err().code, ErrorCode::ProjectNotOpen);
}

#[gpui::test]
fn changes_read_summarizes_a_projects_working_tree(cx: &mut TestAppContext) {
    let (window, _, _) = two_sessions(
        &[Scope::Read, Scope::Navigate, Scope::Manage, Scope::Settings],
        cx,
    );
    let repo = repository("g8");
    std::fs::write(
        std::path::Path::new(&repo).join("README.md"),
        "hello\nworld\n",
    )
    .unwrap();
    window
        .update(cx, |shell, _, _| shell.workspace.add_project(&repo))
        .unwrap();
    let changes: ChangesReadOutput = run(
        window,
        Command::ChangesRead(ChangesReadInput {
            project: Some(repo.clone()),
            ..ChangesReadInput::default()
        }),
        cx,
    )
    .unwrap();
    assert_eq!(changes.files.len(), 1);
    assert_eq!(
        (changes.files[0].path.as_str(), changes.additions),
        ("README.md", 1)
    );
    assert_eq!(changes.commits[0].subject, "First");
    let both = run::<ChangesReadOutput>(
        window,
        Command::ChangesRead(ChangesReadInput {
            project: Some(repo),
            session_id: Some(1),
            limit: None,
        }),
        cx,
    );
    assert_eq!(both.unwrap_err().code, ErrorCode::InvalidInput);
}

/// A session's recorded turn, read as its review reads it.
#[test]
fn a_sessions_changes_are_its_recorded_turn() {
    let repo = repository("g9");
    let store = monocode_core::checkpoint::Store::new(scratch("g10").join("checkpoints"));
    store.begin_turn("7", &repo, "turn").unwrap();
    std::fs::write(std::path::Path::new(&repo).join("new.txt"), "one\ntwo\n").unwrap();
    store.finish_turn("7", &repo, "turn").unwrap();
    let read = exec::changes::session_changes(Some(&store), 7, &["7".into()], &repo, 100).unwrap();
    assert_eq!((read.session_id, read.files.len()), (Some(7), 1));
    assert_eq!(
        (read.files[0].path.as_str(), read.additions),
        ("new.txt", 2)
    );
    let none = exec::changes::session_changes(None, 7, &["7".into()], &repo, 100);
    assert_eq!(none.unwrap_err().code, ErrorCode::Internal);
}
