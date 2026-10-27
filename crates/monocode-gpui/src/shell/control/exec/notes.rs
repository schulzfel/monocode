//! `notes_list`, `notes_read` and `notes_write`: Operator's notes calls
//! (`app_access::write_note` and its siblings), on a worker, scoped to the
//! app's local projects. An agent session sees its own project's notes and
//! those of no project, as Operator's threads do; a Mono those of its
//! assigned projects and of no project, as its old app actions did.
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
    /// The local projects notes may belong to: `project`; else a session
    /// caller's own project, a Mono's assigned projects, or for other
    /// clients all of them.
    fn control_note_projects(
        &self,
        project: Option<&str>,
        caller: &CallerKind,
        cx: &App,
    ) -> Result<Vec<String>, ControlError> {
        if let Some(project) = project {
            return Ok(vec![self.control_local_project_for(
                caller,
                Some(project),
                cx,
            )?]);
        }
        if let CallerKind::Mono { mono, .. } = caller {
            let mono = self
                .caller_mono(mono, cx)
                .ok_or_else(|| ControlError::new(ErrorCode::NotFound, caller::MONO_GONE))?;
            return Ok(mono.projects.clone());
        }
        Ok(match self.caller_project(caller, cx) {
            Some(own) => vec![own],
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
        caller: &Caller,
        cx: &App,
    ) -> Result<Exec, ControlError> {
        let limit = ops::limit(input.limit, DEFAULT_NOTES_PER_PAGE, MAX_NOTES_PER_PAGE)?;
        let offset = match input.cursor.as_deref() {
            None => 0,
            Some(cursor) => cursor
                .parse::<usize>()
                .map_err(|_| ControlError::invalid_input("cursor is not valid for this list"))?,
        };
        let projects = self.control_note_projects(input.project.as_deref(), caller.caller, cx)?;
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
        caller: &Caller,
        cx: &App,
    ) -> Result<Exec, ControlError> {
        let projects = self.control_note_projects(None, caller.caller, cx)?;
        let id = input.id.clone();
        Ok(self.control_notes(move |store| read_note(store, &projects, &id).map(note), cx))
    }

    pub(super) fn control_notes_write(
        &self,
        input: &NotesWriteInput,
        caller: &Caller,
        cx: &App,
    ) -> Result<Exec, ControlError> {
        let projects = match (&input.id, input.project.as_deref(), caller.caller) {
            (Some(_), ..) => self.control_note_projects(None, caller.caller, cx)?,
            // A Mono's defaults leave a new note without a project only
            // when it has none: the note then belongs to no project.
            (None, None, CallerKind::Mono { .. }) => Vec::new(),
            (None, project, _) => {
                vec![self.control_local_project_for(caller.caller, project, cx)?]
            }
        };
        let request = caller.request_key();
        // A session's notes record it as their source, as Operator's do; a
        // Mono's record the chat or habit run it wrote them from.
        let session = match caller.caller {
            CallerKind::Session(id) | CallerKind::Mono { session: id, .. } => Some(*id),
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
