//! A session's context and its end: compact it, hand its last response to a
//! new session, delete it; and removing a project from the app.
use crate::{
    AccessMode, Hints, Operation, Scope,
    ops::{rail::ProjectTarget, session::SessionInput},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `session_compact`: compact a session's context window.
#[derive(Debug)]
pub struct SessionCompact;

/// The session whose context is compacting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionCompactOutput {
    /// The session's id.
    pub session_id: i64,
    /// Whether compaction started; follow it with `session_wait`.
    pub compacting: bool,
}

impl Operation for SessionCompact {
    const NAME: &'static str = "session_compact";
    const TITLE: &'static str = "Compact session";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Compact an idle session's context window, as its /compact command does. Only harnesses whose native transport supports compaction accept it; a session must have sent a message first. Follow it with session_wait.";
    const HINTS: Hints = Hints::CHANGE;
    const LOCAL_ONLY: bool = true;
    type Input = SessionInput;
    type Output = SessionCompactOutput;

    fn target_session(input: &SessionInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionCompactOutput) -> String {
        format!("Session {} is compacting its context", output.session_id)
    }
}

/// `session_handoff`: a new session that continues from another's last response.
#[derive(Debug)]
pub struct SessionHandoff;

/// The session to hand off and where to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionHandoffInput {
    /// The session whose last response is handed off.
    pub session_id: i64,
    /// The new session's provider, for example `claude` or `codex`.
    pub harness: String,
    /// A model id from `model_list`; the provider's default when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The model's reasoning effort, for models that offer one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Other model settings by id, as `model_list`'s models allow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<BTreeMap<String, String>>,
    /// The new session's access mode; the app's default for new chats when
    /// absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<AccessMode>,
}

/// The new session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionHandoffOutput {
    /// The session handed off.
    pub from_session_id: i64,
    /// The new session's id.
    pub session_id: i64,
    /// Its provider.
    pub harness: String,
    /// Its model, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Its access mode.
    pub access: AccessMode,
}

impl Operation for SessionHandoff {
    const NAME: &'static str = "session_handoff";
    const TITLE: &'static str = "Hand off session";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Hand a session's last response to a new session on another harness or model, as the response's Hand off action does. The new session starts in the same project and worktree with the handoff brief and waits for its first message; it gets the access mode asked for, or the app's default for new chats.";
    const HINTS: Hints = Hints::CHANGE;
    const LOCAL_ONLY: bool = true;
    type Input = SessionHandoffInput;
    type Output = SessionHandoffOutput;

    fn target_session(input: &SessionHandoffInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionHandoffOutput) -> String {
        format!(
            "Handed session {} off to session {} ({}{}, {})",
            output.from_session_id,
            output.session_id,
            output.harness,
            output
                .model
                .as_deref()
                .map_or(String::new(), |m| format!(" {m}")),
            output.access.id()
        )
    }
}

/// `session_delete`: delete a session permanently.
#[derive(Debug)]
pub struct SessionDelete;

/// The deleted session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionDeleteOutput {
    /// The session's id.
    pub session_id: i64,
    /// Its title.
    pub title: String,
}

impl Operation for SessionDelete {
    const NAME: &'static str = "session_delete";
    const TITLE: &'static str = "Delete session";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Destructive;
    const DESCRIPTION: &'static str = "Delete a session and its saved settings permanently, after the person confirms in MonoCode. A running turn is stopped first. Mono chats and orchestration workers are refused. Declining, or no answer within two minutes, fails with user_declined and changes nothing.";
    const HINTS: Hints = Hints::DISCARD;
    const CONFIRM: bool = true;
    const NOT_ON_SELF: bool = true;
    const LOCAL_ONLY: bool = true;
    type Input = SessionInput;
    type Output = SessionDeleteOutput;

    fn target_session(input: &SessionInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &SessionDeleteOutput) -> String {
        format!("Deleted session {} “{}”", output.session_id, output.title)
    }
}

/// `project_delete`: remove a project and its sessions from the app.
#[derive(Debug)]
pub struct ProjectDelete;

/// The removed project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDeleteOutput {
    /// The project's path; its folder on disk is untouched.
    pub project: String,
    /// How many of its saved sessions were deleted.
    pub sessions_deleted: u32,
}

impl Operation for ProjectDelete {
    const NAME: &'static str = "project_delete";
    const TITLE: &'static str = "Remove project";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Destructive;
    const DESCRIPTION: &'static str = "Remove a local project from MonoCode and delete its saved sessions, after the person confirms in MonoCode. An agent session is not asked to confirm, but cannot remove its own project or one with an orchestration's workers. Files on disk are never deleted: the folder stays, and adding it again brings the project back empty. Declining, or no answer within two minutes, fails with user_declined and changes nothing.";
    const HINTS: Hints = Hints::DISCARD;
    const CONFIRM: bool = true;
    type Input = ProjectTarget;
    type Output = ProjectDeleteOutput;

    fn summary(output: &ProjectDeleteOutput) -> String {
        format!(
            "Removed {} from MonoCode and deleted {} sessions; the folder is untouched",
            output.project, output.sessions_deleted
        )
    }
}
