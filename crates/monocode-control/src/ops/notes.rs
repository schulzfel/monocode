//! Notes: the app's markdown notes, scoped to local projects.
use crate::{Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Notes per page unless `limit` says otherwise.
pub const DEFAULT_NOTES_PER_PAGE: u32 = 30;
/// The largest `limit` accepted.
pub const MAX_NOTES_PER_PAGE: u32 = 100;
/// The longest note body accepted, in bytes.
pub const MAX_NOTE_BYTES: usize = 240_000;

/// `notes_list`: notes of local projects, and notes of no project.
#[derive(Debug)]
pub struct NotesList;

/// Which project, and paging.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotesListInput {
    /// Only notes of this local project (path or key), and notes of none;
    /// when absent, an agent session sees its own project's, other clients
    /// every local project's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// `nextCursor` from the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Notes per page, 1 to 100 (default 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// A note in a list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    /// Its id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// The start of its text.
    pub preview: String,
    /// Its tags.
    pub tags: Vec<String>,
    /// The project it belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// One page of notes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotesListOutput {
    /// The notes, newest first.
    pub notes: Vec<NoteSummary>,
    /// How many notes there are in all.
    pub total: u32,
    /// The cursor for the next page, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl Operation for NotesList {
    const NAME: &'static str = "notes_list";
    const TITLE: &'static str = "List notes";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str =
        "List notes of MonoCode's local projects, and notes that belong to no project.";
    const HINTS: Hints = Hints::READ;
    type Input = NotesListInput;
    type Output = NotesListOutput;

    fn summary(output: &NotesListOutput) -> String {
        format!("{} of {} notes", output.notes.len(), output.total)
    }
}

/// A whole note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteOutput {
    /// Its id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its markdown text.
    pub body: String,
    /// Its tags.
    pub tags: Vec<String>,
    /// The project it belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// When it last changed, in Unix milliseconds.
    pub updated_at: i64,
}

/// `notes_read`: one note.
#[derive(Debug)]
pub struct NotesRead;

/// Which note.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotesReadInput {
    /// The note's id from `notes_list`.
    pub id: String,
}

impl Operation for NotesRead {
    const NAME: &'static str = "notes_read";
    const TITLE: &'static str = "Read note";
    const SUBJECT: &'static [&'static str] = &["id"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Read one note's title, markdown text and tags.";
    const HINTS: Hints = Hints::READ;
    type Input = NotesReadInput;
    type Output = NoteOutput;

    fn summary(output: &NoteOutput) -> String {
        format!("Note “{}” ({} bytes)", output.title, output.body.len())
    }
}

/// `notes_write`: create or update a note.
#[derive(Debug)]
pub struct NotesWrite;

/// The note's fields; absent fields of an existing note stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotesWriteInput {
    /// An existing note to update; a new note is created when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// For a new note: the local project it belongs to (path or key); when
    /// absent, the calling agent session's project, or for other clients
    /// the current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Its title; a new note takes it from the text when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Its markdown text (at most 240,000 bytes); required for a new note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Its tags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

impl Operation for NotesWrite {
    const NAME: &'static str = "notes_write";
    const TITLE: &'static str = "Write note";
    const SUBJECT: &'static [&'static str] = &["title", "id"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Create a note in a local project, or change an existing note's title, text or tags. Pass a requestId so a retried create does not make a second note.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = NotesWriteInput;
    type Output = NoteOutput;

    fn project_slot(input: &mut NotesWriteInput) -> Option<&mut Option<String>> {
        // A new note goes in the default project; an update keeps its own.
        input.id.is_none().then_some(&mut input.project)
    }

    fn summary(output: &NoteOutput) -> String {
        format!("Saved note “{}” ({})", output.title, output.id)
    }
}
