//! A Mono's memory: `MEMORY.md`, its archive and its topic notes in the
//! Mono's own folder. Only the Mono's chat and habit runs call these, on the
//! calling Mono's memory; every other caller is refused.
use crate::{Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The longest fact accepted, in bytes, before redaction.
pub const MAX_FACT_BYTES: usize = 4_000;
/// The longest search accepted, in bytes.
pub const MAX_QUERY_BYTES: usize = 4_000;

/// One of the Mono's files as it is now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonoFile {
    /// Its text; `null` while the file does not exist.
    pub text: Option<String>,
    /// The SHA-256 of its text (of the empty text while it does not exist).
    pub hash: String,
}

/// A file after a change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileHash {
    /// The SHA-256 of its new text.
    pub hash: String,
}

/// `mono_memory_read`: the Mono's memory, a topic note or the archive.
#[derive(Debug)]
pub struct MonoMemoryRead;

/// Which memory file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoMemoryReadInput {
    /// A topic note's name (letters, numbers, spaces, dots or dashes), or
    /// `archive`; `MEMORY.md` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
}

impl Operation for MonoMemoryRead {
    const NAME: &'static str = "mono_memory_read";
    const TITLE: &'static str = "Read memory";
    const SUBJECT: &'static [&'static str] = &["topic"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Read your memory (MEMORY.md), one of its topic notes, or the archive of entries that no longer fit, with the hash of its text.";
    const HINTS: Hints = Hints::READ;
    const MONO_ONLY: bool = true;
    type Input = MonoMemoryReadInput;
    type Output = MonoFile;

    fn summary(output: &MonoFile) -> String {
        match &output.text {
            Some(text) => format!("{} lines of memory", text.lines().count()),
            None => "No memory yet".into(),
        }
    }
}

/// `mono_memory_search`: entries of the memory, its archive and topic notes
/// that contain every word searched for.
#[derive(Debug)]
pub struct MonoMemorySearch;

/// What to search for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoMemorySearchInput {
    /// A few specific words (at most 4,000 bytes); words under three
    /// letters and common words are ignored.
    pub query: String,
    /// Only entries dated since: `YYYY-MM-DD`, or a duration back from now
    /// such as `24h`, `7d` or `2w`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// One matching entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryHit {
    /// The file it is in, such as `MEMORY.md` or `memory/archive.md`.
    pub file: String,
    /// The entry's line.
    pub line: String,
    /// The entry's date, `YYYY-MM-DD`, when it has one.
    pub date: Option<String>,
}

/// The matching entries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonoMemorySearchOutput {
    /// MEMORY.md's first, then the archive's, then the topic notes'; at
    /// most 50. Struck-out entries are skipped.
    pub hits: Vec<MemoryHit>,
}

impl Operation for MonoMemorySearch {
    const NAME: &'static str = "mono_memory_search";
    const TITLE: &'static str = "Search memory";
    const SUBJECT: &'static [&'static str] = &["query"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Search your memory, its archive and its topic notes for entries containing every word of the query, optionally only those dated since a day or duration. At most 50 hits.";
    const HINTS: Hints = Hints::READ;
    const MONO_ONLY: bool = true;
    type Input = MonoMemorySearchInput;
    type Output = MonoMemorySearchOutput;

    fn summary(output: &MonoMemorySearchOutput) -> String {
        format!("{} memory entries found", output.hits.len())
    }
}

/// `mono_memory_add`: a dated fact appended to the memory or a topic note.
#[derive(Debug)]
pub struct MonoMemoryAdd;

/// The fact.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoMemoryAddInput {
    /// The fact: at most 4,000 bytes, then 1 to 1,000 characters once
    /// credentials are redacted and whitespace collapsed.
    pub fact: String,
    /// A topic note to keep it in instead of MEMORY.md.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// The day it stops being true, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

impl Operation for MonoMemoryAdd {
    const NAME: &'static str = "mono_memory_add";
    const TITLE: &'static str = "Add memory";
    const SUBJECT: &'static [&'static str] = &["fact"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Remember a fact: it is appended to your memory (or a topic note) as a dated entry, with credentials redacted. A fact already remembered is not added twice. MEMORY.md keeps to 200 lines and 24 KiB; older entries move to the archive.";
    const HINTS: Hints = Hints::CHANGE;
    const MONO_ONLY: bool = true;
    type Input = MonoMemoryAddInput;
    type Output = FileHash;

    fn summary(_output: &FileHash) -> String {
        "Remembered".into()
    }
}

/// `mono_memory_replace`: one live entry struck out and a new fact after it.
#[derive(Debug)]
pub struct MonoMemoryReplace;

/// The entry and the fact that replaces it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoMemoryReplaceInput {
    /// Text of exactly one live entry, quoted from the memory.
    pub find: String,
    /// The new fact, as for `mono_memory_add`.
    pub fact: String,
    /// The topic note the entry is in, instead of MEMORY.md.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// The day the new fact stops being true, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

impl Operation for MonoMemoryReplace {
    const NAME: &'static str = "mono_memory_replace";
    const TITLE: &'static str = "Replace memory";
    const SUBJECT: &'static [&'static str] = &["fact", "find"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Replace one entry of your memory: the entry `find` quotes is struck out as superseded today and the new fact is added after it. `find` must match exactly one live entry.";
    const HINTS: Hints = Hints::CHANGE;
    const MONO_ONLY: bool = true;
    type Input = MonoMemoryReplaceInput;
    type Output = FileHash;

    fn summary(_output: &FileHash) -> String {
        "Replaced the memory entry".into()
    }
}

/// `mono_memory_remove`: one live entry deleted.
#[derive(Debug)]
pub struct MonoMemoryRemove;

/// The entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoMemoryRemoveInput {
    /// Text of exactly one live entry, quoted from the memory.
    pub find: String,
    /// The topic note the entry is in, instead of MEMORY.md.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
}

impl Operation for MonoMemoryRemove {
    const NAME: &'static str = "mono_memory_remove";
    const TITLE: &'static str = "Remove memory";
    const SUBJECT: &'static [&'static str] = &["find"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Forget one entry of your memory: the live entry `find` quotes is deleted. `find` must match exactly one live entry.";
    const HINTS: Hints = Hints::DISCARD;
    const MONO_ONLY: bool = true;
    type Input = MonoMemoryRemoveInput;
    type Output = FileHash;

    fn summary(_output: &FileHash) -> String {
        "Removed the memory entry".into()
    }
}
