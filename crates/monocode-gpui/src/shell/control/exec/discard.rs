//! What `git_discard` may touch: only files the repository reports as
//! unstaged changes (modified or deleted in the working tree, or untracked
//! and not ignored), inside the repository once the folders on their way are
//! resolved. Anything else is refused before the person is asked. The plan
//! says which files are reverted and which are deleted; the call checks it
//! again when the person confirms and acts only if it still holds.
use super::vcs::{Files, git_error};
use super::*;
use monocode_core::scm;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// What discarding does to one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::shell::control) enum Change {
    /// A tracked file goes back to its staged or committed version.
    Revert,
    /// An untracked file is deleted.
    Delete,
}

/// The files a discard touches, as the person is shown them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::shell::control) struct Plan {
    pub(in crate::shell::control) root: String,
    pub(in crate::shell::control) files: Vec<(String, Change)>,
}

impl Plan {
    /// The files `change` does, in order.
    pub(in crate::shell::control) fn named(&self, change: Change) -> Vec<&str> {
        self.files
            .iter()
            .filter(|(_, c)| *c == change)
            .map(|(path, _)| path.as_str())
            .collect()
    }
}

/// A path as git names it relative to the repository.
fn normalized(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    path.strip_prefix("./").unwrap_or(&path).to_owned()
}

/// Whether `path`'s folder, once resolved, is inside `root` (already
/// resolved). The file itself is not followed: a link is reverted or
/// deleted, never what it points to.
fn inside(root: &Path, path: &Path) -> bool {
    let mut folder = path.parent();
    while let Some(dir) = folder {
        if let Ok(real) = std::fs::canonicalize(dir) {
            return real.starts_with(root);
        }
        folder = dir.parent();
    }
    false
}

/// What discarding `files` in `root` would do now.
pub(in crate::shell::control) fn plan(root: &str, files: &Files) -> Result<Plan, ControlError> {
    let path = Path::new(root);
    let index = scm::diff_index(path).map_err(git_error)?;
    let changed: BTreeMap<String, Change> = index
        .files
        .into_iter()
        .filter(|file| file.unstaged)
        .map(|file| {
            let change = if file.status == "untracked" {
                Change::Delete
            } else {
                Change::Revert
            };
            (file.relative, change)
        })
        .collect();
    let wanted: Vec<String> = match files {
        Files::All => changed.keys().cloned().collect(),
        Files::Paths(paths) => paths.iter().map(|p| normalized(p)).collect(),
    };
    if wanted.is_empty() {
        return Err(ControlError::invalid_input(format!(
            "{root} has no unstaged changes to discard."
        )));
    }
    let real: PathBuf = std::fs::canonicalize(path)
        .map_err(|e| ControlError::internal(format!("Cannot resolve {root}: {e}")))?;
    let mut planned: Vec<(String, Change)> = Vec::new();
    for relative in wanted {
        let Some(change) = changed.get(&relative) else {
            return Err(ControlError::invalid_input(format!(
                "{relative} is not an unstaged change in {root}. Only files changes_read lists as unstaged (modified or deleted, or untracked and not ignored) can be discarded."
            )));
        };
        if !inside(&real, &path.join(&relative)) {
            return Err(ControlError::invalid_input(format!(
                "{relative} leads outside {root} once its folders are resolved; only unstaged changes inside the repository can be discarded."
            )));
        }
        if !planned.iter().any(|(p, _)| *p == relative) {
            planned.push((relative, *change));
        }
    }
    Ok(Plan {
        root: root.to_owned(),
        files: planned,
    })
}

/// Discards `confirmed` if it still is what discarding would do; otherwise
/// nothing is touched and the call says why.
pub(in crate::shell::control) fn run(confirmed: &Plan, files: &Files) -> Result<(), ControlError> {
    let changed = || {
        ControlError::new(
            ErrorCode::RequestConflict,
            format!(
                "The changes in {} changed after the person was asked; nothing was discarded. Call git_discard again to ask about them as they are now.",
                confirmed.root
            ),
        )
        .retryable(true)
    };
    let now = match plan(&confirmed.root, files) {
        Ok(now) if now == *confirmed => now,
        Ok(_) => return Err(changed()),
        // What the person confirmed is no longer an unstaged change.
        Err(error) if error.code == ErrorCode::InvalidInput => return Err(changed()),
        Err(error) => return Err(error),
    };
    let root = Path::new(&confirmed.root);
    for (relative, _) in &now.files {
        scm::discard_file(root, relative).map_err(git_error)?;
    }
    Ok(())
}
