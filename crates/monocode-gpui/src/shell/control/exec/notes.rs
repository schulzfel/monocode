//! `notes_list`, `notes_read` and `notes_write`: Operator's notes calls
//! (`app_access::write_note` and its siblings), on a worker, scoped to the
//! app's local projects.
use super::*;
use monocode_control::ops::{
    self,
    notes::{
        DEFAULT_NOTES_PER_PAGE, MAX_NOTES_PER_PAGE, NoteOutput, NoteSummary, NotesListInput,
        NotesListOutput, NotesReadInput, NotesWriteInput,
    },
};
use monocode_core::{
    app_access::{NoteError, NoteWrite, list_notes, read_note, write_note},
    notes::Note,
};

fn note(note: Note) -> NoteOutput {
    NoteOutput {
        id: note.id,
        title: note.title,
        body: note.body,
        tags: note.tags,
        project: note.source_cwd,
        updated_at: note.updated_at,
    }
}

fn note_error(error: NoteError) -> ControlError {
    match error {
        NoteError::NotFound => {
            ControlError::new(ErrorCode::NotFound, format!("{error}; see notes_list."))
        }
        NoteError::Invalid(message) => ControlError::invalid_input(message),
    }
}

impl Shell {
    /// The local projects notes may belong to: `project`, or all of them.
    fn control_note_projects(&self, project: Option<&str>) -> Result<Vec<String>, ControlError> {
        Ok(match project {
            Some(project) => vec![self.control_local_project(Some(project))?],
            None => self
                .workspace
                .projects
                .iter()
                .filter(|p| !is_remote_path(p))
                .cloned()
                .collect(),
        })
    }

    /// Runs `work` on the store on a worker.
    fn control_notes<T: serde::Serialize + 'static>(
        &self,
        work: impl FnOnce(&monocode_core::store::Store) -> Result<T, NoteError> + Send + 'static,
        cx: &App,
    ) -> Exec {
        let store = self.store.clone();
        Exec::Later(cx.background_executor().spawn(async move {
            let store = locked(&store).map_err(ControlError::internal)?;
            output(&work(&store).map_err(note_error)?)
        }))
    }

    pub(super) fn control_notes_list(
        &self,
        input: &NotesListInput,
        cx: &App,
    ) -> Result<Exec, ControlError> {
        let limit = ops::limit(input.limit, DEFAULT_NOTES_PER_PAGE, MAX_NOTES_PER_PAGE)?;
        let offset = match input.cursor.as_deref() {
            None => 0,
            Some(cursor) => cursor
                .parse::<usize>()
                .map_err(|_| ControlError::invalid_input("cursor is not valid for this list"))?,
        };
        let projects = self.control_note_projects(input.project.as_deref())?;
        Ok(self.control_notes(
            move |store| {
                let (total, notes) = list_notes(store, &projects, limit, offset)?;
                let end = offset + notes.len();
                Ok(NotesListOutput {
                    next_cursor: (end < total).then(|| end.to_string()),
                    total: u32::try_from(total).unwrap_or(u32::MAX),
                    notes: notes
                        .iter()
                        .map(|n| NoteSummary {
                            id: n.id.clone(),
                            title: n.title.clone(),
                            preview: n.preview(),
                            tags: n.tags.clone(),
                            project: n.source_cwd.clone(),
                        })
                        .collect(),
                })
            },
            cx,
        ))
    }

    pub(super) fn control_notes_read(
        &self,
        input: &NotesReadInput,
        cx: &App,
    ) -> Result<Exec, ControlError> {
        let projects = self.control_note_projects(None)?;
        let id = input.id.clone();
        Ok(self.control_notes(move |store| read_note(store, &projects, &id).map(note), cx))
    }

    pub(super) fn control_notes_write(
        &self,
        input: &NotesWriteInput,
        caller: &Caller,
        cx: &App,
    ) -> Result<Exec, ControlError> {
        let projects = if input.id.is_some() {
            self.control_note_projects(None)?
        } else {
            vec![self.control_local_project(input.project.as_deref())?]
        };
        let request = caller.request_key();
        // A session's notes record it as their source, as Operator's do.
        let session = match caller.caller {
            CallerKind::Session(id) => Some(*id),
            CallerKind::External(_) => None,
        };
        let write = NoteWrite {
            id: input.id.clone(),
            title: input.title.clone(),
            body: input.body.clone(),
            tags: input.tags.clone(),
        };
        Ok(self.control_notes(
            move |store| write_note(store, session, &request, write, &projects).map(note),
            cx,
        ))
    }
}
