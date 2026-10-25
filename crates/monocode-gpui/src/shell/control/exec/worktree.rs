//! `worktree_list` and `worktree_create`: Operator's `worktrees.*`, on a
//! worker, for local projects only.
use super::*;
use monocode_control::ops::{
    organize::ProjectInput,
    worktree::{
        BranchInfo, MAX_BRANCHES, WorktreeCreateInput, WorktreeCreateOutput, WorktreeInfo,
        WorktreeListOutput,
    },
};

/// The longest branch or base name accepted, as for Operator.
const MAX_REF_CHARS: usize = 400;

fn info(tree: &monocode_core::git::Worktree) -> WorktreeInfo {
    WorktreeInfo {
        path: tree.path.clone(),
        branch: tree.branch.clone(),
        head: tree.head.clone(),
        main: tree.main,
        missing: tree.missing,
    }
}

fn git_error(error: String) -> ControlError {
    ControlError::invalid_input(error)
}

impl Shell {
    pub(super) fn control_worktree_list(
        &self,
        input: &ProjectInput,
        cx: &App,
    ) -> Result<Exec, ControlError> {
        let project = self.control_local_project(input.project.as_deref())?;
        Ok(Exec::Later(cx.background_executor().spawn(async move {
            let snapshot =
                monocode_core::git::snapshot(std::path::Path::new(&project)).map_err(git_error)?;
            let omitted = snapshot.branches.len().saturating_sub(MAX_BRANCHES);
            output(&WorktreeListOutput {
                worktrees: snapshot.worktrees.iter().map(info).collect(),
                branches: snapshot
                    .branches
                    .iter()
                    .take(MAX_BRANCHES)
                    .map(|b| BranchInfo {
                        name: b.name.clone(),
                        remote: b.remote.clone(),
                    })
                    .collect(),
                branches_omitted: u32::try_from(omitted).unwrap_or(u32::MAX),
                project,
            })
        })))
    }

    pub(super) fn control_worktree_create(
        &self,
        input: WorktreeCreateInput,
        cx: &App,
    ) -> Result<Exec, ControlError> {
        let project = self.control_local_project(input.project.as_deref())?;
        let branch = input.branch.trim().to_owned();
        let base = input
            .base
            .map(|b| b.trim().to_owned())
            .filter(|b| !b.is_empty());
        let existing = input.existing.unwrap_or(false);
        if branch.is_empty() || branch.chars().count() > MAX_REF_CHARS {
            return Err(ControlError::invalid_input(format!(
                "branch must be 1 to {MAX_REF_CHARS} characters"
            )));
        }
        if base
            .as_ref()
            .is_some_and(|b| b.chars().count() > MAX_REF_CHARS)
        {
            return Err(ControlError::invalid_input(format!(
                "base must be at most {MAX_REF_CHARS} characters"
            )));
        }
        if existing && base.is_some() {
            return Err(ControlError::invalid_input(
                "base cannot be set for an existing branch",
            ));
        }
        Ok(Exec::Later(cx.background_executor().spawn(async move {
            let tree = monocode_core::git::create_project_worktree(
                std::path::Path::new(&project),
                &branch,
                base.as_deref(),
                existing,
            )
            .map_err(git_error)?;
            output(&WorktreeCreateOutput {
                project,
                worktree: info(&tree),
            })
        })))
    }
}
