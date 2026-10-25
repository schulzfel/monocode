//! Monos: the app's standing assistants. Clients can list them, open one's
//! tab and message its chat; memory, soul, habits and artifacts stay inside.
use crate::{Hints, Operation, Scope, ops::dispatch::MAX_PROMPT_BYTES};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A Mono as lists show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonoSummary {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The projects it works in.
    pub projects: Vec<String>,
    /// Whether its chat is answering or has messages waiting.
    pub busy: bool,
}

/// `mono_list`: every Mono.
#[derive(Debug)]
pub struct MonoList;

/// No arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoListInput {}

/// The Monos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonoListOutput {
    /// In the app's order.
    pub monos: Vec<MonoSummary>,
}

impl Operation for MonoList {
    const NAME: &'static str = "mono_list";
    const TITLE: &'static str = "List Monos";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "List the Monos (the app's standing assistants): id, name, the projects each works in and whether its chat is busy.";
    const HINTS: Hints = Hints::READ;
    type Input = MonoListInput;
    type Output = MonoListOutput;

    fn summary(output: &MonoListOutput) -> String {
        format!("{} Monos", output.monos.len())
    }
}

/// One Mono.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoInput {
    /// The Mono's id, from `mono_list`.
    pub mono_id: String,
}

/// `mono_open`: show a Mono's tab.
#[derive(Debug)]
pub struct MonoOpen;

/// The opened Mono.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonoOpenOutput {
    /// The Mono's id.
    pub mono_id: String,
    /// Its name.
    pub name: String,
}

impl Operation for MonoOpen {
    const NAME: &'static str = "mono_open";
    const TITLE: &'static str = "Open Mono";
    const SUBJECT: &'static [&'static str] = &["monoId"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Open a Mono's tab in the window and show its chat.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = MonoInput;
    type Output = MonoOpenOutput;

    fn summary(output: &MonoOpenOutput) -> String {
        format!("Opened {}", output.name)
    }
}

/// `mono_send`: message a Mono's chat.
#[derive(Debug)]
pub struct MonoSend;

/// The Mono and the message.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoSendInput {
    /// The Mono's id.
    pub mono_id: String,
    /// The message (at most 240,000 bytes).
    pub text: String,
}

/// The sent message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonoSendOutput {
    /// The Mono's id.
    pub mono_id: String,
    /// Its name.
    pub name: String,
}

/// The longest message accepted, in bytes.
pub const MAX_MONO_MESSAGE_BYTES: usize = MAX_PROMPT_BYTES;

impl Operation for MonoSend {
    const NAME: &'static str = "mono_send";
    const TITLE: &'static str = "Send to Mono";
    const SUBJECT: &'static [&'static str] = &["text", "monoId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Send a message to a Mono's chat, as typing it in that chat does. Refused with session_busy while the Mono is answering or has messages waiting.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = MonoSendInput;
    type Output = MonoSendOutput;

    fn summary(output: &MonoSendOutput) -> String {
        format!("Sent the message to {}", output.name)
    }
}
