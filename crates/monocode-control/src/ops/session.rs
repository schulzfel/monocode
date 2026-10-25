//! Agent sessions: what they are doing, and their history.
use crate::{AccessMode, Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What a session is doing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionStatus {
    /// Its history is loading.
    Loading,
    /// Nothing runs; the last turn, if any, finished.
    Idle,
    /// A turn is starting: connecting, creating its worktree or handing off.
    Connecting,
    /// A turn is running.
    Working,
    /// The person stopped the turn and it is winding down.
    Stopping,
    /// The agent waits for the person to approve a tool call in the app.
    AwaitingApproval,
    /// The agent asked a question and waits for the answer.
    AwaitingAnswer,
    /// The last step failed or was refused.
    Error {
        /// What went wrong, as the app shows it.
        message: String,
    },
}

impl SessionStatus {
    /// Whether the session is settled: idle, failed, or waiting on the person.
    #[must_use]
    pub fn settled(&self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Error { .. } | Self::AwaitingApproval | Self::AwaitingAnswer
        )
    }
    /// The wire identifier of the state.
    #[must_use]
    pub fn id(&self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::Working => "working",
            Self::Stopping => "stopping",
            Self::AwaitingApproval => "awaiting_approval",
            Self::AwaitingAnswer => "awaiting_answer",
            Self::Error { .. } => "error",
        }
    }
}

/// Which sessions a list includes by archive state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArchivedFilter {
    /// Only sessions that are not archived.
    #[default]
    Exclude,
    /// All sessions.
    Include,
    /// Only archived sessions.
    Only,
}

/// `session_list`: a project's sessions, newest activity first.
#[derive(Debug)]
pub struct SessionList;

/// Sessions per page unless `limit` says otherwise.
pub const DEFAULT_SESSIONS_PER_PAGE: u32 = 50;
/// The largest `limit` accepted.
pub const MAX_SESSIONS_PER_PAGE: u32 = 200;

/// Which project, and paging.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionListInput {
    /// A local project's path or key, as `project_list` reports it; the
    /// current project when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Archived sessions: `exclude` (default), `include` or `only`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<ArchivedFilter>,
    /// `nextCursor` from the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Sessions per page (1 to 200, default 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    /// The session's id, for the other session tools.
    pub id: i64,
    /// Its title.
    pub title: String,
    /// The project it belongs to.
    pub project: String,
    /// The agent provider, for example `claude` or `codex`.
    pub harness: String,
    /// The model, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// What it is doing.
    pub status: SessionStatus,
    /// Whether a turn is running.
    pub busy: bool,
    /// Whether it was archived.
    pub archived: bool,
    /// Whether an unsent message is saved in it.
    pub has_draft: bool,
    /// Who started it from outside the app: `mcp:<client>` for a control
    /// client, `session:<id>` for another session through Operator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_by: Option<String>,
    /// When it last changed, in Unix milliseconds.
    pub updated_at: i64,
}

/// One page of sessions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionListOutput {
    /// The project listed.
    pub project: String,
    /// Sessions on this page.
    pub sessions: Vec<SessionSummary>,
    /// How many sessions match in all.
    pub total: u32,
    /// Pass as `cursor` for the next page; absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl Operation for SessionList {
    const NAME: &'static str = "session_list";
    const TITLE: &'static str = "List sessions";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "List the agent sessions of a local project (the current one by default), most recently active first, with each session's status, harness, model, archive state and who started it. Paged with `cursor` and `limit`; `archived` chooses exclude (default), include or only.";
    const HINTS: Hints = Hints::READ;
    type Input = SessionListInput;
    type Output = SessionListOutput;

    fn summary(output: &SessionListOutput) -> String {
        let busy = output.sessions.iter().filter(|s| s.busy).count();
        let more = if output.next_cursor.is_some() {
            "; more with nextCursor"
        } else {
            ""
        };
        format!(
            "{} of {} sessions in {} ({busy} working){more}",
            output.sessions.len(),
            output.total,
            output.project
        )
    }
}

/// `session_read`: a session's exchanges, newest last.
#[derive(Debug)]
pub struct SessionRead;

/// Which session and which page of its exchanges.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionReadInput {
    /// The session's id.
    pub session_id: i64,
    /// `nextBefore` from the previous page: exchanges before this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Exchanges per page (1 to 3, default 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// The most characters kept per message (200 to 6000, default 1200).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars: Option<u32>,
}

/// A message, cut to `maxChars`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    /// The text.
    pub text: String,
    /// Whether it was cut.
    pub truncated: bool,
}

/// One exchange: the person's message and the agent's last reply to it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    /// The turn's id, for `before`.
    pub turn_id: String,
    /// What the person sent.
    pub user: Text,
    /// The agent's last reply, if it replied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant: Option<Text>,
    /// Earlier replies in the same turn that are left out.
    pub earlier_assistant_messages: u32,
}

/// One page of exchanges.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionReadOutput {
    /// The session's id.
    pub session_id: i64,
    /// Its title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Whether a turn is running.
    pub busy: bool,
    /// Whether an unsent message is saved in it.
    pub has_draft: bool,
    /// Exchanges on this page, oldest first.
    pub turns: Vec<Turn>,
    /// Pass as `before` for the page before this one; absent at the start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_before: Option<String>,
}

impl Operation for SessionRead {
    const NAME: &'static str = "session_read";
    const TITLE: &'static str = "Read session";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Read a session's exchanges, newest last: each of the person's messages with the agent's last reply, cut to `maxChars`. Page back with `before` set to `nextBefore`.";
    const HINTS: Hints = Hints::READ;
    type Input = SessionReadInput;
    type Output = SessionReadOutput;

    fn target_session(input: &SessionReadInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionReadOutput) -> String {
        let more = if output.next_before.is_some() {
            "; earlier with nextBefore"
        } else {
            ""
        };
        format!(
            "{} exchanges of session {}{}{more}",
            output.turns.len(),
            output.session_id,
            if output.busy { " (working)" } else { "" }
        )
    }
}

/// `session_status`: what a session is doing right now.
#[derive(Debug)]
pub struct SessionStatusRead;

/// Which session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionInput {
    /// The session's id.
    pub session_id: i64,
}

/// How much of the model's context window is used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    /// Tokens used.
    pub used: u64,
    /// The window's size in tokens, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
}

/// A session's live state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatusOutput {
    /// The session's id.
    pub session_id: i64,
    /// The project it belongs to.
    pub project: String,
    /// What it is doing.
    pub status: SessionStatus,
    /// The status line the app shows for it.
    pub status_text: String,
    /// Whether a turn is running.
    pub busy: bool,
    /// Tool approvals waiting for the person in the app.
    pub pending_approvals: u32,
    /// Questions waiting for an answer.
    pub pending_questions: u32,
    /// Messages queued to send after the current turn.
    pub queue_length: u32,
    /// Context window use, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextUsage>,
    /// The agent provider.
    pub harness: String,
    /// The model, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The session's access mode.
    pub access: AccessMode,
}

impl Operation for SessionStatusRead {
    const NAME: &'static str = "session_status";
    const TITLE: &'static str = "Check session";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Show what a session is doing: its status (idle, working, awaiting_approval, awaiting_answer, error…), pending approvals and questions, queued messages, context use, model and access mode.";
    const HINTS: Hints = Hints::READ;
    type Input = SessionInput;
    type Output = SessionStatusOutput;

    fn target_session(input: &SessionInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionStatusOutput) -> String {
        format!(
            "Session {}: {} ({}); {} queued, {} approvals, {} questions",
            output.session_id,
            output.status.id(),
            output.status_text,
            output.queue_length,
            output.pending_approvals,
            output.pending_questions
        )
    }
}
