//! Changing an idle session's settings, and leaving it a draft.
use crate::{AccessMode, Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `session_configure`: an idle session's model, effort and access.
#[derive(Debug)]
pub struct SessionConfigure;

/// What to change; absent fields stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionConfigureInput {
    /// The session's id.
    pub session_id: i64,
    /// A model id of the session's provider, from `model_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The reasoning effort, for models that offer one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Other model settings by id, as `model_list`'s models allow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<BTreeMap<String, String>>,
    /// The access mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<AccessMode>,
}

/// The session's settings after the change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfigureOutput {
    /// The session's id.
    pub session_id: i64,
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
}

impl Operation for SessionConfigure {
    const NAME: &'static str = "session_configure";
    const TITLE: &'static str = "Configure session";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Change an idle session's model, reasoning effort, other model settings or access mode; its next turn uses them. Fails with session_busy while a turn runs.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    const NOT_ON_SELF: bool = true;
    const LOCAL_ONLY: bool = true;
    type Input = SessionConfigureInput;
    type Output = SessionConfigureOutput;

    fn target_session(input: &SessionConfigureInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionConfigureOutput) -> String {
        format!(
            "Session {}: {} {}{}, {}",
            output.session_id,
            output.harness,
            output.model.as_deref().unwrap_or("default model"),
            output
                .effort
                .as_deref()
                .map_or_else(String::new, |e| format!(" ({e} effort)")),
            output.access.id()
        )
    }
}

/// `session_draft`: an unsent message for the person to review.
#[derive(Debug)]
pub struct SessionDraft;

/// The session and the draft's text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDraftInput {
    /// The session's id.
    pub session_id: i64,
    /// The message (at most 240,000 bytes).
    pub text: String,
}

/// The saved draft.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionDraftOutput {
    /// The session's id.
    pub session_id: i64,
    /// Whether the draft was saved in the session.
    pub saved: bool,
}

impl Operation for SessionDraft {
    const NAME: &'static str = "session_draft";
    const TITLE: &'static str = "Save draft";
    const SUBJECT: &'static [&'static str] = &["text", "sessionId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Leave an unsent message in an idle session's composer for the person to review and send. Nothing is sent. Fails with session_not_idle when the person already has an unsent draft there, which is kept.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    const NOT_ON_SELF: bool = true;
    const LOCAL_ONLY: bool = true;
    type Input = SessionDraftInput;
    type Output = SessionDraftOutput;

    fn target_session(input: &SessionDraftInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionDraftOutput) -> String {
        if output.saved {
            format!("Draft saved in session {}", output.session_id)
        } else {
            format!("Draft not saved in session {}", output.session_id)
        }
    }
}
