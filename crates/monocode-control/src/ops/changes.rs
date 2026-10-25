//! Read-only source control: what changed in a project, or in a session.
use crate::{Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Files listed unless `limit` says otherwise.
pub const DEFAULT_CHANGED_FILES: u32 = 100;
/// The largest `limit` accepted.
pub const MAX_CHANGED_FILES: u32 = 500;
/// The most recent commits listed for a project.
pub const RECENT_COMMITS: u32 = 10;

/// `changes_read`: uncommitted changes of a project, or a session's edits.
#[derive(Debug)]
pub struct ChangesRead;

/// A project, or a session: exactly one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangesReadInput {
    /// A local project's path or key: its working tree against `HEAD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// A session: the files its last turn changed, as its review shows them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<i64>,
    /// Files listed, 1 to 500 (default 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// A changed file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    /// The file, relative to the project or worktree.
    pub path: String,
    /// What happened to it, such as `modified`, `added` or `deleted`.
    pub status: String,
    /// Lines added.
    pub additions: i64,
    /// Lines removed.
    pub deletions: i64,
    /// For a project: whether some of it is staged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staged: Option<bool>,
}

/// A commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    /// Its short id.
    pub sha: String,
    /// Its subject line.
    pub subject: String,
    /// Its author.
    pub author: String,
    /// When it was made, in Unix seconds.
    pub timestamp: i64,
}

/// The changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangesReadOutput {
    /// The folder the paths are relative to.
    pub root: String,
    /// The session, when one was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<i64>,
    /// For a project: its branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// For a project: commits ahead of its upstream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ahead: Option<i64>,
    /// For a project: commits behind its upstream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behind: Option<i64>,
    /// The changed files.
    pub files: Vec<ChangedFile>,
    /// Changed files left out.
    pub omitted: u32,
    /// Lines added in all.
    pub additions: i64,
    /// Lines removed in all.
    pub deletions: i64,
    /// For a project: its latest commits, newest first.
    pub commits: Vec<Commit>,
}

impl Operation for ChangesRead {
    const NAME: &'static str = "changes_read";
    const TITLE: &'static str = "Read changes";
    const SUBJECT: &'static [&'static str] = &["project", "sessionId"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Show what changed, read-only: a local project's uncommitted files with line counts, branch, ahead/behind and latest commits; or the files a session's last turn changed, as its review in the app shows them.";
    const HINTS: Hints = Hints::READ;
    type Input = ChangesReadInput;
    type Output = ChangesReadOutput;

    fn target_session(input: &ChangesReadInput) -> Option<i64> {
        input.session_id
    }

    fn summary(output: &ChangesReadOutput) -> String {
        format!(
            "{} files changed (+{} −{}) in {}",
            output.files.len() + output.omitted as usize,
            output.additions,
            output.deletions,
            output.root
        )
    }
}
