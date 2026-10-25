//! Dispatching work: start a session, message it, wait for it, stop it.
use crate::{
    AccessMode, Hints, Operation, Scope,
    ops::session::{SessionInput, SessionStatus, Text},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The longest prompt accepted, in bytes, as for Operator.
pub const MAX_PROMPT_BYTES: usize = 240_000;
/// How long `session_wait` waits unless `timeoutMs` says otherwise.
pub const DEFAULT_WAIT_MS: u32 = 30_000;
/// The longest `timeoutMs` accepted.
pub const MAX_WAIT_MS: u32 = 60_000;
/// The most characters of the last reply `session_wait` returns.
pub const MAX_REPLY_CHARS: usize = 4_000;

/// Where a new session's work happens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    /// The project's folder, or the existing worktree `worktreeCwd`.
    #[default]
    Current,
    /// A new git worktree, branched from `worktreeBase` (default `HEAD`).
    Worktree,
}

/// Where a new session opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// Its own tab.
    #[default]
    Tab,
    /// A pane right of `besideSessionId`.
    Right,
    /// A pane below `besideSessionId`.
    Down,
}

/// `session_start`: a new agent session in a local project.
#[derive(Debug)]
pub struct SessionStart;

/// The new session's work and settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionStartInput {
    /// What the agent should do (at most 240,000 bytes).
    pub prompt: String,
    /// A local project's path or key; the current project when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The provider, for example `claude` or `codex`; the app's default when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// A model id from `model_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The model's reasoning effort, for models that offer one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Other model settings by id, as `model_list`'s models allow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<BTreeMap<String, String>>,
    /// The access mode; the app's default for new chats when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<AccessMode>,
    /// `current` (default) or `worktree`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_mode: Option<WorkspaceMode>,
    /// An existing worktree of the project to work in (`current` mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    /// The branch or commit a new worktree starts from (`worktree` mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_base: Option<String>,
    /// Save the prompt as an unsent draft for the person instead of sending it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
    /// Show the new session in the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reveal: Option<bool>,
    /// `tab` (default), `right` or `down`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
    /// The open session pane to place beside; the project's focused one when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beside_session_id: Option<i64>,
}

/// The started session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionStartOutput {
    /// The new session's id.
    pub session_id: i64,
    /// Its project.
    pub project: String,
    /// Its provider.
    pub harness: String,
    /// Its model, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Its reasoning effort, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Its access mode.
    pub access: AccessMode,
    /// The worktree it works in, when not the project's folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    /// Whether the prompt was sent.
    pub submitted: bool,
    /// Whether the prompt was saved as a draft instead.
    pub draft: bool,
}

impl Operation for SessionStart {
    const NAME: &'static str = "session_start";
    const TITLE: &'static str = "Start session";
    const SUBJECT: &'static [&'static str] = &["prompt", "project"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Start a new agent session in a local MonoCode project with a prompt, optionally choosing the harness, model, effort, access mode and a new or existing worktree. Pass a `requestId` so a retry returns the same session. Follow it with session_wait.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = SessionStartInput;
    type Output = SessionStartOutput;

    fn summary(output: &SessionStartOutput) -> String {
        format!(
            "Started session {} ({}{}, {}) in {}{}",
            output.session_id,
            output.harness,
            output
                .model
                .as_deref()
                .map_or(String::new(), |m| format!(" {m}")),
            output.access.id(),
            output.project,
            if output.draft {
                "; prompt saved as a draft"
            } else {
                ""
            }
        )
    }
}

/// How a message reaches a session that is working.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SendMode {
    /// Send after the current turn.
    #[default]
    Queue,
    /// Hand it to the running turn, for harnesses that support it.
    Steer,
}

/// What happened to a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// It started a turn.
    Started,
    /// It waits in the queue.
    Queued,
    /// It is handed to the running turn.
    Steered,
}

/// `session_send`: a follow-up message.
#[derive(Debug)]
pub struct SessionSend;

/// The message.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionSendInput {
    /// The session's id.
    pub session_id: i64,
    /// The message (at most 240,000 bytes).
    pub prompt: String,
    /// While a turn runs: `queue` (default) or `steer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SendMode>,
}

/// Where the message went.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionSendOutput {
    /// The session's id.
    pub session_id: i64,
    /// Started, queued or steered.
    pub delivery: Delivery,
    /// Messages now queued.
    pub queue_length: u32,
}

impl Operation for SessionSend {
    const NAME: &'static str = "session_send";
    const TITLE: &'static str = "Send to session";
    const SUBJECT: &'static [&'static str] = &["prompt", "sessionId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Send a follow-up message to a session. An idle session starts a turn; a working one queues it, or with mode `steer` hands it to the running turn when the harness supports that. Pass a `requestId` so a retry is not sent twice.";
    const HINTS: Hints = Hints::CHANGE;
    const LOCAL_ONLY: bool = true;
    type Input = SessionSendInput;
    type Output = SessionSendOutput;

    fn target_session(input: &SessionSendInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionSendOutput) -> String {
        let delivery = match output.delivery {
            Delivery::Started => "started a turn",
            Delivery::Queued => "queued",
            Delivery::Steered => "steering the running turn",
        };
        format!(
            "Message to session {} {delivery}; {} queued",
            output.session_id, output.queue_length
        )
    }
}

/// `session_wait`: wait until a session settles.
#[derive(Debug)]
pub struct SessionWait;

/// Which session, and for how long.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionWaitInput {
    /// The session's id.
    pub session_id: i64,
    /// How long to wait in milliseconds (0 to 60000, default 30000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
}

/// How the wait ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionWaitOutput {
    /// The session's id.
    pub session_id: i64,
    /// What it is doing now.
    pub status: SessionStatus,
    /// The status line the app shows for it.
    pub status_text: String,
    /// Whether the time ran out before it settled.
    pub timed_out: bool,
    /// The agent's last reply since the person's last message, cut to 4000 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<Text>,
}

impl Operation for SessionWait {
    const NAME: &'static str = "session_wait";
    const TITLE: &'static str = "Wait for session";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Wait until a session is idle, needs the person (an approval or a question), or failed, for at most `timeoutMs` (default 30 s, at most 60 s). Returns its status, whether the time ran out, and the agent's last reply. Call again to keep waiting.";
    const HINTS: Hints = Hints::READ;
    const NOT_ON_SELF: bool = true;
    type Input = SessionWaitInput;
    type Output = SessionWaitOutput;

    fn target_session(input: &SessionWaitInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionWaitOutput) -> String {
        format!(
            "Session {} is {}{}{}",
            output.session_id,
            output.status.id(),
            if output.timed_out {
                " (timed out; call again to keep waiting)"
            } else {
                ""
            },
            output
                .reply
                .as_ref()
                .map_or(String::new(), |r| format!(": {}", one_line(&r.text)))
        )
    }
}

/// The first line of `text`, at most 160 characters.
fn one_line(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let mut out: String = line.trim().chars().take(160).collect();
    if line.trim().chars().count() > 160 || text.trim().lines().count() > 1 {
        out.push('…');
    }
    out
}

/// `session_stop`: stop the running turn and pause the queue.
#[derive(Debug)]
pub struct SessionStop;

/// The stopped session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionStopOutput {
    /// The session's id.
    pub session_id: i64,
    /// Whether a turn was running.
    pub was_busy: bool,
    /// What it is doing now.
    pub status: SessionStatus,
}

impl Operation for SessionStop {
    const NAME: &'static str = "session_stop";
    const TITLE: &'static str = "Stop session";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Stop a session's running turn and pause its queued messages so they wait for the person. Repeating it changes nothing more.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    const NOT_ON_SELF: bool = true;
    const LOCAL_ONLY: bool = true;
    type Input = SessionInput;
    type Output = SessionStopOutput;

    fn target_session(input: &SessionInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionStopOutput) -> String {
        format!(
            "Session {} {}; now {}",
            output.session_id,
            if output.was_busy {
                "stopped"
            } else {
                "was not running"
            },
            output.status.id()
        )
    }
}
