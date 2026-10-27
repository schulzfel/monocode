//! An agent session's old Operator actions on worktrees, folders, sends,
//! stops, archives and deletes, ported as `tests_operator` ports the rest:
//! the old tests' assertions, through the control pipeline.
use super::tests_operator::{call, launch, setup};
use super::*;
use gpui::TestAppContext;
use monocode_core::{agent::Settings, store::Store};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[gpui::test]
fn operator_archive_delete_and_folder_actions_settle_after_persistence(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let archived = launch(window, caller, "archive-child", json!({}), cx);
    let deleted = launch(window, caller, "delete-child", json!({"reveal":true}), cx);
    let folder = call(
        window,
        caller,
        "folder",
        "folders.move",
        json!({"sessionId":deleted,"newFolderName":"Review tasks"}),
        cx,
    )
    .unwrap();
    assert!(folder["folderId"].is_string());
    assert_eq!(
        call(
            window,
            caller,
            "archive",
            "sessions.archive",
            json!({"sessionId":archived}),
            cx
        )
        .unwrap()["archived"],
        true
    );
    // shape: C-8 (`deleted: true` became the deleted session's ID and title)
    assert_eq!(
        call(
            window,
            caller,
            "delete",
            "sessions.delete",
            json!({"sessionId":deleted}),
            cx
        )
        .unwrap()["sessionId"],
        deleted
    );
    window
        .update(cx, |shell, _, _| {
            assert!(shell.session_summaries[&archived].archived);
            assert!(!shell.session_summaries.contains_key(&deleted));
            assert!(shell.workspace.tab_of(deleted).is_none());
            assert!(
                shell
                    .groups
                    .values()
                    .all(|g| g.folder_of(deleted).is_none())
            );
            let db = locked(&shell.store).unwrap();
            assert!(
                db.sessions()
                    .unwrap()
                    .iter()
                    .find(|s| s.id == archived)
                    .unwrap()
                    .archived
            );
            assert!(!db.sessions().unwrap().iter().any(|s| s.id == deleted));
        })
        .unwrap();
    assert!(
        call(
            window,
            caller,
            "self-delete",
            "sessions.delete",
            json!({"sessionId":caller}),
            cx
        )
        .is_err()
    );
}

#[gpui::test]
fn operator_creates_selects_and_validates_real_worktrees(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join(format!(
        "operator-worktrees-{}",
        monocode_core::mono::new_id()
    ));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&project)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    std::fs::write(project.join("README.md"), "Fixture\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Operator Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-qm",
        "Fixture",
    ]);
    let project = project.to_string_lossy().into_owned();
    let store = Arc::new(Mutex::new(Store::open(&root.join("native.db")).unwrap()));
    let caller = locked(&store).unwrap().create("codex", &project).unwrap();
    let id = caller.id;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Shell::new(store, vec![caller], project.clone(), None, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            shell.ensure_session(id, cx).unwrap().update(cx, |v, cx| {
                v.install_transfer(
                    Settings {
                        operator_enabled: true,
                        ..Default::default()
                    },
                    None,
                    cx,
                );
            })
        })
        .unwrap();
    cx.run_until_parked();
    let tree = call(
        window,
        id,
        "create-tree",
        "worktrees.create",
        json!({"branch":"operator-review","base":"HEAD"}),
        cx,
    )
    .unwrap();
    // shape: C-8 (the worktree's `path` moved under `worktree`)
    let path = tree["worktree"]["path"].as_str().unwrap();
    assert!(std::path::Path::new(path).join("README.md").is_file());
    let listed = call(window, id, "list-trees", "worktrees.list", json!({}), cx).unwrap();
    assert!(
        listed["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["path"] == path)
    );
    let selected = launch(window, id, "select-tree", json!({"worktreeCwd":path}), cx);
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(
                shell.sessions[&selected]
                    .read(cx)
                    .settings
                    .worktree_cwd
                    .as_deref(),
                Some(path)
            )
        })
        .unwrap();
    let fresh = launch(
        window,
        id,
        "new-tree",
        json!({"workspaceMode":"worktree","worktreeBase":"HEAD"}),
        cx,
    );
    window
        .update(cx, |shell, _, cx| {
            let path = shell.sessions[&fresh]
                .read(cx)
                .settings
                .worktree_cwd
                .as_deref()
                .unwrap();
            assert_ne!(path, project);
            assert!(std::path::Path::new(path).join("README.md").is_file());
        })
        .unwrap();
    let before = window
        .update(cx, |shell, _, _| {
            locked(&shell.store).unwrap().sessions().unwrap().len()
        })
        .unwrap();
    assert!(
        call(
            window,
            id,
            "invalid-checkout",
            "sessions.start",
            json!({"prompt":"Must not launch","draft":true,"worktreeCwd":"/unrelated-checkout"}),
            cx
        )
        .is_err()
    );
    window
        .update(cx, |shell, _, _| {
            assert_eq!(
                locked(&shell.store).unwrap().sessions().unwrap().len(),
                before
            )
        })
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[gpui::test]
fn app_launch_submission_uses_the_saved_message_identity(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let child = launch(window, caller, "queued-seed", json!({}), cx);
    let expected = window
        .update(cx, |shell, _, cx| {
            let view = shell.sessions[&child].clone();
            let expected = locked(&shell.store)
                .unwrap()
                .follow_ups(child)
                .unwrap()
                .draft
                .unwrap()
                .id;
            view.update(cx, |v, cx| {
                // Hold the provider boundary: verify queue acceptance without starting
                // a real CLI or making an inference request.
                v.busy = true;
                v.install_app_launch(v.settings.clone(), true, cx);
            });
            expected
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            let state = locked(&shell.store).unwrap().follow_ups(child).unwrap();
            assert!(state.draft.is_none());
            assert_eq!(state.messages.len(), 1);
            assert_eq!(state.messages[0].id, expected);
            assert_eq!(state.messages[0].text, "Review this project");
            shell.sessions[&child].update(cx, |v, _| v.busy = false);
        })
        .unwrap();
}

#[gpui::test]
fn operator_send_and_draft_refuse_a_target_with_composer_text_or_a_saved_draft(
    cx: &mut TestAppContext,
) {
    let (window, _, caller) = setup(cx);
    let saved = launch(window, caller, "saved-draft", json!({}), cx);
    window
        .update(cx, |shell, _, cx| shell.create(Kind::Codex, cx))
        .unwrap();
    cx.run_until_parked();
    let typed = window
        .update(cx, |shell, _, cx| {
            let typed = shell.workspace.active.unwrap();
            assert!(typed != caller && typed != saved);
            shell.sessions[&typed].update(cx, |v, cx| {
                v.input
                    .update(cx, |i, cx| i.set_text("Keep my words".into(), cx))
            });
            typed
        })
        .unwrap();
    // C-1 (OP-45): a send keeps Operator's words; a draft is refused in
    // `session_draft`'s, which call a saved draft and the person's unsent
    // text alike an unsent draft.
    const BUSY: &str = "Session is busy, loading, or has an unsent draft. Its draft was preserved.";
    for (n, (action, target, refusal)) in [
        ("sessions.send", saved, BUSY.to_string()),
        (
            "sessions.draft",
            saved,
            format!("Session {saved} already has an unsent draft; it was kept."),
        ),
        ("sessions.send", typed, BUSY.to_string()),
        (
            "sessions.draft",
            typed,
            format!("Session {typed} already has an unsent draft; it was kept."),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            call(
                window,
                caller,
                &format!("refused-{n}"),
                action,
                json!({"sessionId":target,"prompt":"Replace it"}),
                cx
            ),
            Err(refusal),
            "{action} {target}"
        );
    }
    window
        .update(cx, |shell, _, cx| {
            let db = locked(&shell.store).unwrap();
            assert_eq!(
                db.follow_ups(saved).unwrap().draft.unwrap().text,
                "Review this project"
            );
            assert!(db.follow_ups(typed).unwrap().draft.is_none());
            assert_eq!(
                shell.sessions[&typed].read(cx).input.read(cx).content,
                "Keep my words"
            );
            assert!(!shell.sessions[&saved].read(cx).busy);
            assert!(!shell.sessions[&typed].read(cx).busy);
        })
        .unwrap();
}

#[gpui::test]
fn operator_stop_pauses_the_target_queue_and_keeps_its_messages(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let child = launch(window, caller, "queued-stop", json!({}), cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&child].update(cx, |v, cx| {
                // Hold the provider boundary while the saved draft joins the queue.
                v.busy = true;
                v.install_app_launch(v.settings.clone(), true, cx);
            })
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&child].update(cx, |v, _| v.busy = false);
            let (messages, paused, _) = shell.sessions[&child].read(cx).queued();
            assert_eq!(messages.len(), 1);
            assert!(!paused);
        })
        .unwrap();
    // shape: C-8 (`stopped: true` became the stopped session's ID, whether
    // a turn ran, and its status)
    assert_eq!(
        call(
            window,
            caller,
            "stop",
            "sessions.stop",
            json!({"sessionId":child}),
            cx
        )
        .unwrap()["sessionId"],
        child
    );
    window
        .update(cx, |shell, _, cx| {
            let view = shell.sessions[&child].read(cx);
            let (messages, paused, _) = view.queued();
            assert!(paused);
            assert_eq!(messages.len(), 1);
            assert!(!view.busy);
            let saved = locked(&shell.store).unwrap().follow_ups(child).unwrap();
            assert_eq!(saved.messages.len(), 1);
            assert_eq!(saved.messages[0].text, "Review this project");
        })
        .unwrap();
}

#[gpui::test]
fn operator_archive_stops_a_busy_session_and_closes_its_pane(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let child = launch(window, caller, "busy-archive", json!({"reveal":true}), cx);
    window
        .update(cx, |shell, _, cx| {
            assert!(shell.workspace.tab_of(child).is_some());
            shell.sessions[&child].update(cx, |v, _| v.busy = true);
        })
        .unwrap();
    assert_eq!(
        call(
            window,
            caller,
            "archive",
            "sessions.archive",
            json!({"sessionId":child}),
            cx
        )
        .unwrap()["archived"],
        true
    );
    window
        .update(cx, |shell, _, cx| {
            assert!(shell.workspace.tab_of(child).is_none());
            assert!(shell.session_summaries[&child].archived);
            assert!(shell.sessions.get(&child).is_none_or(|v| !v.read(cx).busy));
            let db = locked(&shell.store).unwrap();
            assert!(
                db.sessions()
                    .unwrap()
                    .iter()
                    .find(|s| s.id == child)
                    .unwrap()
                    .archived
            );
        })
        .unwrap();
}
