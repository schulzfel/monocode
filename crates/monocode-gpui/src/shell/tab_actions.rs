//! Tab actions as methods: what ⌘W closes, and opening a file reference.
//! The window's action handlers delegate here, and external control calls
//! the same methods.
use super::*;

impl Shell {
    /// ⌘W: closes what has focus. An app view or search closes first, then a
    /// Mono tab, the focused terminal, the focused review tab, and finally
    /// the active pane: only that pane in a split, the whole tab otherwise.
    pub(super) fn close_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.active_app_view.filter(|_| self.settings.is_none()) {
            self.close_title_tab(&TitleTab::App(view), window, cx);
            return;
        }
        if self.app_search.is_some() {
            self.close_title_tab(&TitleTab::Search, window, cx);
            return;
        }
        if let Some(tab @ TitleTab::Mono { .. }) = self.active_title_tab(cx) {
            self.close_title_tab(&tab, window, cx);
            return;
        }
        // ⌘W in the focused dock closes its terminal.
        if let Some(ix) = self.dock_focused(window, cx) {
            self.close_terminal(ix, window, cx);
            return;
        }
        // A focused review closes its tab first, as a focused file pane does.
        if self.review_focused
            && let Some(review) = self.active_review(cx)
        {
            review.update(cx, |r, cx| r.close_active(cx));
            return;
        }
        if let Some(id) = self.workspace.active {
            // Close Pane (⌘W) in a split; the whole tab otherwise.
            self.close_pane(id, window, cx);
        }
    }

    /// Closes one title tab: an app view (in the original layout, the open
    /// one), search, a Mono's chat, or a session tab with all its panes.
    pub(super) fn close_title_tab(
        &mut self,
        tab: &TitleTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match tab {
            TitleTab::App(view) if self.workspace.classic_layout => {
                if self.active_app_view == Some(*view) {
                    self.close_app_view(window, cx);
                }
            }
            TitleTab::App(_) => self.close_title_tabs(std::slice::from_ref(tab), None, window, cx),
            TitleTab::Search => self.close_search_tab(window, cx),
            TitleTab::Mono { id, .. } => self.close_mono_tab(id, window, cx),
            TitleTab::Session(key) => self.close(*key, window, cx),
        }
    }

    /// Opens a file reference from the current project's checkout: a path,
    /// `path:line[:column]`, `path#L12` or a `file://` URL.
    pub(super) fn open_path(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.git.read(cx).project().to_owned();
        self.open_path_in(cwd, target, window, cx);
    }

    /// Opens `target`, resolved against `cwd`, in the active pane's review
    /// panel and moves to its line and column.
    pub(super) fn open_path_in(
        &mut self,
        cwd: String,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (path, location) = crate::file_editor::file_location(target, &cwd);
        self.open_review(crate::review::ReviewTab::File { cwd, path }, window, cx);
        // Monos render above the session pane tree. Give its file
        // an independent visible tab using the normal navigation
        // path, which retains the Mono tab and live conversation.
        if self.mono_active(cx)
            && let Some(owner) = self.workspace.active
            && let Some(pane) = self.independent_file_pane(owner, cx)
        {
            self.open_file_pane(pane, window, cx);
        }
        if let Some((line, column)) = location
            && let Some(pane) = self.active_review(cx)
        {
            pane.update(cx, |pane, cx| pane.navigate_file(line, column, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn dispatch(
        window: gpui::WindowHandle<Shell>,
        action: impl gpui::Action,
        cx: &mut TestAppContext,
    ) {
        // Actions travel the dispatch tree of the last drawn frame.
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        window
            .update(visual, |_, w, cx| w.dispatch_action(Box::new(action), cx))
            .unwrap();
        visual.run_until_parked();
    }

    #[gpui::test]
    fn close_tab_closes_an_app_view_then_the_focused_pane_then_its_tab(cx: &mut TestAppContext) {
        let (window, a, b) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, w, cx| shell.open_notes(w, cx))
            .unwrap();
        cx.run_until_parked();
        dispatch(window, CloseTab, cx);
        window
            .update(cx, |shell, _, _| {
                assert_eq!(shell.active_app_view, None, "the app view closes first");
                assert_eq!(shell.workspace.active, Some(b));
            })
            .unwrap();
        window
            .update(cx, |shell, w, cx| {
                shell.workspace.place(a, b, Edge::Right);
                shell.activate_session(a, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        dispatch(window, CloseTab, cx);
        window
            .update(cx, |shell, _, _| {
                let tab = shell.workspace.tab_of(b).unwrap();
                assert_eq!(
                    shell.workspace.layout(tab).leaf_ids(),
                    [b],
                    "in a split, only the focused pane closes"
                );
                assert_eq!(shell.workspace.tab_of(a), None);
                assert_eq!(shell.workspace.active, Some(b));
            })
            .unwrap();
        window
            .update(cx, |shell, _, cx| {
                // A lone blank tab stays; one with history may close.
                shell.sessions[&b].update(cx, |s, _| s.summary.has_messages = true);
            })
            .unwrap();
        dispatch(window, CloseTab, cx);
        window
            .update(cx, |shell, _, _| {
                assert_eq!(
                    shell.workspace.tab_of(b),
                    None,
                    "outside a split the tab closes"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn open_path_opens_the_file_in_the_active_panes_review(cx: &mut TestAppContext) {
        let (window, _, b) = crate::shell::tests::window(cx);
        let cwd = window
            .update(cx, |shell, w, cx| {
                // Actions start at the focused element: the chat's composer.
                shell.activate_session(b, w, cx);
                shell.git.read(cx).project().to_owned()
            })
            .unwrap();
        // A file that exists, read only. The location suffix is stripped here;
        // file_panes' tests check that it moves the cursor once loaded.
        let file = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        dispatch(
            window,
            crate::file_editor::OpenPath(format!("{file}:3:2")),
            cx,
        );
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.workspace.active, Some(b));
                let review = shell.reviews.get(&b).expect("a review panel").read(cx);
                let expected = crate::review::ReviewTab::File {
                    path: file.into(),
                    cwd: cwd.clone(),
                };
                assert!(
                    review.state.tabs.iter().any(|tab| tab.same_tab(&expected)),
                    "{:?}",
                    review.state.tabs
                );
                assert!(shell.review_focused);
                let editor = review.active_editor().expect("the file editor is mounted");
                assert_eq!(editor.read(cx).path, file);
            })
            .unwrap();
    }
}
