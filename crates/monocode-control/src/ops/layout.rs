//! Arranging the window: moving tabs and panes, and the layout's settings.
use crate::{
    Hints, Operation, Scope,
    ops::{tab::TabOutput, workspace::LayoutStyle},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A side of a pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    /// Left of it.
    Left,
    /// Right of it.
    #[default]
    Right,
    /// Above it.
    Top,
    /// Below it.
    Bottom,
}

/// `tab_move`: move a title tab or a pane.
#[derive(Debug)]
pub struct TabMove;

/// What moves and where. With `besideSessionId` the session's whole title
/// tab (or the pane, when it is one of a split) goes beside that pane;
/// without it, a pane of a split becomes its own tab at `index`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TabMoveInput {
    /// The session whose tab or pane moves; it must be open.
    pub session_id: i64,
    /// An open pane of another tab or of the same split to place it beside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beside_session_id: Option<i64>,
    /// Which side of `besideSessionId` (default `right`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<Edge>,
    /// Without `besideSessionId`: the tab position, from 0 (default last).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

impl Operation for TabMove {
    const NAME: &'static str = "tab_move";
    const TITLE: &'static str = "Move tab";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Move a session's title tab, with all its panes, beside another pane (besideSessionId and edge), or detach a pane of a split into its own tab (no besideSessionId; optional index). Use workspace_read for the open sessions.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = TabMoveInput;
    type Output = TabOutput;

    fn target_session(input: &TabMoveInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &TabOutput) -> String {
        moved("Moved", output)
    }
}

fn moved(verb: &str, output: &TabOutput) -> String {
    match output.pane {
        Some(pane) => format!("{verb}; pane {pane} has focus in {}", output.project),
        None => format!("{verb} in {}", output.project),
    }
}

/// Where a split puts the moved pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SplitSide {
    /// To the right of the pane.
    #[default]
    Right,
    /// Below the pane.
    Down,
}

/// `pane_split`: move a session into a split beside a pane.
#[derive(Debug)]
pub struct PaneSplit;

/// The session to move and the pane to split.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaneSplitInput {
    /// An existing session of the pane's project; open or not.
    pub session_id: i64,
    /// The open pane to split; the focused pane when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beside_session_id: Option<i64>,
    /// `right` (default) or `down`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<SplitSide>,
}

impl Operation for PaneSplit {
    const NAME: &'static str = "pane_split";
    const TITLE: &'static str = "Split pane";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Split a pane and show an existing session beside it, moving the session's pane there if it is open elsewhere. To start a new session in a split, use session_start with placement.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = PaneSplitInput;
    type Output = TabOutput;

    fn target_session(input: &PaneSplitInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &TabOutput) -> String {
        moved("Split", output)
    }
}

/// A tab of the Classic layout's sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SidebarTab {
    /// The project's sessions.
    Sessions,
    /// The file explorer.
    Files,
    /// Source control changes.
    Changes,
}

/// A tab of the right panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PanelTab {
    /// The file explorer.
    Files,
    /// Source control changes.
    Changes,
    /// The commit graph.
    Graph,
}

/// `layout_set`: the window's layout, set explicitly.
#[derive(Debug)]
pub struct LayoutSet;

/// What to set; absent fields stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutSetInput {
    /// `classic` or `focused`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<LayoutStyle>,
    /// The Focused layout's boxy frame instead of its rounded card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boxy: Option<bool>,
    /// The Classic layout's sidebar tab; shows the sidebar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_tab: Option<SidebarTab>,
    /// Whether the right panel is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_panel_open: Option<bool>,
    /// The right panel's tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_tab: Option<PanelTab>,
    /// Whether the primary sidebar (the project rail in Classic, the session
    /// sidebar in Focused) is shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_sidebar_visible: Option<bool>,
}

/// The layout now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSetOutput {
    /// Classic or focused.
    pub style: LayoutStyle,
    /// Whether the Focused layout uses its boxy frame.
    pub boxy: bool,
    /// The Classic layout's sidebar tab.
    pub sidebar_tab: SidebarTab,
    /// Whether the right panel is open.
    pub right_panel_open: bool,
    /// The right panel's tab, when it shows one of these.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_tab: Option<PanelTab>,
    /// Whether the primary sidebar is shown.
    pub primary_sidebar_visible: bool,
    /// Requested changes the app could not make now, with why.
    pub skipped: Vec<String>,
}

impl Operation for LayoutSet {
    const NAME: &'static str = "layout_set";
    const TITLE: &'static str = "Change layout";
    const SUBJECT: &'static [&'static str] = &[crate::SET_FIELDS];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Set the window's layout explicitly: Classic or Focused, the Focused layout's boxy frame, the Classic sidebar's tab, the right panel's open state and tab, and whether the primary sidebar shows. Changes the app cannot make now (no git repository for the right panel, say) are listed in skipped.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = LayoutSetInput;
    type Output = LayoutSetOutput;

    fn summary(output: &LayoutSetOutput) -> String {
        let style = match output.style {
            LayoutStyle::Classic => "Classic",
            LayoutStyle::Focused => "Focused",
        };
        format!(
            "{style} layout; right panel {}{}",
            if output.right_panel_open {
                "open"
            } else {
                "closed"
            },
            if output.skipped.is_empty() {
                String::new()
            } else {
                format!("; skipped: {}", output.skipped.join(", "))
            }
        )
    }
}
