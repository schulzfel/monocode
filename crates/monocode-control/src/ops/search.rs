//! Searching saved conversations and a project's files.
use crate::{Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The longest query accepted, in characters.
pub const MAX_QUERY_CHARS: usize = 200;
/// Session hits unless `limit` says otherwise.
pub const DEFAULT_SESSION_HITS: u32 = 20;
/// The largest session `limit` accepted.
pub const MAX_SESSION_HITS: u32 = 100;
/// File matches unless `limit` says otherwise.
pub const DEFAULT_FILE_MATCHES: u32 = 50;
/// The largest file `limit` accepted.
pub const MAX_FILE_MATCHES: u32 = 200;
/// The most characters of a match's preview.
pub const MAX_PREVIEW_CHARS: usize = 300;
/// How long a project search may run before it stops with what it found.
pub const PROJECT_SEARCH_MS: u64 = 10_000;

/// `search_sessions`: find conversations by title or message text.
#[derive(Debug)]
pub struct SearchSessions;

/// What to find, where.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchSessionsInput {
    /// The text to find (1 to 200 characters), case-insensitively.
    pub query: String,
    /// Only sessions of this project (path or key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Hits, 1 to 100 (default 20).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// What matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HitKind {
    /// The session's title.
    Title,
    /// A message in it.
    Message,
}

/// One hit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionHit {
    /// The session.
    pub session_id: i64,
    /// Its project.
    pub project: String,
    /// Its title.
    pub title: String,
    /// What matched.
    pub kind: HitKind,
    /// The matching text around the query.
    pub snippet: String,
}

/// The hits, best first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchSessionsOutput {
    /// The hits.
    pub hits: Vec<SessionHit>,
    /// Whether more matched than are listed.
    pub truncated: bool,
}

impl Operation for SearchSessions {
    const NAME: &'static str = "search_sessions";
    const TITLE: &'static str = "Search sessions";
    const SUBJECT: &'static [&'static str] = &["query"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Search saved conversations by title and message text, as the app's search does; archived sessions are left out. Read a hit with session_read.";
    const HINTS: Hints = Hints::READ;
    type Input = SearchSessionsInput;
    type Output = SearchSessionsOutput;

    fn summary(output: &SearchSessionsOutput) -> String {
        format!(
            "{} hits{}",
            output.hits.len(),
            if output.truncated {
                " (more matched)"
            } else {
                ""
            }
        )
    }
}

/// `search_project`: find text in a project's files.
#[derive(Debug)]
pub struct SearchProject;

/// What to find, and how.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchProjectInput {
    /// The text or pattern to find (1 to 200 characters).
    pub query: String,
    /// A local project's path or key; when absent, the calling agent
    /// session's project, or for other clients the current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Match case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_sensitive: Option<bool>,
    /// Match whole words only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whole_word: Option<bool>,
    /// Treat `query` as an extended regular expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<bool>,
    /// Only files matching these comma-separated globs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<String>,
    /// Leave out files matching these comma-separated globs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<String>,
    /// Matches, 1 to 200 (default 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileMatch {
    /// The file, relative to the project.
    pub path: String,
    /// Its line, from 1.
    pub line: u32,
    /// Its column, from 1.
    pub column: u32,
    /// The line, at most 300 characters.
    pub preview: String,
}

/// The matches, in file order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchProjectOutput {
    /// The project.
    pub project: String,
    /// The matches.
    pub matches: Vec<FileMatch>,
    /// Whether more matched than are listed.
    pub truncated: bool,
    /// Whether the search stopped after 10 seconds.
    pub timed_out: bool,
}

impl Operation for SearchProject {
    const NAME: &'static str = "search_project";
    const TITLE: &'static str = "Search project";
    const SUBJECT: &'static [&'static str] = &["query"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Search the text of a local project's files (git grep where possible), as the app's project search does. Stops after 10 seconds with what it found; open a match with tab_open's path.";
    const HINTS: Hints = Hints::READ;
    type Input = SearchProjectInput;
    type Output = SearchProjectOutput;

    fn project_slot(input: &mut SearchProjectInput) -> Option<&mut Option<String>> {
        Some(&mut input.project)
    }

    fn summary(output: &SearchProjectOutput) -> String {
        format!(
            "{} matches in {}{}",
            output.matches.len(),
            output.project,
            if output.timed_out {
                " (stopped after 10 seconds)"
            } else if output.truncated {
                " (more matched)"
            } else {
                ""
            }
        )
    }
}
