//! A Mono's artifacts: the documents, HTML pages and Markdown it saves for
//! the person, shown as cards in its chat and overview. Only the Mono's chat
//! and habit runs call these, on the calling Mono's artifacts.
use crate::{Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Artifacts listed per page unless `limit` says otherwise.
pub const DEFAULT_ARTIFACTS_PER_PAGE: u32 = 50;
/// The largest `limit` accepted.
pub const MAX_ARTIFACTS_PER_PAGE: u32 = 200;
/// The longest title accepted, in bytes.
pub const MAX_ARTIFACT_TITLE_BYTES: usize = 512;
/// The longest body accepted, in bytes.
pub const MAX_ARTIFACT_BODY_BYTES: usize = 512 * 1024;
/// The most characters of a summary kept; the rest is cut.
pub const MAX_ARTIFACT_SUMMARY_CHARS: usize = 4_000;

/// What an artifact's body is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// A document, written in Markdown.
    #[default]
    Document,
    /// An HTML page.
    Html,
    /// Markdown text.
    Markdown,
}

/// An artifact with its body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    /// Its id.
    pub id: String,
    /// What its body is.
    pub kind: ArtifactKind,
    /// Its title.
    pub title: String,
    /// Its summary, shown on its card.
    pub summary: String,
    /// Its body.
    pub body: String,
    /// When it was first written, in Unix milliseconds.
    pub created_at: u64,
    /// When it was last written.
    pub updated_at: u64,
}

/// An artifact as lists show it: everything but its body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSummary {
    /// Its id, for `mono_artifact_read`.
    pub id: String,
    /// What its body is.
    pub kind: ArtifactKind,
    /// Its title.
    pub title: String,
    /// Its summary.
    pub summary: String,
    /// When it was first written, in Unix milliseconds.
    pub created_at: u64,
    /// When it was last written.
    pub updated_at: u64,
}

/// `mono_artifact_list`: the Mono's artifacts, without their bodies.
#[derive(Debug)]
pub struct MonoArtifactList;

/// Which artifacts, and one page of them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoArtifactListInput {
    /// Only artifacts of this kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ArtifactKind>,
    /// Artifacts per page, 1 to 200 (default 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// The `nextCursor` of the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// One page of artifacts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonoArtifactListOutput {
    /// In the order they were first written.
    pub artifacts: Vec<ArtifactSummary>,
    /// How many there are of the kind asked for, on every page.
    pub total: u32,
    /// Pass as `cursor` for the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl Operation for MonoArtifactList {
    const NAME: &'static str = "mono_artifact_list";
    const TITLE: &'static str = "List artifacts";
    const SUBJECT: &'static [&'static str] = &["kind"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "List your artifacts (documents, HTML pages and Markdown you saved), optionally of one kind: id, kind, title, summary and when each was written, without bodies; mono_artifact_read returns one with its body. Paged.";
    const HINTS: Hints = Hints::READ;
    const MONO_ONLY: bool = true;
    type Input = MonoArtifactListInput;
    type Output = MonoArtifactListOutput;

    fn summary(output: &MonoArtifactListOutput) -> String {
        format!("{} of {} artifacts", output.artifacts.len(), output.total)
    }
}

/// `mono_artifact_read`: one artifact with its body.
#[derive(Debug)]
pub struct MonoArtifactRead;

/// One artifact.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactInput {
    /// The artifact's id, from `mono_artifact_list`.
    pub id: String,
}

impl Operation for MonoArtifactRead {
    const NAME: &'static str = "mono_artifact_read";
    const TITLE: &'static str = "Read artifact";
    const SUBJECT: &'static [&'static str] = &["id"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Read one of your artifacts with its body.";
    const HINTS: Hints = Hints::READ;
    const MONO_ONLY: bool = true;
    type Input = ArtifactInput;
    type Output = Artifact;

    fn summary(output: &Artifact) -> String {
        format!("“{}”, {} bytes", output.title, output.body.len())
    }
}

/// `mono_artifact_write`: an artifact saved, and its card posted.
#[derive(Debug)]
pub struct MonoArtifactWrite;

/// The artifact.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonoArtifactWriteInput {
    /// The artifact to replace, keeping when it was first written; a new
    /// one is saved when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What its body is (default `document`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ArtifactKind>,
    /// Its title (at most 512 bytes).
    pub title: String,
    /// Its complete body (at most 512 KiB).
    pub body: String,
    /// A summary for its card; cut to 4,000 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

impl Operation for MonoArtifactWrite {
    const NAME: &'static str = "mono_artifact_write";
    const TITLE: &'static str = "Write artifact";
    const SUBJECT: &'static [&'static str] = &["title", "id"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Save a substantial report as an artifact (a document, HTML page or Markdown) with a title, summary and complete body, and post its card in your chat. Writing an existing id replaces it.";
    const HINTS: Hints = Hints::CHANGE;
    const MONO_ONLY: bool = true;
    type Input = MonoArtifactWriteInput;
    type Output = Artifact;

    fn summary(output: &Artifact) -> String {
        format!("Saved “{}”", output.title)
    }
}
