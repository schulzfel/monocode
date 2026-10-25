//! `git_stage`, `git_unstage`, `git_discard`, `git_commit` and `git_push`:
//! the Changes panel's `scm` calls, on a worker, for local projects only.
//! Discard and push run only once the person has confirmed them
//! (`confirm`), and only on what the prompt showed ([`Pin`]): the files a
//! discard reverts and deletes (`discard`), and the branch and commit a
//! push publishes. Every result is bounded; the Changes panel reloads.
use super::changes::project_changes;
use super::*;
use monocode_control::ops::{
    changes::Commit,
    organize::ProjectInput,
    vcs::{
        GitCommitInput, GitCommitOutput, GitPathsInput, GitPushOutput, LISTED_FILES,
        MAX_COMMIT_MESSAGE_CHARS, MAX_PATH_BYTES, MAX_PATHS,
    },
};
use monocode_core::scm;
use std::path::Path;

/// The longest git error passed on, in characters.
const MAX_ERROR_CHARS: usize = 2_000;

/// What the person confirmed, which the call checks again before it acts.
pub(in crate::shell::control) enum Pin {
    /// The files a discard reverts and deletes.
    Discard(super::discard::Plan),
    /// The branch a push publishes and the commit it is at.
    Push { branch: String, head: String },
}

/// A confirmed call that reached its executor without the person's answer.
fn unconfirmed(name: &str) -> ControlError {
    ControlError::internal(format!("{name} runs only after the person confirms it"))
}

/// Which files a call names.
#[derive(Clone)]
pub(in crate::shell::control) enum Files {
    All,
    Paths(Vec<String>),
}

/// The files `input` names: its paths, or every change with `all`.
pub(in crate::shell::control) fn files(input: &GitPathsInput) -> Result<Files, ControlError> {
    match (input.all.unwrap_or(false), input.paths.is_empty()) {
        (true, true) => Ok(Files::All),
        (true, false) => Err(ControlError::invalid_input(
            "Pass either paths or all: true, not both.",
        )),
        (false, true) => Err(ControlError::invalid_input(
            "Name the files in paths, or pass all: true.",
        )),
        (false, false) => {
            if input.paths.len() > MAX_PATHS {
                return Err(ControlError::invalid_input(format!(
                    "At most {MAX_PATHS} paths are accepted per call."
                )));
            }
            if let Some(bad) = input
                .paths
                .iter()
                .find(|p| p.trim().is_empty() || p.len() > MAX_PATH_BYTES || p.contains('\0'))
            {
                return Err(ControlError::invalid_input(format!(
                    "{bad:?} is not a usable path."
                )));
            }
            Ok(Files::Paths(input.paths.clone()))
        }
    }
}

/// A git failure, cut to a readable length.
pub(super) fn git_error(error: String) -> ControlError {
    let cut: String = error.trim().chars().take(MAX_ERROR_CHARS).collect();
    ControlError::invalid_input(if cut.is_empty() {
        "git failed".into()
    } else {
        cut
    })
}

/// Runs `each` for every named file, or `all` once.
fn apply(
    root: &str,
    files: &Files,
    each: fn(&Path, &str) -> Result<(), String>,
    all: fn(&Path) -> Result<(), String>,
) -> Result<(), ControlError> {
    let path = Path::new(root);
    match files {
        Files::All => all(path).map_err(git_error),
        Files::Paths(paths) => paths
            .iter()
            .try_for_each(|relative| each(path, relative))
            .map_err(git_error),
    }
}

impl Shell {
    /// Runs `work` on a worker, then reloads the Changes panel.
    fn git_work(
        &self,
        work: impl FnOnce() -> Result<Value, ControlError> + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Exec {
        let work = cx.background_executor().spawn(async move { work() });
        Exec::Later(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| this.git.update(cx, |git, cx| git.reload(cx)));
            result
        }))
    }

    /// Runs `work` on the local project and files `input` names, on a
    /// worker, and answers with the project's changes after it.
    fn git_files(
        &self,
        input: &GitPathsInput,
        work: impl FnOnce(&str, &Files) -> Result<(), ControlError> + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let root = self.control_local_project(input.project.as_deref())?;
        let files = files(input)?;
        Ok(self.git_work(
            move || {
                work(&root, &files)?;
                output(&project_changes(&root, LISTED_FILES as usize)?)
            },
            cx,
        ))
    }

    pub(super) fn control_git_stage(
        &self,
        input: &GitPathsInput,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        self.git_files(
            input,
            |root, files| apply(root, files, scm::stage_file, scm::stage_all),
            cx,
        )
    }

    pub(super) fn control_git_unstage(
        &self,
        input: &GitPathsInput,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        self.git_files(
            input,
            |root, files| apply(root, files, scm::unstage_file, scm::unstage_all),
            cx,
        )
    }

    pub(super) fn control_git_discard(
        &self,
        input: &GitPathsInput,
        pin: Option<&Pin>,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let Some(Pin::Discard(confirmed)) = pin else {
            return Err(unconfirmed("git_discard"));
        };
        let confirmed = confirmed.clone();
        self.git_files(
            input,
            move |_, files| super::discard::run(&confirmed, files),
            cx,
        )
    }

    pub(super) fn control_git_commit(
        &self,
        input: GitCommitInput,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let root = self.control_local_project(input.project.as_deref())?;
        let message = input.message.trim().to_owned();
        if message.is_empty() || message.chars().count() > MAX_COMMIT_MESSAGE_CHARS {
            return Err(ControlError::invalid_input(format!(
                "The commit message must be 1 to {MAX_COMMIT_MESSAGE_CHARS} characters."
            )));
        }
        Ok(self.git_work(
            move || {
                let path = Path::new(&root);
                let index = scm::diff_index(path).map_err(git_error)?;
                if !index.files.iter().any(|f| f.staged) {
                    return Err(ControlError::invalid_input(format!(
                        "Nothing is staged in {root}; stage files with git_stage first."
                    )));
                }
                scm::commit(path, &message, false).map_err(git_error)?;
                let head = scm::history(path, Some(1))
                    .map_err(git_error)?
                    .commits
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        ControlError::internal("The new commit is not in the history")
                    })?;
                output(&GitCommitOutput {
                    project: root,
                    branch: index.branch,
                    commit: Commit {
                        sha: head.short_sha,
                        subject: head.subject,
                        author: head.author,
                        timestamp: head.timestamp,
                    },
                })
            },
            cx,
        ))
    }

    pub(super) fn control_git_push(
        &self,
        input: &ProjectInput,
        pin: Option<&Pin>,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let Some(Pin::Push { branch, head }) = pin else {
            return Err(unconfirmed("git_push"));
        };
        let root = self.control_local_project(input.project.as_deref())?;
        let (branch, head) = (branch.clone(), head.clone());
        Ok(self.git_work(
            move || {
                let path = Path::new(&root);
                let (now, at, _) = push_target(&root)?;
                if now != branch || at != head {
                    return Err(ControlError::new(
                        ErrorCode::RequestConflict,
                        format!(
                            "{root} moved after the person was asked to push “{branch}” at {}: it is now “{now}” at {}, so nothing was pushed. Call git_push again to ask about what is checked out now.",
                            short(&head),
                            short(&at)
                        ),
                    )
                    .retryable(true));
                }
                scm::push_branch(path, &branch, &head).map_err(git_error)?;
                let index = scm::diff_index(path).map_err(git_error)?;
                output(&GitPushOutput {
                    project: root,
                    branch: index.branch,
                    upstream: index.upstream,
                })
            },
            cx,
        ))
    }
}

/// A commit's short form, for messages.
pub(in crate::shell::control) fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// What a push would publish, for its confirmation: the branch, the commit
/// it is at, and where it goes.
pub(in crate::shell::control) fn push_target(
    root: &str,
) -> Result<(String, String, String), ControlError> {
    let index = scm::diff_index(Path::new(root)).map_err(git_error)?;
    let branch = index
        .branch
        .ok_or_else(|| ControlError::invalid_input(format!("{root} is not on a branch.")))?;
    let head = index
        .head
        .ok_or_else(|| ControlError::invalid_input(format!("{root} has no commit to push.")))?;
    let to = index.upstream.or(index.remote).ok_or_else(|| {
        ControlError::invalid_input(format!("{root} has no git remote to push to."))
    })?;
    Ok((branch, head, to))
}
