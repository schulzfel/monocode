//! Explicit setters for preferences and the window's layout. The Settings
//! page, keyboard toggles and external control all change state through
//! these, so each change applies the same way wherever it comes from.
use super::*;
use crate::settings::Preferences;

impl Shell {
    /// Changes the app-wide preferences and applies them at once: the
    /// keymap, themes, open transcripts and the chrome, then saves them.
    pub(super) fn update_preferences(
        &mut self,
        change: impl FnOnce(&mut Preferences),
        cx: &mut Context<Self>,
    ) {
        change(&mut self.workspace.preferences);
        self.apply_preferences(cx);
        self.persist(cx);
        self.invalidate_chrome(cx);
        cx.notify();
    }

    /// Classic (`true`) or the Focused layout.
    pub(super) fn set_classic_layout(&mut self, classic: bool, cx: &mut Context<Self>) {
        if self.workspace.classic_layout != classic {
            self.toggle_layout(cx);
        }
    }

    /// The Focused layout's boxy frame or its rounded card.
    pub(super) fn set_layout_boxy(&mut self, boxy: bool, cx: &mut Context<Self>) {
        self.workspace.new_layout_boxy = boxy;
        self.layout_changed(cx);
        self.sync_settings(cx);
    }

    /// Whether the primary sidebar shows: the project rail in Classic, the
    /// session sidebar otherwise, as the sidebar shortcut decides.
    pub(super) fn primary_sidebar_visible(&self) -> bool {
        if self.workspace.classic_layout && self.settings.is_none() {
            self.workspace.project_rail_open
        } else {
            self.workspace.sidebar_visible
        }
    }

    /// Shows or hides the primary sidebar, as its shortcut would.
    pub(super) fn set_primary_sidebar_visible(
        &mut self,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.primary_sidebar_visible() != visible {
            self.toggle_primary_sidebar(window, cx);
        }
    }

    /// Shows `tab` in the right panel, opening it.
    pub(super) fn set_right_tab(
        &mut self,
        tab: crate::right_panel::RightTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_right_panel(window, cx);
        if let Some(panel) = self.right_panel.clone() {
            panel.update(cx, |panel, cx| panel.select(tab, cx));
        }
        self.set_right_panel_open(true, window, cx);
        self.invalidate_chrome(cx);
        cx.notify();
    }
}

#[cfg(test)]
#[path = "setter_tests.rs"]
mod tests;
