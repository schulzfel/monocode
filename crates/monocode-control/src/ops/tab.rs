//! Title tabs and panes: open, focus, close.
use crate::{Hints, Operation, Scope, ops::dispatch::Placement};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// An app view with its own tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AppView {
    /// Pull requests and issues.
    Inbox,
    /// Notes.
    Notes,
    /// Automations.
    Automations,
}

/// `tab_open`: show a session, a file, an app view or search.
#[derive(Debug)]
pub struct TabOpen;

/// What to open: exactly one of `sessionId`, `path`, `view` or `search`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TabOpenInput {
    /// A session of any local project; its project becomes current.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<i64>,
    /// A file inside a local project, absolute or relative to the current
    /// project, optionally with `:line` or `:line:column`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `inbox`, `notes` or `automations`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<AppView>,
    /// Open search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<bool>,
    /// For a session: `tab` (default), `right` or `down`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
    /// For a session placed `right` or `down`: the open session pane to place
    /// beside; the focused one when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beside_session_id: Option<i64>,
}

/// What is shown now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabOutput {
    /// The active tab's id, as `workspace_read` reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    /// The focused pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<i64>,
    /// The current project.
    pub project: String,
}

impl Operation for TabOpen {
    const NAME: &'static str = "tab_open";
    const TITLE: &'static str = "Open tab";
    const SUBJECT: &'static [&'static str] = &["path", "sessionId", "view"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Show something in MonoCode's window: a session (`sessionId`, optionally placed `right` or `down` beside another pane), a file inside a local project (`path`, with an optional `:line:column`), the Inbox, Notes or Automations (`view`), or search.";
    const HINTS: Hints = Hints::CHANGE;
    const LOCAL_ONLY: bool = true;
    type Input = TabOpenInput;
    type Output = TabOutput;

    fn target_session(input: &TabOpenInput) -> Option<i64> {
        input.session_id
    }

    fn summary(output: &TabOutput) -> String {
        summary(output)
    }
}

/// A title tab or a pane: exactly one of `tab` or `pane`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TabTarget {
    /// A tab id from `workspace_read`, such as `session:12` or `app:notes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    /// A pane id from `workspace_read`'s pane tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<i64>,
}

/// `tab_focus`: make a tab active or focus a pane.
#[derive(Debug)]
pub struct TabFocus;

impl Operation for TabFocus {
    const NAME: &'static str = "tab_focus";
    const TITLE: &'static str = "Focus tab";
    const SUBJECT: &'static [&'static str] = &["tab", "pane"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Make a title tab active (`tab`, an id from workspace_read) or focus a pane of a split (`pane`).";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = TabTarget;
    type Output = TabOutput;

    fn summary(output: &TabOutput) -> String {
        summary(output)
    }
}

/// `tab_close`: close a title tab or one pane of a split.
#[derive(Debug)]
pub struct TabClose;

/// What was closed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabCloseOutput {
    /// Whether something closed; `false` when it was already gone or is the
    /// last empty tab, which stays.
    pub closed: bool,
    /// What is shown now.
    pub now: TabOutput,
}

impl Operation for TabClose {
    const NAME: &'static str = "tab_close";
    const TITLE: &'static str = "Close tab";
    const SUBJECT: &'static [&'static str] = &["tab", "pane"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Close a title tab (`tab`) or one pane of a split (`pane`). Closing never stops or deletes a session; it stays in the sidebar.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = TabTarget;
    type Output = TabCloseOutput;

    fn summary(output: &TabCloseOutput) -> String {
        format!(
            "{}; {}",
            if output.closed {
                "Closed"
            } else {
                "Nothing to close"
            },
            summary(&output.now)
        )
    }
}

fn summary(output: &TabOutput) -> String {
    format!(
        "Showing {} in {}",
        output.tab.as_deref().unwrap_or("no tab"),
        output.project
    )
}
