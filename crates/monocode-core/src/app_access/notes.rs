//! Notes for Operator, Monos and external control clients: typed calls, and
//! the JSON actions `notes.list`, `notes.read` and `notes.write` Operator
//! threads and Monos use. Notes keep their persisted string IDs, including
//! desktop-created ones, and a caller sees only the notes of its projects
//! and those of no project.
use super::{bounded, optional_text};
use crate::{
    notes::{Note, NoteUpsert},
    store::Store,
};
use serde_json::{json, Value};
use std::fmt;

/// The longest body a write accepts, in bytes.
const MAX_BODY: usize = 240_000;

/// Why a notes call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteError {
    /// No note with that ID among those the caller may see.
    NotFound,
    /// The input or the store refused; the text says why.
    Invalid(String),
}

impl fmt::Display for NoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("Note not found in this project"),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl From<String> for NoteError {
    fn from(message: String) -> Self {
        Self::Invalid(message)
    }
}

/// A note to create, or the changes to the note `id` names: absent fields
/// keep its values, and a new note needs a body.
#[derive(Clone, Debug, Default)]
pub struct NoteWrite {
    pub id: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// The notes `projects` may see.
fn visible(store: &Store, projects: &[String]) -> Result<Vec<Note>, NoteError> {
    let keys: Vec<String> = projects.iter().map(|p| crate::files::path_key(p)).collect();
    let mut notes = store.list_notes()?;
    notes.retain(|note| {
        note.source_cwd
            .as_deref()
            .is_none_or(|cwd| keys.contains(&crate::files::path_key(cwd)))
    });
    Ok(notes)
}

/// The note `id` names: its own ID, its desktop alias or its numeric handle.
fn find(notes: Vec<Note>, id: &str) -> Result<Note, NoteError> {
    let number = id.parse::<u64>().ok();
    notes
        .into_iter()
        .find(|n| {
            n.id == id
                || n.id == format!("gpui-note-{id}")
                || number.is_some_and(|number| {
                    crate::store::desktop_session_handle(&n.id) as u64 == number
                })
        })
        .ok_or(NoteError::NotFound)
}

/// How many notes `projects` may see, and at most `limit` of them from
/// `offset` on.
pub fn list_notes(
    store: &Store,
    projects: &[String],
    limit: usize,
    offset: usize,
) -> Result<(usize, Vec<Note>), NoteError> {
    let notes = visible(store, projects)?;
    let total = notes.len();
    Ok((total, notes.into_iter().skip(offset).take(limit).collect()))
}

/// The note `id` names among those `projects` may see.
pub fn read_note(store: &Store, projects: &[String], id: &str) -> Result<Note, NoteError> {
    find(visible(store, projects)?, id)
}

/// Creates a note in the first of `projects`, or changes the one
/// `write.id` names, for `caller`'s `request`: retrying the request finds
/// the note it created. A caller without a session, such as an external
/// control client, records no source session.
pub fn write_note(
    store: &Store,
    caller: Option<i64>,
    request: &str,
    write: NoteWrite,
    projects: &[String],
) -> Result<Note, NoteError> {
    let current = write
        .id
        .as_deref()
        .map(|id| find(visible(store, projects)?, id))
        .transpose()?;
    let title = write
        .title
        .map(|title| {
            let title = title.trim();
            (!title.is_empty() && title.len() <= crate::notes::MAX_TITLE)
                .then(|| title.to_owned())
                .ok_or_else(|| format!("Provide title (up to {} bytes).", crate::notes::MAX_TITLE))
        })
        .transpose()?;
    let body = write
        .body
        .map(|body| {
            (body.len() <= MAX_BODY)
                .then(|| body.replace("\r\n", "\n").replace('\r', "\n"))
                .ok_or_else(|| format!("body must be text under {MAX_BODY} bytes"))
        })
        .transpose()?;
    let tags = write.tags;
    if current.is_some() && title.is_none() && body.is_none() && tags.is_none() {
        return Err(NoteError::Invalid(
            "Supply title, body or tags to update a note".into(),
        ));
    }
    let Some(body) = body.or_else(|| current.as_ref().map(|n| n.body.clone())) else {
        return Err(NoteError::Invalid(
            "body is required to create a note".into(),
        ));
    };
    let id = current
        .as_ref()
        .map(|n| n.id.clone())
        .or(write.id)
        .unwrap_or_else(|| {
            let request = crate::mono::files::content_hash(request);
            match caller {
                Some(caller) => format!("app-note-{caller}-{request}"),
                None => format!("control-note-{request}"),
            }
        });
    let source_session_id = match (&current, caller) {
        (Some(note), _) => note.source_session_id.clone(),
        (None, Some(caller)) => Some(store.persisted_session_id(caller)?),
        (None, None) => None,
    };
    Ok(store.upsert_note(&NoteUpsert {
        id,
        title: title
            .or_else(|| current.as_ref().map(|n| n.title.clone()))
            .unwrap_or_else(|| crate::notes::title(&body)),
        body,
        tags: tags
            .or_else(|| current.as_ref().map(|n| n.tags.clone()))
            .unwrap_or_default(),
        source_session_id,
        source_cwd: current
            .map(|n| n.source_cwd)
            .unwrap_or_else(|| projects.first().cloned()),
        finalize_slug: true,
    })?)
}

/// `notes.list`, `notes.read` and `notes.write` for an Operator thread or
/// Mono, session `caller`.
pub fn notes_action(
    store: &Store,
    caller: i64,
    request: &str,
    action: &str,
    input: &Value,
    projects: &[String],
) -> Result<Value, String> {
    notes_action_for(store, Some(caller), request, action, input, projects)
}

/// [`notes_action`] for a caller that may have no session.
pub fn notes_action_for(
    store: &Store,
    caller: Option<i64>,
    request: &str,
    action: &str,
    input: &Value,
    projects: &[String],
) -> Result<Value, String> {
    let id = input
        .get("id")
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_u64().map(|id| id.to_string()))
                .ok_or("id must be a note ID")
        })
        .transpose()?;
    let note = match action {
        "notes.list" => {
            let limit = bounded(input, "limit", 30, 1, 100)?;
            let offset = bounded(input, "offset", 0, 0, usize::MAX)?;
            let (total, notes) =
                list_notes(store, projects, limit, offset).map_err(|e| e.to_string())?;
            return Ok(
                json!({"total":total,"offset":offset,"notes":notes.iter().map(|n|json!({"id":n.id,"title":n.title,"preview":n.preview(),"tags":n.tags,"sourceCwd":n.source_cwd})).collect::<Vec<_>>() }),
            );
        }
        "notes.read" => read_note(store, projects, id.as_deref().ok_or("Provide id")?),
        _ => {
            let write = NoteWrite {
                id,
                title: optional_text(input, "title", crate::notes::MAX_TITLE)?,
                body: input
                    .get("body")
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| format!("body must be text under {MAX_BODY} bytes"))
                    })
                    .transpose()?,
                tags: input
                    .get("tags")
                    .map(|v| serde_json::from_value(v.clone()).map_err(|e| e.to_string()))
                    .transpose()?,
            };
            write_note(store, caller, request, write, projects)
        }
    };
    note.map(|note| json!(note)).map_err(|e| e.to_string())
}
