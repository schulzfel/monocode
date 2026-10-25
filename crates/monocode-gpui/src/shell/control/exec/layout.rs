//! `tab_move`, `pane_split` and `layout_set`. Moves use the pane drop's own
//! workspace steps (`place`, `place_tab`, `detach`) and motion; splits reuse
//! the launch placement (`app_place_launch`); layout uses explicit setters.
use super::*;
use crate::{
    pane_layout::Edge as PaneEdge, right_panel::RightTab, shell::classic_sidebar::ClassicSidebarTab,
};
use monocode_control::ops::{
    layout::{
        Edge, LayoutSetInput, LayoutSetOutput, PaneSplitInput, PanelTab, SidebarTab, SplitSide,
        TabMoveInput,
    },
    tab::TabOutput,
    workspace::LayoutStyle,
};

fn edge(edge: Edge) -> PaneEdge {
    match edge {
        Edge::Left => PaneEdge::Left,
        Edge::Right => PaneEdge::Right,
        Edge::Top => PaneEdge::Top,
        Edge::Bottom => PaneEdge::Bottom,
    }
}

fn not_open(id: i64) -> ControlError {
    ControlError::new(
        ErrorCode::NotFound,
        format!("Session {id} is not open in a pane; see workspace_read."),
    )
}

impl Shell {
    pub(super) fn control_tab_move(
        &mut self,
        input: &TabMoveInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<TabOutput, ControlError> {
        let id = input.session_id;
        self.control_session(id, cx)?;
        let tab = self.workspace.tab_of(id).ok_or_else(|| not_open(id))?;
        let split = self.workspace.layout(tab).is_split();
        if let Some(target) = input.beside_session_id {
            if target == id {
                return Err(ControlError::invalid_input(
                    "A pane cannot move beside itself.",
                ));
            }
            let target_tab = self
                .workspace
                .tab_of(target)
                .ok_or_else(|| not_open(target))?;
            let edge = edge(input.edge.unwrap_or_default());
            let mut before = self.active_leaves();
            if tab == id && target_tab != tab {
                // The whole title tab, its panes intact, as a dropped tab.
                self.workspace.place_tab(tab, target, edge);
            } else {
                // One pane, as a dropped pane: its file review keeps its place.
                if let Some(review) = self.separate_session_review(id, cx) {
                    for leaf in &mut before {
                        if leaf.id == id {
                            leaf.id = review;
                        }
                    }
                }
                self.workspace.place(id, target, edge);
            }
            self.begin_pane_motion(before, cx);
        } else {
            if !split {
                return Err(ControlError::invalid_input(format!(
                    "Session {id} already has its own tab; pass besideSessionId to move it."
                )));
            }
            let tabs = self.visible_tabs(cx);
            let to = input
                .index
                .map_or(tabs.len(), |i| i as usize)
                .min(tabs.len());
            let session_index = tabs
                .get(to)
                .and_then(TitleTab::session)
                .and_then(|next| self.workspace.tabs.iter().position(|t| *t == next))
                .unwrap_or(self.workspace.tabs.len());
            self.workspace.detach(id, session_index);
            self.reorder_title_tab(&TitleTab::Session(id), to, cx);
            self.pane_motion = None;
        }
        self.history
            .prune(&self.workspace.open_sessions(), self.workspace.active);
        self.fit_sidebar(cx);
        if let Some(active) = self.workspace.active {
            self.open(active, window, cx);
        }
        self.persist(cx);
        Ok(self.control_tab_now(cx))
    }

    pub(super) fn control_pane_split(
        &mut self,
        input: &PaneSplitInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<TabOutput, ControlError> {
        let id = input.session_id;
        self.control_session(id, cx)?;
        let target = input
            .beside_session_id
            .or(self.workspace.active)
            .ok_or_else(|| {
                ControlError::invalid_input("No pane has focus; pass besideSessionId.")
            })?;
        if target == id {
            return Err(ControlError::invalid_input(
                "A pane cannot split beside itself.",
            ));
        }
        if self.workspace.tab_of(target).is_none() {
            return Err(not_open(target));
        }
        self.control_session(target, cx)?;
        let project = self.session_metadata(id, cx).summary.cwd.clone();
        if self.session_metadata(target, cx).summary.cwd != project {
            return Err(ControlError::invalid_input(format!(
                "Session {id} belongs to {project}; split a pane of that project."
            )));
        }
        let side = match input.side.unwrap_or_default() {
            SplitSide::Right => PaneEdge::Right,
            SplitSide::Down => PaneEdge::Bottom,
        };
        self.app_place_launch(id, Some((target, side)), true, window, cx);
        Ok(self.control_tab_now(cx))
    }

    pub(super) fn control_layout_set(
        &mut self,
        input: &LayoutSetInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> LayoutSetOutput {
        let mut skipped = Vec::new();
        if let Some(style) = input.style {
            self.set_classic_layout(style == LayoutStyle::Classic, cx);
        }
        if let Some(boxy) = input.boxy {
            self.set_layout_boxy(boxy, cx);
        }
        if let Some(tab) = input.sidebar_tab {
            if self.workspace.classic_layout {
                let tab = ClassicSidebarTab::from_id(match tab {
                    SidebarTab::Sessions => "sessions",
                    SidebarTab::Files => "files",
                    SidebarTab::Changes => "changes",
                });
                self.select_classic_sidebar_tab(tab, window, cx);
            } else {
                skipped.push("sidebarTab: only the Classic layout has sidebar tabs".into());
            }
        }
        if let Some(tab) = input.right_tab {
            let tab = match tab {
                PanelTab::Files => RightTab::Files,
                PanelTab::Changes => RightTab::Changes,
                PanelTab::Graph => RightTab::Graph,
            };
            self.set_right_tab(tab, window, cx);
        }
        if let Some(open) = input.right_panel_open {
            self.set_right_panel_open(open, window, cx);
        }
        if (input.right_tab.is_some() || input.right_panel_open == Some(true))
            && !self.workspace.right_panel_open
        {
            skipped.push(
                "rightPanelOpen: the right panel needs a git repository and no app view in front"
                    .into(),
            );
        }
        if let Some(visible) = input.primary_sidebar_visible {
            self.set_primary_sidebar_visible(visible, window, cx);
        }
        self.control_layout_now(skipped, cx)
    }

    fn control_layout_now(&self, skipped: Vec<String>, cx: &App) -> LayoutSetOutput {
        LayoutSetOutput {
            style: if self.workspace.classic_layout {
                LayoutStyle::Classic
            } else {
                LayoutStyle::Focused
            },
            boxy: self.workspace.new_layout_boxy,
            sidebar_tab: match self.classic_sidebar_tab {
                ClassicSidebarTab::Sessions => SidebarTab::Sessions,
                ClassicSidebarTab::Files => SidebarTab::Files,
                ClassicSidebarTab::Changes => SidebarTab::Changes,
            },
            right_panel_open: self.workspace.right_panel_open,
            right_tab: self
                .right_panel
                .as_ref()
                .and_then(|panel| match panel.read(cx).tab() {
                    RightTab::Files => Some(PanelTab::Files),
                    RightTab::Changes => Some(PanelTab::Changes),
                    RightTab::Graph => Some(PanelTab::Graph),
                    RightTab::Terminal => None,
                }),
            primary_sidebar_visible: self.primary_sidebar_visible(),
            skipped,
        }
    }
}
