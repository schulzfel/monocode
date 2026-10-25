//! `workspace_read`, and the stable tab ids the tab operations take back.
use super::*;
use crate::{
    pane_layout::{PaneNode as Node, SplitDir},
    review::ReviewTab,
};
use monocode_control::ops::workspace::{
    LayoutStyle, PaneNode, PaneReview, ReviewTabInfo, RightPanel, SplitDirection, TabInfo, TabKind,
    WorkspaceReadOutput,
};

/// A title tab's id: `session:<id>`, `search`, `app:<view>` or `mono:<id>`.
pub(in crate::shell::control) fn tab_id(tab: &TitleTab) -> String {
    match tab {
        TitleTab::Session(id) => format!("session:{id}"),
        TitleTab::Search => "search".into(),
        TitleTab::App(view) => format!("app:{}", view.label().to_lowercase()),
        TitleTab::Mono { id, .. } => format!("mono:{id}"),
    }
}

fn review_tab(tab: &ReviewTab) -> ReviewTabInfo {
    let (kind, cwd, path) = match tab {
        ReviewTab::File { cwd, path } => ("file", cwd, Some(path)),
        ReviewTab::Changes { cwd, path, .. } => ("changes", cwd, path.as_ref()),
        ReviewTab::Commit { cwd, .. } => ("commit", cwd, None),
        ReviewTab::Session { cwd, path, .. } => ("session", cwd, path.as_ref()),
    };
    ReviewTabInfo {
        kind: kind.into(),
        label: tab.label(),
        cwd: cwd.clone(),
        path: path.cloned(),
    }
}

impl Shell {
    /// The title tab `id` names: a session tab of any project, or a tab in
    /// the current strip.
    pub(in crate::shell::control) fn control_tab(
        &self,
        id: &str,
        cx: &App,
    ) -> Result<Option<TitleTab>, ControlError> {
        let invalid = || {
            ControlError::invalid_input(format!(
                "{id} is not a tab id; workspace_read lists them (session:<id>, search, app:inbox, app:notes, app:automations, mono:<id>)."
            ))
        };
        if let Some(session) = id.strip_prefix("session:") {
            let session: i64 = session.parse().map_err(|_| invalid())?;
            return Ok(self
                .workspace
                .tabs
                .contains(&session)
                .then_some(TitleTab::Session(session)));
        }
        let wanted = match id {
            "search" | "app:inbox" | "app:notes" | "app:automations" => id,
            _ if id.starts_with("mono:") => id,
            _ => return Err(invalid()),
        };
        Ok(self
            .visible_tabs(cx)
            .into_iter()
            .chain(self.active_title_tab(cx))
            .find(|tab| tab_id(tab) == wanted))
    }

    fn tab_label(&self, tab: &TitleTab, cx: &App) -> String {
        match tab {
            TitleTab::Session(key) => {
                let session = self
                    .workspace
                    .session_for_pane(self.workspace.tab_focus(*key));
                if self.session_summaries.contains_key(&session) {
                    self.session_metadata(session, cx).display_title()
                } else {
                    "Session".into()
                }
            }
            TitleTab::Search => "Search".into(),
            TitleTab::App(view) => view.label().into(),
            TitleTab::Mono { id, .. } => self
                .monos
                .as_ref()
                .and_then(|m| m.read(cx).roster.get(id).map(|mono| mono.name.clone()))
                .unwrap_or_else(|| "Mono".into()),
        }
    }

    fn pane_node(&self, node: &Node) -> PaneNode {
        match node {
            Node::Leaf { id } => PaneNode::Pane {
                pane: *id,
                session_id: self.workspace.session_for_pane(*id),
                file: self.workspace.review_owners.contains_key(id),
            },
            Node::Split {
                dir,
                children,
                sizes,
            } => PaneNode::Split {
                direction: match dir {
                    SplitDir::Right => SplitDirection::Right,
                    SplitDir::Down => SplitDirection::Down,
                },
                children: children.iter().map(|child| self.pane_node(child)).collect(),
                sizes: sizes.clone(),
            },
        }
    }

    fn pane_review(&self, pane: i64, cx: &App) -> Option<PaneReview> {
        let live = self.reviews.get(&pane).map(|r| &r.read(cx).state);
        let state = live.or_else(|| self.workspace.reviews.get(&pane))?;
        (!state.tabs.is_empty()).then(|| PaneReview {
            pane,
            tabs: state.tabs.iter().map(review_tab).collect(),
            active: u32::try_from(state.active).unwrap_or(0),
            chat_hidden: state.chat_hidden,
        })
    }

    pub(super) fn control_workspace(&self, cx: &App) -> WorkspaceReadOutput {
        let active = self.active_title_tab(cx);
        let mut strip = self.visible_tabs(cx);
        if let Some(tab) = active.as_ref().filter(|tab| !strip.contains(tab)) {
            // The original layout shows app views and Monos without a tab.
            strip.push(tab.clone());
        }
        let tabs = strip
            .iter()
            .map(|tab| TabInfo {
                id: tab_id(tab),
                kind: match tab {
                    TitleTab::Session(_) => TabKind::Session,
                    TitleTab::Search => TabKind::Search,
                    TitleTab::App(_) => TabKind::App,
                    TitleTab::Mono { .. } => TabKind::Mono,
                },
                label: self.tab_label(tab, cx),
                active: active.as_ref() == Some(tab),
            })
            .collect();
        let session_tab = match active {
            Some(TitleTab::Session(key)) => Some(key),
            _ => None,
        };
        let layout = session_tab.map(|key| self.workspace.layout(key));
        let reviews = layout
            .as_ref()
            .map(|layout| {
                layout
                    .leaf_ids()
                    .into_iter()
                    .filter_map(|pane| self.pane_review(pane, cx))
                    .collect()
            })
            .unwrap_or_default();
        WorkspaceReadOutput {
            project: self.workspace.project.clone(),
            tabs,
            panes: layout.as_ref().map(|layout| self.pane_node(layout)),
            focused_pane: session_tab.and(self.workspace.active),
            reviews,
            right_panel: RightPanel {
                open: self.workspace.right_panel_open,
                tab: serde_json::to_value(self.workspace.right_panel_tab)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
            },
            layout: if self.workspace.classic_layout {
                LayoutStyle::Classic
            } else {
                LayoutStyle::Focused
            },
        }
    }
}
