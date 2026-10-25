//! A session's follow-up queue: messages waiting to be sent after the
//! running turn.
use crate::{Hints, Operation, Scope, ops::session::SessionInput};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The most characters of a queued message listed.
pub const MAX_QUEUED_TEXT_CHARS: usize = 2_000;

/// A queued message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueuedMessage {
    /// Its id, for the other queue tools.
    pub id: String,
    /// Its text, at most 2,000 characters.
    pub text: String,
    /// Whether `text` was cut short.
    pub truncated: bool,
    /// Files attached to it.
    pub attachments: u32,
    /// The mode it runs in, such as `plan`, when not a plain message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Whether it is marked to steer the running turn.
    pub steer: bool,
    /// Whether it is being sent now; it cannot be changed then.
    pub sending: bool,
    /// Why sending it failed, if it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `queue_list`: a session's queued messages.
#[derive(Debug)]
pub struct QueueList;

/// A session's queue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueueListOutput {
    /// The session.
    pub session_id: i64,
    /// Whether the queue is paused, after the person stopped a turn or a
    /// message failed; `queue_resume` releases it.
    pub paused: bool,
    /// The messages, in sending order.
    pub messages: Vec<QueuedMessage>,
}

impl Operation for QueueList {
    const NAME: &'static str = "queue_list";
    const TITLE: &'static str = "List queued messages";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "List the messages queued in a session to send after its running turn, in order, and whether the queue is paused.";
    const HINTS: Hints = Hints::READ;
    type Input = SessionInput;
    type Output = QueueListOutput;

    fn target_session(input: &SessionInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &QueueListOutput) -> String {
        format!(
            "{} queued in session {}{}",
            output.messages.len(),
            output.session_id,
            if output.paused { " (paused)" } else { "" }
        )
    }
}

/// One queued message.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueItemInput {
    /// The session.
    pub session_id: i64,
    /// The message's id from `queue_list`.
    pub message_id: String,
}

/// The queue after the change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueueOutput {
    /// The session.
    pub session_id: i64,
    /// Messages still queued.
    pub queue_length: u32,
    /// Whether the queue is paused.
    pub paused: bool,
}

fn queue_summary(verb: &str, output: &QueueOutput) -> String {
    format!(
        "{verb} in session {}; {} queued{}",
        output.session_id,
        output.queue_length,
        if output.paused { " (paused)" } else { "" }
    )
}

/// `queue_remove`: discard a queued message unsent.
#[derive(Debug)]
pub struct QueueRemove;

impl Operation for QueueRemove {
    const NAME: &'static str = "queue_remove";
    const TITLE: &'static str = "Remove queued message";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Destructive;
    const DESCRIPTION: &'static str = "Discard a queued message so it is never sent, after the person confirms in MonoCode. A message being sent now cannot be removed. Declining, or no answer within two minutes, fails with user_declined and changes nothing.";
    const HINTS: Hints = Hints::DISCARD;
    const CONFIRM: bool = true;
    const LOCAL_ONLY: bool = true;
    type Input = QueueItemInput;
    type Output = QueueOutput;

    fn target_session(input: &QueueItemInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &QueueOutput) -> String {
        queue_summary("Message removed", output)
    }
}

/// `queue_steer`: send a queued message into the running turn.
#[derive(Debug)]
pub struct QueueSteer;

impl Operation for QueueSteer {
    const NAME: &'static str = "queue_steer";
    const TITLE: &'static str = "Steer with queued message";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Send a queued message into the session's running turn now instead of after it, for providers that can take one (see session_send's steer mode).";
    const HINTS: Hints = Hints::CHANGE;
    const LOCAL_ONLY: bool = true;
    type Input = QueueItemInput;
    type Output = QueueOutput;

    fn target_session(input: &QueueItemInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &QueueOutput) -> String {
        queue_summary("Message steering", output)
    }
}

/// `queue_resume`: release a paused queue.
#[derive(Debug)]
pub struct QueueResume;

impl Operation for QueueResume {
    const NAME: &'static str = "queue_resume";
    const TITLE: &'static str = "Resume queue";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Resume a paused queue, as the app's Resume or Retry button does: the interrupted turn continues and the queued messages follow.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    const LOCAL_ONLY: bool = true;
    type Input = SessionInput;
    type Output = QueueOutput;

    fn target_session(input: &SessionInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &QueueOutput) -> String {
        queue_summary("Queue resumed", output)
    }
}

/// `queue_edit`: change a queued message's text.
#[derive(Debug)]
pub struct QueueEdit;

/// The message and its new text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueEditInput {
    /// The session.
    pub session_id: i64,
    /// The message's id from `queue_list`.
    pub message_id: String,
    /// Its new text (at most 240,000 bytes); empty only when it has attachments.
    pub text: String,
}

impl Operation for QueueEdit {
    const NAME: &'static str = "queue_edit";
    const TITLE: &'static str = "Edit queued message";
    const SUBJECT: &'static [&'static str] = &["text", "sessionId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Replace a queued message's text before it is sent; the text it had is gone. Not while it is being sent or the person is editing it.";
    const HINTS: Hints = Hints::DISCARD;
    const LOCAL_ONLY: bool = true;
    type Input = QueueEditInput;
    type Output = QueueOutput;

    fn target_session(input: &QueueEditInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &QueueOutput) -> String {
        queue_summary("Message edited", output)
    }
}
