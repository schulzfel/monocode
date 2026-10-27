//! What `git_discard` and `git_push` may touch. A discard names only files
//! the repository reports as unstaged changes, inside the repository once
//! links are followed, and its prompt says which are reverted and which are
//! deleted. A push publishes the branch and commit the person was shown, or
//! nothing. Repositories and remotes live under the checkout's `target/c/`.
use super::tests::CLIENT;
use super::tests_vcs::{git, local_repo, with_repo};
use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    Command, Scope,
    ops::{organize::ProjectInput, vcs::GitPathsInput},
};
use std::{path::Path, sync::mpsc::Receiver};

type Answer = Receiver<Result<Value, ControlError>>;

fn ask(window: WindowHandle<Shell>, command: Command, cx: &mut TestAppContext) -> Answer {
    let (reply, answer) = std::sync::mpsc::sync_channel(1);
    window
        .update(cx, |shell, window, cx| {
            let request = Request::Call {
                caller: CLIENT.into(),
                request_id: None,
                command: Box::new(command),
                reply,
            };
            shell.handle_control(request, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    answer
}

fn discard(repo: &str, paths: &[&str]) -> Command {
    Command::GitDiscard(GitPathsInput {
        project: Some(repo.to_owned()),
        paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        all: None,
    })
}

fn answered(answer: &Answer, button: &str, cx: &mut TestAppContext) -> Result<Value, ControlError> {
    cx.simulate_prompt_answer(button);
    cx.run_until_parked();
    answer.try_recv().expect("the call has settled")
}

/// A repository with unsaved work in README.md, an untracked new.txt, an
/// ignored debug.log, and (on Unix) `link`, an untracked link to a folder
/// beside the repository.
fn messy(name: &str) -> String {
    let repo = local_repo(name);
    let root = Path::new(&repo);
    std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    git(&repo, &["add", ".gitignore"]);
    git(&repo, &["commit", "-q", "-m", "Ignore logs"]);
    std::fs::write(root.join("README.md"), "unsaved work\n").unwrap();
    std::fs::write(root.join("new.txt"), "new\n").unwrap();
    std::fs::write(root.join("debug.log"), "keep\n").unwrap();
    let elsewhere = root.parent().unwrap().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("keep.txt"), "outside\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&elsewhere, root.join("link")).unwrap();
    repo
}

#[gpui::test]
fn discard_refuses_anything_that_is_not_an_unstaged_change(cx: &mut TestAppContext) {
    let repo = messy("x2");
    let window = with_repo(&repo, &[Scope::Read, Scope::Vcs], cx);
    let mut refused = vec![
        vec!["debug.log"],
        vec!["../elsewhere/keep.txt"],
        vec!["README.md", "missing.txt"],
        vec![".gitignore"],
    ];
    if cfg!(unix) {
        refused.push(vec!["link/keep.txt"]);
    }
    for paths in refused {
        let answer = ask(window, discard(&repo, &paths), cx);
        assert!(
            !cx.has_pending_prompt(),
            "{paths:?} is refused before asking"
        );
        let error = answer.try_recv().unwrap().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{paths:?}");
        assert!(error.message.contains("unstaged"), "{}", error.message);
    }
    let root = Path::new(&repo);
    let elsewhere = root.parent().unwrap().join("elsewhere/keep.txt");
    assert!(
        elsewhere.is_file(),
        "nothing outside the repository was touched"
    );
    assert!(root.join("debug.log").is_file() && root.join("new.txt").is_file());
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "unsaved work\n"
    );
}

#[gpui::test]
fn discard_says_which_files_are_reverted_and_which_are_deleted(cx: &mut TestAppContext) {
    let repo = messy("x3");
    let window = with_repo(&repo, &[Scope::Read, Scope::Vcs], cx);
    let root = Path::new(&repo);
    let answer = ask(window, discard(&repo, &["README.md", "new.txt"]), cx);
    let (message, detail) = cx.pending_prompt().expect("the person is asked");
    assert!(
        message.contains("revert 1 file and delete 1 untracked file"),
        "{message}"
    );
    let (reverted, deleted) = detail.split_once("Deleted").expect("two lists");
    assert!(
        reverted.contains("README.md") && !reverted.contains("new.txt"),
        "{detail}"
    );
    assert!(
        deleted.contains("new.txt") && !deleted.contains("README.md"),
        "{detail}"
    );
    answered(&answer, "Discard", cx).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "hello\n"
    );
    assert!(!root.join("new.txt").exists());
    assert!(
        root.join("debug.log").is_file(),
        "ignored files are never discarded"
    );

    // Everything: the untracked link goes, never what it points to.
    std::fs::write(root.join("README.md"), "more work\n").unwrap();
    let all = Command::GitDiscard(GitPathsInput {
        project: Some(repo.clone()),
        all: Some(true),
        ..GitPathsInput::default()
    });
    let answer = ask(window, all, cx);
    let (message, _) = cx.pending_prompt().expect("the person is asked");
    assert!(message.contains("revert 1 file"), "{message}");
    if cfg!(unix) {
        assert!(message.contains("delete 1 untracked file"), "{message}");
    }
    answered(&answer, "Discard", cx).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "hello\n"
    );
    assert!(!root.join("link").exists());
    assert!(root.parent().unwrap().join("elsewhere/keep.txt").is_file());
    assert!(root.join("debug.log").is_file());

    // A change that went away while the person decided is not touched.
    std::fs::write(root.join("README.md"), "even more\n").unwrap();
    let answer = ask(window, discard(&repo, &["README.md"]), cx);
    assert!(cx.has_pending_prompt());
    git(&repo, &["add", "README.md"]);
    let stale = answered(&answer, "Discard", cx).unwrap_err();
    assert_eq!(
        (stale.code, stale.retryable),
        (ErrorCode::RequestConflict, true)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "even more\n"
    );
}

/// A repository with a bare remote beside it.
fn pushable(name: &str) -> (String, String) {
    let repo = local_repo(name);
    let remote = Path::new(&repo).parent().unwrap().join("remote.git");
    let remote = remote.to_str().unwrap().to_owned();
    git(&repo, &["init", "-q", "--bare", &remote]);
    git(&repo, &["remote", "add", "origin", &remote]);
    (repo, remote)
}

fn heads(remote: &str) -> String {
    git(
        remote,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
        ],
    )
}

#[gpui::test]
fn a_push_publishes_the_branch_and_commit_the_person_saw(cx: &mut TestAppContext) {
    let (repo, remote) = pushable("x4");
    let window = with_repo(&repo, &[Scope::Read, Scope::Vcs], cx);
    let push = || {
        Command::GitPush(ProjectInput {
            project: Some(repo.clone()),
        })
    };
    // A commit lands while the person decides.
    let answer = ask(window, push(), cx);
    assert!(cx.has_pending_prompt());
    std::fs::write(Path::new(&repo).join("late.txt"), "late\n").unwrap();
    git(&repo, &["add", "late.txt"]);
    git(&repo, &["commit", "-q", "-m", "Late"]);
    let moved = answered(&answer, "Push", cx).unwrap_err();
    assert_eq!(
        (moved.code, moved.retryable),
        (ErrorCode::RequestConflict, true)
    );
    assert!(
        moved.message.contains("nothing was pushed"),
        "{}",
        moved.message
    );
    assert_eq!(heads(&remote), "", "nothing was pushed");
    // Another branch is checked out while the person decides.
    let answer = ask(window, push(), cx);
    git(&repo, &["checkout", "-q", "-b", "other"]);
    let switched = answered(&answer, "Push", cx).unwrap_err();
    assert_eq!(switched.code, ErrorCode::RequestConflict);
    assert_eq!(heads(&remote), "");
    git(&repo, &["checkout", "-q", "main"]);
    // Unchanged: exactly what was shown is published, and tracked.
    let answer = ask(window, push(), cx);
    let (message, _) = cx.pending_prompt().unwrap();
    assert!(message.contains("“main”"), "{message}");
    let pushed = answered(&answer, "Push", cx).unwrap();
    assert_eq!(pushed["branch"], "main");
    let head = git(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(heads(&remote), format!("refs/heads/main {head}"));
    assert_eq!(
        git(&repo, &["rev-parse", "--abbrev-ref", "main@{upstream}"]),
        "origin/main"
    );
}
