//! `session_rename`, `session_archive`, `session_restore`, `folder_list` and
//! `session_move`, through the same paths as the sidebar, the phone and
//! Operator's `folders.*`.
use super::*;
use crate::shell::session_admin::{FOLDER_CHOICE, move_to_folder};
use monocode_control::ops::{
    organize::{
        Folder, FolderListOutput, MAX_FOLDER_NAME_CHARS, MAX_TITLE_CHARS, ProjectInput,
        SessionArchiveOutput, SessionMoveInput, SessionMoveOutput, SessionRenameInput,
        SessionRenameOutput,
    },
    session::SessionStatus,
};

impl Shell {
    pub(super) fn control_session_rename(
        &mut self,
        input: &SessionRenameInput,
        cx: &mut Context<Self>,
    ) -> Result<SessionRenameOutput, ControlError> {
        let id = input.session_id;
        if input.title.trim().chars().count() > MAX_TITLE_CHARS {
            return Err(ControlError::invalid_input(format!(
                "title must be at most {MAX_TITLE_CHARS} characters"
            )));
        }
        let title = self
            .rename_session(id, &input.title, cx)
            .map_err(ControlError::invalid_input)?;
        Ok(SessionRenameOutput {
            session_id: id,
            title,
        })
    }

    /// Archives (`true`) or restores session `id`, answering once the store
    /// and the window agree. A working session is not archived: that would
    /// stop its turn, which a client asks for with `session_stop`.
    pub(super) fn control_session_archive(
        &mut self,
        id: i64,
        archived: bool,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let out = SessionArchiveOutput {
            session_id: id,
            archived,
        };
        if self.session_metadata(id, cx).summary.archived == archived {
            return ready(Ok(out));
        }
        let working = self.session_metadata(id, cx).busy
            || matches!(
                self.control_status_of(id, cx),
                SessionStatus::Working
                    | SessionStatus::Connecting
                    | SessionStatus::Stopping
                    | SessionStatus::AwaitingApproval
                    | SessionStatus::AwaitingAnswer
            );
        if archived && working {
            return Err(ControlError::new(
                ErrorCode::SessionBusy,
                format!(
                    "Session {id} is working; archiving it would stop its turn. Stop it with session_stop first, or wait with session_wait."
                ),
            ));
        }
        let Some(task) = self.set_session_archived(id, archived, cx) else {
            return Err(ControlError::new(
                ErrorCode::SessionBusy,
                format!("Session {id}'s archive state is still being saved; try again."),
            ));
        };
        Ok(Exec::Later(cx.background_executor().spawn(async move {
            task.await.map_err(ControlError::internal)?;
            output(&out)
        })))
    }

    pub(super) fn control_folder_list(
        &self,
        input: &ProjectInput,
    ) -> Result<FolderListOutput, ControlError> {
        let project = self.control_project(input.project.as_deref())?;
        let folders = self
            .groups
            .get(&project)
            .map(|groups| {
                groups
                    .folders
                    .iter()
                    .map(|f| Folder {
                        id: f.id.clone(),
                        name: f.name.clone(),
                        session_ids: f.session_ids.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(FolderListOutput { project, folders })
    }

    pub(super) fn control_session_move(
        &mut self,
        input: SessionMoveInput,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let id = input.session_id;
        if input
            .new_folder_name
            .as_ref()
            .is_some_and(|n| n.trim().is_empty() || n.chars().count() > MAX_FOLDER_NAME_CHARS)
        {
            return Err(ControlError::invalid_input(format!(
                "newFolderName must be 1 to {MAX_FOLDER_NAME_CHARS} characters"
            )));
        }
        let project = self.session_metadata(id, cx).summary.cwd.clone();
        let mut groups = self.groups.get(&project).cloned().unwrap_or_default();
        let name = input.new_folder_name.map(|n| n.trim().to_owned());
        let (folder, name) = match move_to_folder(&mut groups, id, input.folder_id, name) {
            Ok(moved) => moved,
            Err(error) => {
                return Err(match error.as_str() {
                    FOLDER_CHOICE => ControlError::invalid_input(error),
                    "Folder not found" => ControlError::new(
                        ErrorCode::NotFound,
                        "There is no such folder in the session's project; see folder_list.",
                    ),
                    _ => ControlError::internal(error),
                });
            }
        };
        let store = self.store.clone();
        let saved = groups.clone();
        let target = project.clone();
        let save = cx
            .background_executor()
            .spawn(async move { locked(&store)?.save_groups(&target, &saved) });
        Ok(Exec::Later(cx.spawn(async move |this, cx| {
            save.await.map_err(ControlError::internal)?;
            this.update(cx, |this, cx| {
                this.groups.insert(project, groups);
                this.invalidate_chrome(cx);
                cx.notify();
            })
            .map_err(|_| ControlError::internal("The window closed"))?;
            output(&SessionMoveOutput {
                session_id: id,
                folder_id: folder,
                folder_name: name,
            })
        })))
    }
}
