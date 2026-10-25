//! Publishing exactly the commit the person was shown: `git_push` from an
//! external control client pushes a named branch at a named commit, so a
//! checkout or commit made while the person decided is never published.
use super::{expand_home, git_checked, git_remote_name, git_stdout, remote_workspace};
use std::path::Path;

/// Pushes commit `head` as branch `branch` to the branch's upstream, or to
/// the default remote (setting it as the upstream) when it has none. Fails
/// without pushing when `branch` no longer points at `head`.
pub fn push_branch(cwd: &Path, branch: &str, head: &str) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return Err("Pushing a pinned branch is not available on remote projects".into());
    }
    let root = expand_home(cwd);
    let reference = format!("refs/heads/{branch}");
    let at = git_stdout(&root, &["rev-parse", "--verify", "--quiet", &reference]);
    if at.as_deref() != Some(head) {
        return Err(format!(
            "{branch} is no longer at {head}; nothing was pushed"
        ));
    }
    let config = |key: &str| git_stdout(&root, &["config", &format!("branch.{branch}.{key}")]);
    if let (Some(remote), Some(merge)) = (config("remote"), config("merge")) {
        return git_checked(&root, &["push", &remote, &format!("{head}:{merge}")]);
    }
    let remote = git_remote_name(&root).ok_or_else(|| "No git remote to push to".to_string())?;
    git_checked(&root, &["push", &remote, &format!("{head}:{reference}")])?;
    git_checked(
        &root,
        &["config", &format!("branch.{branch}.remote"), &remote],
    )?;
    git_checked(
        &root,
        &["config", &format!("branch.{branch}.merge"), &reference],
    )
}
