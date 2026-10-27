//! A project's git worktrees.
use crate::{Hints, Operation, Scope, ops::organize::ProjectInput};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The most branches listed.
pub const MAX_BRANCHES: usize = 200;

/// A git worktree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    /// Its folder.
    pub path: String,
    /// Its branch, unless it is detached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Its commit.
    pub head: String,
    /// Whether it is the repository's main worktree.
    pub main: bool,
    /// Whether its folder is gone.
    pub missing: bool,
}

/// A branch a worktree can start from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    /// Its name.
    pub name: String,
    /// Its remote, for a remote-tracking branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
}

/// `worktree_list`: a project's worktrees and branches.
#[derive(Debug)]
pub struct WorktreeList;

/// The worktrees and branches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeListOutput {
    /// The project.
    pub project: String,
    /// Its worktrees.
    pub worktrees: Vec<WorktreeInfo>,
    /// Up to 200 of its branches.
    pub branches: Vec<BranchInfo>,
    /// Branches left out.
    pub branches_omitted: u32,
}

impl Operation for WorktreeList {
    const NAME: &'static str = "worktree_list";
    const TITLE: &'static str = "List worktrees";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "List a local project's git worktrees and branches; session_start can work in one with worktreeCwd.";
    const HINTS: Hints = Hints::READ;
    type Input = ProjectInput;
    type Output = WorktreeListOutput;

    fn project_slot(input: &mut ProjectInput) -> Option<&mut Option<String>> {
        Some(&mut input.project)
    }

    fn summary(output: &WorktreeListOutput) -> String {
        format!(
            "{} worktrees, {} branches in {}",
            output.worktrees.len(),
            output.branches.len() + output.branches_omitted as usize,
            output.project
        )
    }
}

/// `worktree_create`: a new worktree of a project.
#[derive(Debug)]
pub struct WorktreeCreate;

/// The branch and where it starts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeCreateInput {
    /// A local project's path or key; when absent, the calling agent
    /// session's project, or for other clients the current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The branch to create, or to check out with `existing`.
    pub branch: String,
    /// The branch or commit a new branch starts from (default `HEAD`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Check out an existing branch instead of creating one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing: Option<bool>,
}

/// The new worktree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreateOutput {
    /// The project.
    pub project: String,
    /// The worktree.
    pub worktree: WorktreeInfo,
}

impl Operation for WorktreeCreate {
    const NAME: &'static str = "worktree_create";
    const TITLE: &'static str = "Create worktree";
    const SUBJECT: &'static [&'static str] = &["branch", "project"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Create a git worktree of a local project, on a new branch (from base, default HEAD) or an existing one, as the app's worktree menu does.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = WorktreeCreateInput;
    type Output = WorktreeCreateOutput;

    fn project_slot(input: &mut WorktreeCreateInput) -> Option<&mut Option<String>> {
        Some(&mut input.project)
    }

    fn summary(output: &WorktreeCreateOutput) -> String {
        format!(
            "Worktree {} on {}",
            output.worktree.path,
            output
                .worktree
                .branch
                .as_deref()
                .unwrap_or("a detached head")
        )
    }
}
