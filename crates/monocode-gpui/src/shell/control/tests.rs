//! The policy gate, approvals, scopes, receipts and the activity log.
use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    ClientInfo, Command, Scope, ops::project::ProjectListInput, transport::Connection,
};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

/// A short folder under the checkout's `target/c/`, short enough that
/// `<folder>/control/monocode-control.sock` fits `sun_path`.
pub(super) fn scratch(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/c");
    std::fs::create_dir_all(&root).unwrap();
    let dir = root.canonicalize().unwrap().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        cfg!(windows) || monocode_control::socket_path(&dir).is_ok(),
        "{dir:?} must fit sun_path"
    );
    dir
}

/// A git repository with one commit, inside the checkout.
pub(super) fn repository(name: &str) -> String {
    let dir = scratch(name).join("repo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("README.md"), "hello\n").unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "README.md"]);
    git(&["commit", "-q", "-m", "First"]);
    dir.display().to_string()
}

pub(super) fn setup(cx: &mut TestAppContext) -> WindowHandle<Shell> {
    let (window, _, _) = crate::shell::tests::window(cx);
    cx.run_until_parked();
    window
}

/// Turns control on with these approved clients, without a listener.
pub(super) fn approve(
    window: WindowHandle<Shell>,
    clients: &[(&str, &[Scope])],
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |shell, _, _| {
            shell.control.policy.enabled = true;
            shell.control.policy.clients = clients
                .iter()
                .map(|(name, scopes)| monocode_control::ApprovedClient {
                    name: (*name).into(),
                    scopes: scopes.to_vec(),
                    approved_at: 1,
                    last_seen: 1,
                })
                .collect();
        })
        .unwrap();
}

pub(super) fn call(
    window: WindowHandle<Shell>,
    client: &str,
    request_id: Option<&str>,
    command: Command,
    cx: &mut TestAppContext,
) -> Result<Value, ControlError> {
    let (reply, result) = mpsc::sync_channel(1);
    window
        .update(cx, |shell, window, cx| {
            shell.handle_control(
                Request::Call {
                    caller: client.into(),
                    request_id: request_id.map(str::to_owned),
                    command: Box::new(command),
                    reply,
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();
    result
        .try_recv()
        .expect("the call settles once its writes are done")
}

pub(super) fn hello(
    window: WindowHandle<Shell>,
    name: &str,
    cx: &mut TestAppContext,
) -> mpsc::Receiver<Result<Grant, ControlError>> {
    let (reply, grant) = mpsc::sync_channel(1);
    window
        .update(cx, |shell, window, cx| {
            shell.handle_control(
                Request::Hello {
                    client: ClientInfo {
                        name: name.into(),
                        version: "1".into(),
                    },
                    session: None,
                    reply,
                },
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();
    grant
}

/// The client most tests call as.
pub(super) const CLIENT: &str = "Claude Code";

/// Calls `command` as [`CLIENT`] and reads its output as `T`.
pub(super) fn run<T: serde::de::DeserializeOwned>(
    window: WindowHandle<Shell>,
    command: Command,
    cx: &mut TestAppContext,
) -> Result<T, ControlError> {
    call(window, CLIENT, None, command, cx).map(|v| serde_json::from_value(v).unwrap())
}

/// The fixture window's two sessions, oldest first.
pub(super) fn sessions(window: WindowHandle<Shell>, cx: &mut TestAppContext) -> (i64, i64) {
    window
        .update(cx, |shell, _, _| {
            let mut ids: Vec<_> = shell.session_summaries.keys().copied().collect();
            ids.sort_unstable();
            (ids[0], ids[1])
        })
        .unwrap()
}

/// The fixture window with control on and [`CLIENT`] holding `scopes`, and
/// its two sessions.
pub(super) fn two_sessions(
    scopes: &[Scope],
    cx: &mut TestAppContext,
) -> (WindowHandle<Shell>, i64, i64) {
    let window = setup(cx);
    approve(window, &[(CLIENT, scopes)], cx);
    let (a, b) = sessions(window, cx);
    (window, a, b)
}

/// Makes session `id` busy or idle as a turn does, saying so
/// (`SessionChanged`), and lets the window catch up.
pub(super) fn set_busy(window: WindowHandle<Shell>, id: i64, busy: bool, cx: &mut TestAppContext) {
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&id].update(cx, |view, cx| {
                view.busy = busy;
                cx.emit(crate::conversation::SessionChanged);
                cx.notify();
            });
        })
        .unwrap();
    cx.run_until_parked();
}

fn projects() -> Command {
    Command::ProjectList(ProjectListInput::default())
}

fn stored(window: WindowHandle<Shell>, key: &str, cx: &mut TestAppContext) -> Option<String> {
    window
        .update(cx, |shell, _, _| {
            locked(&shell.store).unwrap().setting(key).unwrap()
        })
        .unwrap()
}

#[gpui::test]
fn a_disabled_policy_still_listens_but_refuses_every_external_client(cx: &mut TestAppContext) {
    let window = setup(cx);
    let endpoint = Endpoint::for_data_dir(&scratch("g1")).unwrap();
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.control.policy.enabled, "control is off by default");
            shell.control.endpoint = Some(Ok(endpoint.clone()));
            shell.apply_control(cx);
            assert!(shell.control.server.is_some(), "the app's sessions call in");
            assert_eq!(shell.control_status().url, None, "no MCP server while off");
        })
        .unwrap();
    assert!(Connection::connect(&endpoint).is_ok(), "the app listens");
    let refused = hello(window, "Claude Code", cx).try_recv().unwrap();
    assert_eq!(refused.unwrap_err().code, ErrorCode::ControlDisabled);
    let refused = call(window, "Claude Code", None, projects(), cx);
    assert_eq!(refused.unwrap_err().code, ErrorCode::ControlDisabled);

    window
        .update(cx, |shell, _, cx| {
            shell.edit_control(&ExternalControlEdit::Enabled(true), cx);
            assert!(shell.control.server.is_some());
            let url = shell.control_status().url.unwrap();
            assert!(
                url.starts_with("http://127.0.0.1:") && url.ends_with("/mcp"),
                "{url}"
            );
        })
        .unwrap();
    cx.run_until_parked();
    let saved = Policy::parse(stored(window, POLICY_KEY, cx).as_deref());
    assert!(saved.enabled, "the switch is kept in the store");

    window
        .update(cx, |shell, _, cx| {
            shell.edit_control(&ExternalControlEdit::Enabled(false), cx);
            assert!(shell.control.server.is_some(), "off keeps the socket");
        })
        .unwrap();
    cx.run_until_parked();
    assert!(
        Connection::connect(&endpoint).is_ok(),
        "off: still listening"
    );
    let refused = hello(window, "Claude Code", cx).try_recv().unwrap();
    assert_eq!(refused.unwrap_err().code, ErrorCode::ControlDisabled);
    assert!(!Policy::parse(stored(window, POLICY_KEY, cx).as_deref()).enabled);
}

#[gpui::test]
fn an_unknown_client_is_asked_once_and_then_remembered(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[], cx);
    let first = hello(window, "Claude Code", cx);
    let second = hello(window, "Claude Code", cx);
    assert!(cx.has_pending_prompt(), "the person is asked in the window");
    assert!(
        cx.pending_prompt()
            .unwrap()
            .0
            .contains("Allow “Claude Code” to control MonoCode?")
    );
    assert!(
        first.try_recv().is_err(),
        "nothing is granted before the answer"
    );
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    for waiting in [first, second] {
        let grant = waiting.try_recv().unwrap().unwrap();
        assert_eq!(grant.scopes, Scope::DEFAULT_GRANT);
    }
    let again = hello(window, "Claude Code", cx);
    assert!(
        !cx.has_pending_prompt(),
        "a known client is not asked again"
    );
    assert_eq!(
        again.try_recv().unwrap().unwrap().scopes,
        Scope::DEFAULT_GRANT
    );
    let saved = Policy::parse(stored(window, POLICY_KEY, cx).as_deref());
    assert_eq!(
        saved.client("Claude Code").unwrap().scopes,
        Scope::DEFAULT_GRANT
    );
}

#[gpui::test]
fn deny_or_no_answer_in_two_minutes_declines(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[], cx);
    let denied = hello(window, "Codex", cx);
    cx.simulate_prompt_answer("Deny");
    cx.run_until_parked();
    assert_eq!(
        denied.try_recv().unwrap().unwrap_err().code,
        ErrorCode::UserDeclined
    );

    // Two minutes in the app; one simulated second here, because every
    // periodic timer of the fixture window runs while the clock advances.
    let timeout = window
        .update(cx, |shell, _, _| {
            assert_eq!(shell.control.timeouts.approval, Duration::from_secs(120));
            shell.control.timeouts.approval = Duration::from_secs(1);
            shell.control.timeouts.approval
        })
        .unwrap();
    // Another client: a denied one is not asked again for a while.
    let silent = hello(window, "Cursor", cx);
    assert!(cx.has_pending_prompt());
    cx.executor().advance_clock(timeout / 2);
    cx.run_until_parked();
    assert!(
        silent.try_recv().is_err(),
        "still waiting before the timeout"
    );
    cx.executor().advance_clock(timeout);
    cx.run_until_parked();
    assert_eq!(
        silent.try_recv().unwrap().unwrap_err().code,
        ErrorCode::UserDeclined
    );
    // A late answer to the stale prompt approves nothing.
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    window
        .update(cx, |shell, _, _| {
            assert!(shell.control.policy.client("Codex").is_none());
            assert!(shell.control.policy.client("Cursor").is_none());
            let decisions: Vec<_> = shell
                .control
                .activity
                .entries()
                .map(|e| e.operation.as_str())
                .collect();
            assert_eq!(decisions, ["decline", "decline"]);
        })
        .unwrap();
}

#[gpui::test]
fn scopes_are_checked_on_every_call(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[("Navigator", &[Scope::Navigate])], cx);
    let denied = call(window, "Navigator", None, projects(), cx).unwrap_err();
    assert_eq!(denied.code, ErrorCode::ScopeDenied);
    assert!(denied.message.contains("read scope"), "{}", denied.message);
    let stranger = call(window, "Stranger", None, projects(), cx).unwrap_err();
    assert_eq!(stranger.code, ErrorCode::ScopeDenied);
    window
        .update(cx, |shell, _, cx| {
            shell.edit_control(
                &ExternalControlEdit::Scope {
                    client: "Navigator".into(),
                    scope: Scope::Read,
                    on: true,
                },
                cx,
            );
        })
        .unwrap();
    assert!(
        call(window, "Navigator", None, projects(), cx).is_ok(),
        "granted live"
    );
    window
        .update(cx, |shell, _, cx| {
            shell.edit_control(&ExternalControlEdit::Revoke("Navigator".into()), cx);
        })
        .unwrap();
    let revoked = call(window, "Navigator", None, projects(), cx).unwrap_err();
    assert_eq!(revoked.code, ErrorCode::ScopeDenied);
}

#[gpui::test]
fn request_ids_replay_the_first_result_and_conflict_on_other_input(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[("Claude Code", &[Scope::Read])], cx);
    let first = call(window, "Claude Code", Some("list-1"), projects(), cx).unwrap();
    assert_eq!(first["total"], 2);
    let key = receipts::key("Claude Code", "list-1");
    assert!(key.starts_with("control-receipt:"));
    assert!(stored(window, &key, cx).is_some(), "the receipt is durable");
    window
        .update(cx, |shell, _, _| shell.workspace.add_project("/work/third"))
        .unwrap();
    let replay = call(window, "Claude Code", Some("list-1"), projects(), cx).unwrap();
    assert_eq!(replay, first, "a retry gets the original result");
    let fresh = call(window, "Claude Code", None, projects(), cx).unwrap();
    assert_eq!(
        fresh["total"], 3,
        "without a request ID the call runs again"
    );
    let other = Command::ProjectList(ProjectListInput {
        cursor: None,
        limit: Some(1),
    });
    let conflict = call(window, "Claude Code", Some("list-1"), other, cx).unwrap_err();
    assert_eq!(conflict.code, ErrorCode::RequestConflict);
    // Receipts are per client: another client may use the same ID.
    approve(
        window,
        &[("Claude Code", &[Scope::Read]), ("Codex", &[Scope::Read])],
        cx,
    );
    let theirs = call(window, "Codex", Some("list-1"), projects(), cx).unwrap();
    assert_eq!(theirs["total"], 3);
}

#[gpui::test]
fn a_refusal_keeps_no_receipt(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[("Claude Code", &[])], cx);
    let refused = call(window, "Claude Code", Some("later"), projects(), cx);
    assert_eq!(refused.unwrap_err().code, ErrorCode::ScopeDenied);
    approve(window, &[("Claude Code", &[Scope::Read])], cx);
    assert!(call(window, "Claude Code", Some("later"), projects(), cx).is_ok());
    window
        .update(cx, |shell, _, _| {
            let log: Vec<_> = shell
                .control
                .activity
                .entries()
                .map(|e| (e.operation.as_str(), e.outcome.as_str()))
                .collect();
            assert!(log.is_empty(), "reads are not logged: {log:?}");
        })
        .unwrap();
}

#[gpui::test]
fn settings_lists_activity_newest_first_and_clear_forgets_it(cx: &mut TestAppContext) {
    let window = setup(cx);
    approve(window, &[("Claude Code", &[Scope::Read])], cx);
    let stop = Command::SessionStop(monocode_control::ops::session::SessionInput { session_id: 7 });
    let rename = Command::SessionRename(monocode_control::ops::organize::SessionRenameInput {
        session_id: 7,
        title: "Parser fix".into(),
    });
    let _ = call(window, "Claude Code", None, projects(), cx);
    let _ = call(window, "Claude Code", None, rename, cx);
    let _ = call(window, "Claude Code", None, stop, cx);
    window
        .update(cx, |shell, _, cx| {
            let rows = shell.control_status().activity;
            let shown: Vec<_> = rows
                .iter()
                .map(|r| {
                    (
                        r.operation.as_str(),
                        r.target.as_deref(),
                        r.outcome.as_str(),
                    )
                })
                .collect();
            assert_eq!(
                shown,
                [
                    ("session_stop", Some("session 7"), "scope_denied"),
                    ("session_rename", Some("session 7"), "scope_denied"),
                ],
                "reads are not listed"
            );
            shell.edit_control(&ExternalControlEdit::ClearActivity, cx);
            assert!(shell.control_status().activity.is_empty());
            assert!(
                shell.control.policy.enabled,
                "clearing leaves the policy alone"
            );
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(stored(window, ACTIVITY_KEY, cx).as_deref(), Some("[]"));
}
