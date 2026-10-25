//! `changes_read`: a local project's working tree (`scm::diff_index`,
//! `scm::history`), or a session's recorded turn as its review reads it
//! (`checkpoint::status_readonly_aliases`). Read-only, on a worker.
use super::*;
use monocode_control::ops::{
    self,
    changes::{
        ChangedFile, ChangesReadInput, ChangesReadOutput, Commit, DEFAULT_CHANGED_FILES,
        MAX_CHANGED_FILES, RECENT_COMMITS,
    },
};

fn capped(files: Vec<ChangedFile>, limit: usize) -> (Vec<ChangedFile>, u32) {
    let omitted = u32::try_from(files.len().saturating_sub(limit)).unwrap_or(u32::MAX);
    (files.into_iter().take(limit).collect(), omitted)
}

pub(in crate::shell::control) fn project_changes(
    root: &str,
    limit: usize,
) -> Result<ChangesReadOutput, ControlError> {
    let path = std::path::Path::new(root);
    let index = monocode_core::scm::diff_index(path).map_err(ControlError::invalid_input)?;
    let commits = monocode_core::scm::history(path, Some(RECENT_COMMITS))
        .map(|h| h.commits)
        .unwrap_or_default();
    let files = index
        .files
        .iter()
        .map(|f| ChangedFile {
            path: f.relative.clone(),
            status: f.status.clone(),
            additions: f.additions,
            deletions: f.deletions,
            staged: Some(f.staged),
        })
        .collect();
    let (files, omitted) = capped(files, limit);
    Ok(ChangesReadOutput {
        root: root.to_owned(),
        session_id: None,
        branch: index.branch,
        ahead: Some(index.ahead),
        behind: Some(index.behind),
        files,
        omitted,
        additions: index.additions,
        deletions: index.deletions,
        commits: commits
            .into_iter()
            .take(RECENT_COMMITS as usize)
            .map(|c| Commit {
                sha: c.short_sha,
                subject: c.subject,
                author: c.author,
                timestamp: c.timestamp,
            })
            .collect(),
    })
}

/// The files session `id`'s last turn changed, as its review reads them
/// from `store` (the app's checkpoint store).
pub(in crate::shell::control) fn session_changes(
    store: Option<&monocode_core::checkpoint::Store>,
    id: i64,
    aliases: &[String],
    cwd: &str,
    limit: usize,
) -> Result<ChangesReadOutput, ControlError> {
    let store = store.ok_or_else(|| ControlError::internal("Session changes are unavailable"))?;
    let status = store
        .exclusive(|s| s.status_readonly_aliases(aliases, cwd))
        .map_err(ControlError::internal)?;
    let (additions, deletions) = status
        .files
        .iter()
        .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions));
    let files = status
        .files
        .into_iter()
        .map(|f| ChangedFile {
            path: f.relative,
            status: f.status,
            additions: f.additions,
            deletions: f.deletions,
            staged: None,
        })
        .collect();
    let (files, omitted) = capped(files, limit);
    Ok(ChangesReadOutput {
        root: cwd.to_owned(),
        session_id: Some(id),
        branch: None,
        ahead: None,
        behind: None,
        files,
        omitted,
        additions,
        deletions,
        commits: Vec::new(),
    })
}

impl Shell {
    pub(super) fn control_changes_read(
        &self,
        input: &ChangesReadInput,
        cx: &App,
    ) -> Result<Exec, ControlError> {
        let limit = ops::limit(input.limit, DEFAULT_CHANGED_FILES, MAX_CHANGED_FILES)?;
        match (input.project.as_deref(), input.session_id) {
            (Some(project), None) => {
                let root = self.control_local_project(Some(project))?;
                Ok(Exec::Later(cx.background_executor().spawn(async move {
                    output(&project_changes(&root, limit)?)
                })))
            }
            (None, Some(id)) => {
                self.control_local_session(id, cx)?;
                // The review's folder: the session's worktree once it has one.
                let cwd = self.sessions.get(&id).map_or_else(
                    || self.session_metadata(id, cx).summary.cwd.clone(),
                    |view| view.read(cx).review.read(cx).session().1.to_owned(),
                );
                let store = self.store.clone();
                Ok(Exec::Later(cx.background_executor().spawn(async move {
                    let mut aliases = vec![id.to_string()];
                    if let Ok(persisted) = locked(&store).and_then(|s| s.persisted_session_id(id))
                        && !aliases.contains(&persisted)
                    {
                        aliases.push(persisted);
                    }
                    let store = monocode_core::checkpoint::store();
                    output(&session_changes(store, id, &aliases, &cwd, limit)?)
                })))
            }
            _ => Err(ControlError::invalid_input(
                "Pass exactly one of project or sessionId.",
            )),
        }
    }
}
