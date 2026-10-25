//! Session lifecycle changes the sidebar, the phone, Operator and external
//! control share: archiving, renaming and moving into a folder.
use super::*;
use monocode_core::session_groups::Groups;

/// Why a session could not move into a folder.
pub(super) const FOLDER_CHOICE: &str = "Supply exactly one of folderId or newFolderName";

/// Puts session `id` into `folder`, or into a new folder called `name`:
/// exactly one of the two. Returns the folder's id and name; the caller
/// saves `groups`.
pub(super) fn move_to_folder(
    groups: &mut Groups,
    id: i64,
    folder: Option<String>,
    name: Option<String>,
) -> Result<(String, String), String> {
    let folder = match (folder, name) {
        (Some(folder), None) => folder,
        (None, Some(name)) => {
            let folder = groups
                .create_folder(&[id], monocode_core::mono::new_id())
                .ok_or("Could not create the folder")?;
            groups.rename_folder(&folder, &name);
            folder
        }
        _ => return Err(FOLDER_CHOICE.into()),
    };
    if groups.folder(&folder).is_none() {
        return Err("Folder not found".into());
    }
    groups.add_to_folder(&folder, id);
    let name = groups
        .folder(&folder)
        .map(|f| f.name.clone())
        .unwrap_or_default();
    Ok((folder, name))
}

impl Shell {
    /// Archives or restores session `id`. Archiving stops its turn and
    /// closes its pane. The task finishes once the store and the window
    /// agree; `None` while another change to it is still being saved.
    pub(super) fn set_session_archived(
        &mut self,
        id: i64,
        archived: bool,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Task<Result<(), String>>> {
        if !self.archiving.insert(id) {
            return None;
        }
        self.close_preview(cx);
        let store = self.store.clone();
        let work = cx
            .background_executor()
            .spawn(async move { locked(&store)?.set_archived(id, archived) });
        let task = cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update_in(cx, |this, window, cx| {
                this.archiving.remove(&id);
                match &result {
                    Ok(()) => {
                        if let Some(summary) = this.session_summaries.get_mut(&id) {
                            summary.archived = archived;
                        }
                        if let Some(view) = this.sessions.get(&id) {
                            view.update(cx, |session, cx| {
                                session.summary.archived = archived;
                                if archived {
                                    session.stop_if_busy(cx);
                                }
                            });
                        }
                        this.refresh_session_mark(id, cx);
                        if archived && this.workspace.tab_of(id).is_some() {
                            this.close_pane(id, window, cx);
                        } else {
                            this.invalidate_chrome(cx);
                            cx.notify();
                        }
                    }
                    Err(error) => {
                        this.error = Some(format!("Could not update archive: {error}"));
                        this.invalidate_chrome(cx);
                        cx.notify();
                    }
                }
            })
            .map_err(|_| "The window closed".to_owned())?;
            result
        });
        self.invalidate_chrome(cx);
        Some(task)
    }

    /// Renames session `id` in the sidebar, its tab and the store.
    pub(super) fn rename_session(
        &mut self,
        id: i64,
        title: &str,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let title = title.trim().to_owned();
        if title.is_empty() || title.chars().count() > 200 {
            return Err("Enter a title under 200 characters".into());
        }
        if let Some(summary) = self.session_summaries.get_mut(&id) {
            summary.title = title.clone();
        }
        if let Some(view) = self.sessions.get(&id) {
            view.update(cx, |v, cx| v.rename(title.clone(), cx));
        }
        let saved = title.clone();
        self.store_write("rename the session", cx, move |s| s.title(id, &saved));
        self.invalidate_chrome(cx);
        cx.notify();
        Ok(title)
    }
}
