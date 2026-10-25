//! Source control in a local project, behind the `vcs` scope: stage, unstage,
//! commit, and, each confirmed in the app, discard and push. There is no
//! commit-message generation: it would spend model tokens on the person's
//! account.
use crate::{
    Hints, Operation, Scope,
    ops::{
        changes::{ChangesReadOutput, Commit},
        organize::ProjectInput,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The most paths one call names.
pub const MAX_PATHS: usize = 200;
/// The longest path accepted, in bytes.
pub const MAX_PATH_BYTES: usize = 4_096;
/// The longest commit message accepted, in characters.
pub const MAX_COMMIT_MESSAGE_CHARS: usize = 20_000;
/// The most changed files a result lists.
pub const LISTED_FILES: u32 = 100;

/// Files of a local project: `paths`, or every changed file with `all`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitPathsInput {
    /// A local project's path or key; the current project when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Files or folders relative to the project (at most 200).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// Every changed file instead of `paths`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
}

fn changes_summary(verb: &str, output: &ChangesReadOutput) -> String {
    let staged = output
        .files
        .iter()
        .filter(|f| f.staged == Some(true))
        .count();
    format!(
        "{verb} in {}: {} files changed, {staged} of those listed staged",
        output.root,
        output.files.len() + output.omitted as usize
    )
}

/// `git_stage`: stage files.
#[derive(Debug)]
pub struct GitStage;

impl Operation for GitStage {
    const NAME: &'static str = "git_stage";
    const TITLE: &'static str = "Stage";
    const SUBJECT: &'static [&'static str] = &["paths", "all", "project"];
    const SCOPE: Scope = Scope::Vcs;
    const DESCRIPTION: &'static str = "Stage files of a local project (`git add`): the given paths, or every change with `all`. Returns the project's changes afterwards, at most 100 files.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = GitPathsInput;
    type Output = ChangesReadOutput;

    fn summary(output: &ChangesReadOutput) -> String {
        changes_summary("Staged", output)
    }
}

/// `git_unstage`: unstage files.
#[derive(Debug)]
pub struct GitUnstage;

impl Operation for GitUnstage {
    const NAME: &'static str = "git_unstage";
    const TITLE: &'static str = "Unstage";
    const SUBJECT: &'static [&'static str] = &["paths", "all", "project"];
    const SCOPE: Scope = Scope::Vcs;
    const DESCRIPTION: &'static str = "Unstage files of a local project (`git restore --staged`), keeping their changes: the given paths, or everything with `all`. Returns the project's changes afterwards, at most 100 files.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = GitPathsInput;
    type Output = ChangesReadOutput;

    fn summary(output: &ChangesReadOutput) -> String {
        changes_summary("Unstaged", output)
    }
}

/// `git_discard`: throw away unstaged changes.
#[derive(Debug)]
pub struct GitDiscard;

impl Operation for GitDiscard {
    const NAME: &'static str = "git_discard";
    const TITLE: &'static str = "Discard changes";
    const SUBJECT: &'static [&'static str] = &["paths", "all", "project"];
    const SCOPE: Scope = Scope::Vcs;
    const DESCRIPTION: &'static str = "Discard unstaged changes in a local project, after the person confirms in MonoCode: tracked files go back to the index, untracked ones are deleted. The given paths, or every unstaged change with `all`. Discarded work cannot be recovered. Declining, or no answer within two minutes, fails with user_declined.";
    const HINTS: Hints = Hints::DISCARD;
    const CONFIRM: bool = true;
    type Input = GitPathsInput;
    type Output = ChangesReadOutput;

    fn summary(output: &ChangesReadOutput) -> String {
        changes_summary("Discarded", output)
    }
}

/// `git_commit`: commit what is staged.
#[derive(Debug)]
pub struct GitCommit;

/// The project and the commit message.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitCommitInput {
    /// A local project's path or key; the current project when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The commit message (1 to 20,000 characters).
    pub message: String,
}

/// The new commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitOutput {
    /// The project.
    pub project: String,
    /// Its branch, when on one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The commit.
    pub commit: Commit,
}

impl Operation for GitCommit {
    const NAME: &'static str = "git_commit";
    const TITLE: &'static str = "Commit";
    const SUBJECT: &'static [&'static str] = &["message"];
    const SCOPE: Scope = Scope::Vcs;
    const DESCRIPTION: &'static str = "Commit the staged changes of a local project with the given message. Only staged changes are committed; nothing staged is invalid_input. MonoCode does not write the message for you.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = GitCommitInput;
    type Output = GitCommitOutput;

    fn summary(output: &GitCommitOutput) -> String {
        format!(
            "Committed {} “{}” in {}",
            output.commit.sha, output.commit.subject, output.project
        )
    }
}

/// `git_push`: push the current branch.
#[derive(Debug)]
pub struct GitPush;

/// The pushed branch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitPushOutput {
    /// The project.
    pub project: String,
    /// The branch pushed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Its upstream now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
}

impl Operation for GitPush {
    const NAME: &'static str = "git_push";
    const TITLE: &'static str = "Push";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Vcs;
    const DESCRIPTION: &'static str = "Push a local project's current branch to its upstream (or set the upstream on its remote on the first push), after the person confirms in MonoCode. Never forces. Declining, or no answer within two minutes, fails with user_declined.";
    // Publishing cannot be taken back; pushing again adds nothing.
    const HINTS: Hints = Hints::DISCARD;
    const CONFIRM: bool = true;
    type Input = ProjectInput;
    type Output = GitPushOutput;

    fn summary(output: &GitPushOutput) -> String {
        format!(
            "Pushed {} in {}{}",
            output.branch.as_deref().unwrap_or("HEAD"),
            output.project,
            output
                .upstream
                .as_deref()
                .map_or(String::new(), |u| format!(" to {u}"))
        )
    }
}
