//! The window's layout: the title tab strip, the active tab's panes and the
//! panels around them.
use crate::{Hints, Operation, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `workspace_read`: what the window shows.
#[derive(Debug)]
pub struct WorkspaceRead;

/// `workspace_read` takes no arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReadInput {}

/// What a title tab shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TabKind {
    /// Sessions (one, or several panes in a split).
    Session,
    /// Search.
    Search,
    /// Inbox, Notes or Automations.
    App,
    /// A Mono's chat.
    Mono,
}

/// One title tab.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    /// A stable id to pass to `tab_focus` and `tab_close`: `session:<id>`,
    /// `search`, `app:inbox`, `app:notes`, `app:automations` or `mono:<id>`.
    pub id: String,
    /// What it shows.
    pub kind: TabKind,
    /// Its label.
    pub label: String,
    /// Whether it is the active tab.
    pub active: bool,
}

/// How a split divides its space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    /// Side by side.
    Right,
    /// Stacked.
    Down,
}

/// A tab's pane tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaneNode {
    /// One pane.
    Pane {
        /// The pane's id, for `tab_focus` and `tab_close`: a session id, or a
        /// negative id for a file pane.
        pane: i64,
        /// The session it shows or belongs to.
        #[serde(rename = "sessionId")]
        session_id: i64,
        /// Whether it is a file pane rather than a chat.
        file: bool,
    },
    /// Panes side by side or stacked.
    Split {
        /// How they are arranged.
        direction: SplitDirection,
        /// The panes or nested splits.
        children: Vec<PaneNode>,
        /// Each child's share of the space.
        sizes: Vec<f32>,
    },
}

/// One tab of a pane's review panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewTabInfo {
    /// `file`, `changes`, `commit` or `session`.
    pub kind: String,
    /// Its label.
    pub label: String,
    /// The checkout it belongs to.
    pub cwd: String,
    /// The file, when it shows one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// A pane's review panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaneReview {
    /// The pane.
    pub pane: i64,
    /// Its tabs in order.
    pub tabs: Vec<ReviewTabInfo>,
    /// The index of the shown tab.
    pub active: u32,
    /// Whether the review fills the pane, its chat hidden.
    pub chat_hidden: bool,
}

/// The right panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RightPanel {
    /// Whether it is open.
    pub open: bool,
    /// The tab it shows.
    pub tab: String,
}

/// The window's arrangement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayoutStyle {
    /// Separate project and session sidebars.
    Classic,
    /// One sidebar with the projects as spaces.
    Focused,
}

/// What the window shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceReadOutput {
    /// The current project's path.
    pub project: String,
    /// The title tabs in order.
    pub tabs: Vec<TabInfo>,
    /// The active tab's pane tree, when it shows sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panes: Option<PaneNode>,
    /// The focused pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_pane: Option<i64>,
    /// The review panels of the active tab's panes.
    pub reviews: Vec<PaneReview>,
    /// The right panel.
    pub right_panel: RightPanel,
    /// Classic or focused.
    pub layout: LayoutStyle,
}

impl Operation for WorkspaceRead {
    const NAME: &'static str = "workspace_read";
    const TITLE: &'static str = "Read workspace";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Show what MonoCode's window shows: the current project, the title tabs in order with stable ids for tab_focus and tab_close, the active tab's pane tree and focused pane, each pane's review tabs, the right panel and the layout.";
    const HINTS: Hints = Hints::READ;
    type Input = WorkspaceReadInput;
    type Output = WorkspaceReadOutput;

    fn summary(output: &WorkspaceReadOutput) -> String {
        let active = output
            .tabs
            .iter()
            .find(|t| t.active)
            .map_or(String::new(), |t| format!("; active: {}", t.label));
        format!("{} tabs in {}{active}", output.tabs.len(), output.project)
    }
}
