//! A Mono's soul: `SOUL.md`, its standing instructions. Only the Mono's chat
//! and habit runs call these, on the calling Mono's soul.
use crate::{
    Hints, Operation, Scope,
    ops::mono_memory::{FileHash, MonoFile},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The longest soul accepted, in bytes.
pub const MAX_SOUL_BYTES: usize = 128 * 1024;
/// The longest `expectedHash` accepted, in bytes.
pub const MAX_HASH_BYTES: usize = 128;

/// `mono_soul_read`: the Mono's soul as it is now.
#[derive(Debug)]
pub struct MonoSoulRead;

/// No arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoSoulReadInput {}

impl Operation for MonoSoulRead {
    const NAME: &'static str = "mono_soul_read";
    const TITLE: &'static str = "Read soul";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Read your soul (SOUL.md, your standing instructions) and the hash of its text, which mono_soul_update needs.";
    const HINTS: Hints = Hints::READ;
    const MONO_ONLY: bool = true;
    type Input = MonoSoulReadInput;
    type Output = MonoFile;

    fn summary(output: &MonoFile) -> String {
        match &output.text {
            Some(text) => format!("{} lines of soul", text.lines().count()),
            None => "No soul yet".into(),
        }
    }
}

/// `mono_soul_update`: the soul replaced, unless it changed since it was read.
#[derive(Debug)]
pub struct MonoSoulUpdate;

/// The new soul, and the hash of the text it replaces.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoSoulUpdateInput {
    /// The whole new soul (at most 128 KiB; empty clears it).
    pub text: String,
    /// The `hash` `mono_soul_read` returned (at most 128 bytes).
    pub expected_hash: String,
}

impl Operation for MonoSoulUpdate {
    const NAME: &'static str = "mono_soul_update";
    const TITLE: &'static str = "Update soul";
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Replace your soul (SOUL.md) with new text, only when asked. It is saved only over the text whose hash `expectedHash` names: if it changed since you read it, the call fails with request_conflict (\"conflict\"); read it again first.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    const MONO_ONLY: bool = true;
    type Input = MonoSoulUpdateInput;
    type Output = FileHash;

    fn summary(_output: &FileHash) -> String {
        "Updated the soul".into()
    }
}
