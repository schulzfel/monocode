//! `tab_open`, `tab_focus` and `tab_close`, through the window's own tab
//! methods (`open`, `open_title_tab`, `close_title_tab`, `close_pane`).
use super::session::remote_unsupported;
use super::workspace::tab_id;
use super::*;
use monocode_control::ops::tab::{AppView, TabCloseOutput, TabOpenInput, TabOutput, TabTarget};
use monocode_core::files::path_key;

impl Shell {
    /// What the window shows now.
    pub(super) fn control_tab_now(&self, cx: &App) -> TabOutput {
        let active = self.active_title_tab(cx);
        TabOutput {
            pane: matches!(active, Some(TitleTab::Session(_)))
                .then_some(self.workspace.active)
                .flatten(),
            tab: active.as_ref().map(tab_id),
            project: self.workspace.project.clone(),
        }
    }

    pub(super) fn control_tab_open(
        &mut self,
        input: &TabOpenInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<TabOutput, ControlError> {
        let targets = [
            input.session_id.is_some(),
            input.path.is_some(),
            input.view.is_some(),
            input.search == Some(true),
        ];
        if targets.iter().filter(|t| **t).count() != 1 {
            return Err(ControlError::invalid_input(
                "Pass exactly one of sessionId, path, view or search.",
            ));
        }
        let placed = input.placement.is_some() || input.beside_session_id.is_some();
        if placed && input.session_id.is_none() {
            return Err(ControlError::invalid_input(
                "placement and besideSessionId only apply to a session.",
            ));
        }
        if let Some(id) = input.session_id {
            self.control_open_session(id, input, window, cx)?;
        } else if let Some(path) = &input.path {
            self.control_open_file(path, window, cx)?;
        } else if let Some(view) = input.view {
            match view {
                AppView::Inbox => self.open_inbox(window, cx),
                AppView::Notes => self.open_notes(window, cx),
                AppView::Automations => self.open_automations(window, cx),
            }
        } else {
            self.open_app_search(window, cx);
        }
        Ok(self.control_tab_now(cx))
    }

    fn control_open_session(
        &mut self,
        id: i64,
        input: &TabOpenInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), ControlError> {
        let project = self.session_metadata(id, cx).summary.cwd.clone();
        if let Some(beside) = input.beside_session_id {
            self.control_local_session(beside, cx)?;
        }
        let focused = self.control_focused_pane(&project, Some(id), cx);
        let placement = self.control_placement(
            input.placement,
            input.beside_session_id,
            focused,
            &project,
            cx,
        )?;
        if placement.is_some_and(|(beside, _)| beside == id) {
            return Err(ControlError::invalid_input(
                "besideSessionId must be another session's pane.",
            ));
        }
        if placement.is_some() {
            self.app_place_launch(id, placement, true, window, cx);
        } else {
            self.open(id, window, cx);
        }
        Ok(())
    }

    /// Opens a file of a local project, switching to that project first.
    fn control_open_file(
        &mut self,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), ControlError> {
        if is_remote_path(target) {
            return Err(remote_unsupported());
        }
        let current = self.workspace.project.clone();
        let (resolved, location) = crate::file_editor::file_location(target, &current);
        let file = std::path::Path::new(&resolved)
            .canonicalize()
            .map_err(|_| {
                ControlError::new(ErrorCode::NotFound, format!("There is no file {resolved}."))
            })?;
        if !file.is_file() {
            return Err(ControlError::invalid_input(format!(
                "{resolved} is not a file."
            )));
        }
        let project = self
            .workspace
            .projects
            .iter()
            .filter(|p| !is_remote_path(p))
            .filter(|p| {
                std::path::Path::new(p.as_str())
                    .canonicalize()
                    .is_ok_and(|root| file.starts_with(root))
            })
            .max_by_key(|p| p.len())
            .cloned()
            .ok_or_else(|| {
                ControlError::invalid_input(format!(
                    "{resolved} is not inside a local MonoCode project."
                ))
            })?;
        if path_key(&project) != path_key(&current) {
            self.select_project(project.clone(), window, cx);
        }
        if self.workspace.active.is_none() {
            return Err(ControlError::new(
                ErrorCode::ProjectNotOpen,
                format!(
                    "{project} has no open session to show the file beside; open one with tab_open sessionId."
                ),
            ));
        }
        let file = file.to_string_lossy();
        let target = match location {
            Some((line, column)) => format!("{file}:{line}:{column}"),
            None => file.into_owned(),
        };
        self.open_path_in(project, &target, window, cx);
        Ok(())
    }

    pub(super) fn control_tab_focus(
        &mut self,
        input: &TabTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<TabOutput, ControlError> {
        match (&input.tab, input.pane) {
            (Some(id), None) => {
                let tab = self.control_tab(id, cx)?.ok_or_else(|| {
                    ControlError::new(ErrorCode::NotFound, format!("There is no tab {id}."))
                })?;
                if let TitleTab::Session(key) = &tab {
                    self.control_local_session(self.workspace.tab_focus(*key), cx)?;
                }
                self.open_title_tab(&tab, window, cx);
            }
            (None, Some(pane)) => {
                if self.workspace.tab_of(pane).is_none() {
                    return Err(ControlError::new(
                        ErrorCode::NotFound,
                        format!("There is no open pane {pane}."),
                    ));
                }
                self.control_local_session(self.workspace.session_for_pane(pane), cx)?;
                self.open(pane, window, cx);
            }
            _ => {
                return Err(ControlError::invalid_input(
                    "Pass exactly one of tab or pane.",
                ));
            }
        }
        Ok(self.control_tab_now(cx))
    }

    pub(super) fn control_tab_close(
        &mut self,
        input: &TabTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<TabCloseOutput, ControlError> {
        let closed = match (&input.tab, input.pane) {
            (Some(id), None) => match self.control_tab(id, cx)? {
                None => false,
                Some(tab) => {
                    if let TitleTab::Session(key) = &tab {
                        self.control_local_session(self.workspace.tab_focus(*key), cx)?;
                    }
                    let before = self.control_tab(id, cx)?.is_some();
                    self.close_title_tab(&tab, window, cx);
                    before && self.control_tab(id, cx)?.is_none()
                }
            },
            (None, Some(pane)) => {
                if self.workspace.tab_of(pane).is_none() {
                    false
                } else {
                    self.control_local_session(self.workspace.session_for_pane(pane), cx)?;
                    self.close_pane(pane, window, cx);
                    self.workspace.tab_of(pane).is_none()
                }
            }
            _ => {
                return Err(ControlError::invalid_input(
                    "Pass exactly one of tab or pane.",
                ));
            }
        };
        Ok(TabCloseOutput {
            closed,
            now: self.control_tab_now(cx),
        })
    }
}
