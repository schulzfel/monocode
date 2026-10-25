//! The `vcs` scope and `git_stage`, `git_unstage` and `git_commit` on
//! throwaway repositories under the checkout's `target/c/`. Discard and push
//! wait for the person; `tests_confirm` covers them.
use super::tests::{CLIENT, approve, call, repository, run, setup};
use super::*;
use gpui::TestAppContext;
use monocode_control::{
    Command, Policy, Scope,
    ops::{
        changes::ChangesReadOutput,
        vcs::{GitCommitInput, GitCommitOutput, GitPathsInput},
    },
};
use std::path::Path;

/// Runs git in `dir` and returns its trimmed output.
pub(super) fn git(dir: &str, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A repository with one commit whose own settings make commits and pushes
/// independent of the machine's git config: an author, no signing, no hooks.
pub(super) fn local_repo(name: &str) -> String {
    let repo = repository(name);
    let hooks = Path::new(&repo).parent().unwrap().join("no-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for (key, value) in [
        ("user.name", "Test"),
        ("user.email", "test@example.com"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", hooks.to_str().unwrap()),
    ] {
        git(&repo, &["config", key, value]);
    }
    repo
}

/// A window whose client holds `scopes`, with `repo` as a project.
pub(super) fn with_repo(
    repo: &str,
    scopes: &[Scope],
    cx: &mut TestAppContext,
) -> gpui::WindowHandle<Shell> {
    let window = setup(cx);
    approve(window, &[(CLIENT, scopes)], cx);
    window
        .update(cx, |shell, _, _| shell.workspace.add_project(repo))
        .unwrap();
    window
}

fn paths(repo: &str, paths: &[&str]) -> GitPathsInput {
    GitPathsInput {
        project: Some(repo.to_owned()),
        paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        all: None,
    }
}

fn all(repo: &str) -> GitPathsInput {
    GitPathsInput {
        project: Some(repo.to_owned()),
        all: Some(true),
        ..GitPathsInput::default()
    }
}

fn staged(changes: &ChangesReadOutput) -> Vec<String> {
    let mut staged: Vec<_> = changes
        .files
        .iter()
        .filter(|f| f.staged == Some(true))
        .map(|f| f.path.clone())
        .collect();
    staged.sort();
    staged
}

#[test]
fn vcs_is_off_for_new_clients_and_old_policies_stay_valid() {
    let mut policy = Policy::default();
    policy.approve("Claude Code", 1);
    assert!(!policy.grant("Claude Code").unwrap().allows(Scope::Vcs));
    // A policy stored before the vcs scope existed reads as it was; the
    // permission ceiling it kept then is ignored.
    let old = r#"{"enabled":true,"ceiling":"auto","clients":[{"name":"Codex","scopes":["read","navigate","dispatch","manage","settings","destructive"],"approved_at":1,"last_seen":2}]}"#;
    let parsed = Policy::parse(Some(old));
    assert!(parsed.enabled);
    assert!(!serde_json::to_string(&parsed).unwrap().contains("ceiling"));
    let codex = parsed.client("Codex").unwrap();
    assert_eq!(codex.scopes.len(), 6);
    assert!(!codex.scopes.contains(&Scope::Vcs));
    // Turning it on keeps it, in scope order, and survives a round trip.
    let mut turned = parsed.clone();
    assert!(turned.set_scope("Codex", Scope::Vcs, true));
    let scopes = &turned.client("Codex").unwrap().scopes;
    assert_eq!(scopes[5], Scope::Vcs);
    assert_eq!(scopes[6], Scope::Destructive);
    let round = Policy::parse(Some(&serde_json::to_string(&turned).unwrap()));
    assert_eq!(round, turned);
}

#[gpui::test]
fn git_needs_the_vcs_scope(cx: &mut TestAppContext) {
    let repo = local_repo("q1");
    let window = with_repo(&repo, &Scope::DEFAULT_GRANT, cx);
    std::fs::write(Path::new(&repo).join("new.txt"), "new\n").unwrap();
    for command in [
        Command::GitStage(all(&repo)),
        Command::GitCommit(GitCommitInput {
            project: Some(repo.clone()),
            message: "Nope".into(),
        }),
    ] {
        let refused = call(window, CLIENT, None, command, cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::ScopeDenied);
        assert!(refused.message.contains("vcs"), "{}", refused.message);
    }
    assert_eq!(git(&repo, &["status", "--porcelain"]), "?? new.txt");
}

#[gpui::test]
fn stage_unstage_and_commit_change_only_the_index_and_head(cx: &mut TestAppContext) {
    let repo = local_repo("q2");
    let window = with_repo(&repo, &[Scope::Read, Scope::Vcs], cx);
    std::fs::write(Path::new(&repo).join("a.txt"), "a\n").unwrap();
    std::fs::write(Path::new(&repo).join("README.md"), "hello again\n").unwrap();

    let after: ChangesReadOutput =
        run(window, Command::GitStage(paths(&repo, &["a.txt"])), cx).unwrap();
    assert_eq!(after.root, repo);
    assert_eq!(staged(&after), ["a.txt"]);
    assert_eq!(after.files.len(), 2, "README.md stays unstaged");

    let after: ChangesReadOutput = run(window, Command::GitUnstage(all(&repo)), cx).unwrap();
    assert!(staged(&after).is_empty());
    assert_eq!(git(&repo, &["diff", "--cached", "--name-only"]), "");

    let nothing = run::<GitCommitOutput>(
        window,
        Command::GitCommit(GitCommitInput {
            project: Some(repo.clone()),
            message: "Empty".into(),
        }),
        cx,
    );
    assert_eq!(nothing.unwrap_err().code, ErrorCode::InvalidInput);

    let after: ChangesReadOutput = run(window, Command::GitStage(all(&repo)), cx).unwrap();
    assert_eq!(staged(&after), ["README.md", "a.txt"]);
    let committed: GitCommitOutput = run(
        window,
        Command::GitCommit(GitCommitInput {
            project: Some(repo.clone()),
            message: "Add a and update the readme".into(),
        }),
        cx,
    )
    .unwrap();
    assert_eq!(committed.commit.subject, "Add a and update the readme");
    assert_eq!(committed.branch.as_deref(), Some("main"));
    assert_eq!(
        git(&repo, &["log", "-1", "--format=%h"]),
        committed.commit.sha
    );
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), "2");
}

#[gpui::test]
fn git_input_is_checked_before_anything_runs(cx: &mut TestAppContext) {
    let repo = local_repo("q3");
    let window = with_repo(&repo, &[Scope::Read, Scope::Vcs], cx);
    std::fs::write(Path::new(&repo).join("a.txt"), "a\n").unwrap();
    let both = GitPathsInput {
        all: Some(true),
        ..paths(&repo, &["a.txt"])
    };
    let none = GitPathsInput {
        project: Some(repo.clone()),
        ..GitPathsInput::default()
    };
    let outside = paths(&repo, &["../escape.txt"]);
    for input in [both, none, outside] {
        let refused = run::<ChangesReadOutput>(window, Command::GitStage(input), cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidInput, "{}", refused.message);
    }
    let blank = run::<GitCommitOutput>(
        window,
        Command::GitCommit(GitCommitInput {
            project: Some(repo.clone()),
            message: "  ".into(),
        }),
        cx,
    );
    assert_eq!(blank.unwrap_err().code, ErrorCode::InvalidInput);
    let elsewhere = run::<ChangesReadOutput>(
        window,
        Command::GitStage(paths("/work/nowhere", &["a.txt"])),
        cx,
    );
    assert_eq!(elsewhere.unwrap_err().code, ErrorCode::ProjectNotOpen);
    assert_eq!(git(&repo, &["status", "--porcelain"]), "?? a.txt");
}
