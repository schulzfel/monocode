//! What the debug-only QA hook (`crate::qa_hook`) does through the shell:
//! read what the window shows, type into a chat's composer, switch views
//! and quit, each as the person would.
use super::*;
use crate::qa_hook::View;
use serde_json::{Value, json};

impl Shell {
    /// What the window shows, in text.
    pub(crate) fn qa_state(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Value {
        let title = |shell: &Self, id: i64| {
            shell
                .session_summaries
                .get(&id)
                .map(|summary| summary.title.clone())
        };
        let panes: Vec<Value> = self
            .active_leaves()
            .iter()
            .map(|leaf| {
                json!({
                    "session": leaf.id,
                    "title": title(self, leaf.id),
                    "busy": self.sessions.get(&leaf.id).is_some_and(|v| v.read(cx).busy),
                })
            })
            .collect();
        let active = self.workspace.active;
        json!({
            "activeSession": active,
            "activeTitle": active.and_then(|id| title(self, id)),
            "panes": panes,
            "tabs": self.workspace.tabs,
            "banner": {"shown": self.error.is_some(), "text": self.error},
            "mcpServer": self.control_status().enabled,
            "settingsOpen": self.settings.is_some(),
            "mono": self.monos.as_ref().and_then(|m| m.read(cx).active.clone()),
            "window": crate::qa_hook::window_facts(window),
        })
    }

    /// Types `text` into session `id`'s composer and presses Enter.
    pub(crate) fn qa_submit(
        &mut self,
        id: i64,
        text: String,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let view = self
            .ensure_session(id, cx)
            .ok_or_else(|| format!("There is no session {id}."))?;
        let input = view.read(cx).input.clone();
        input.update(cx, |input, cx| {
            input.set_text(text, cx);
            // What Enter does in a composer with no completion open.
            cx.emit(crate::input::Submitted);
        });
        Ok(json!({"session": id, "submitted": true}))
    }

    /// Shows `view`, then says what the window shows.
    pub(crate) fn qa_view(
        &mut self,
        view: &View,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        match view {
            View::Session(id) => {
                if !self.session_summaries.contains_key(id) {
                    return Err(format!("There is no session {id}."));
                }
                self.open(*id, window, cx);
            }
            View::Mono(id) => {
                let monos = self.monos.clone().ok_or("Monos are not available.")?;
                monos.update(cx, |monos, cx| monos.open(id, window, cx));
                if monos.read(cx).active.as_deref() != Some(id.as_str()) {
                    return Err(format!("Mono {id} could not be shown."));
                }
            }
        }
        Ok(self.qa_state(window, cx))
    }

    /// Quits as Cmd-Q does.
    pub(crate) fn qa_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_quit(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn state_names_the_active_session_its_panes_and_the_mcp_switch(cx: &mut TestAppContext) {
        let (window, _, b) = crate::shell::tests::window(cx);
        let state = window
            .update(cx, |shell, window, cx| shell.qa_state(window, cx))
            .unwrap();
        assert_eq!(state["activeSession"], json!(b));
        assert!(
            state["panes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["session"] == json!(b))
        );
        assert_eq!(state["mcpServer"], json!(false));
        assert_eq!(state["banner"]["shown"], json!(false));
        assert_eq!(state["settingsOpen"], json!(false));
    }

    #[gpui::test]
    fn view_shows_a_session_and_refuses_an_unknown_one(cx: &mut TestAppContext) {
        let (window, a, _) = crate::shell::tests::window(cx);
        let state = window
            .update(cx, |shell, window, cx| {
                shell.qa_view(&View::Session(a), window, cx)
            })
            .unwrap()
            .unwrap();
        assert_eq!(state["activeSession"], json!(a));
        let missing = window
            .update(cx, |shell, window, cx| {
                shell.qa_view(&View::Session(9_999), window, cx)
            })
            .unwrap();
        assert!(missing.unwrap_err().contains("9999"));
    }

    /// The composer's own Enter path: `/operator` turns the session's
    /// Operator access on, as typing it does, while an app call still
    /// cannot enable `/operator`.
    #[gpui::test]
    fn submit_takes_the_composer_path_without_weakening_the_operator_refusal(
        cx: &mut TestAppContext,
    ) {
        let (window, _, b) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&b].update(cx, |view, _| {
                    // Save the turn, then fail without starting a provider.
                    view.summary.provider = "fixture".into();
                });
                shell.qa_submit(b, "/operator @qa echo-prompt".into(), cx)
            })
            .unwrap()
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let view = shell.sessions[&b].read(cx);
                assert!(view.busy || !view.input.read(cx).content.contains("@qa"));
                assert!(view.settings.operator_enabled);
            })
            .unwrap();
        assert!(
            monocode_core::app_access::prompt(&json!({"prompt": "/operator list sessions"}))
                .is_err()
        );
        let missing = window
            .update(cx, |shell, _, cx| shell.qa_submit(9_999, "hi".into(), cx))
            .unwrap();
        assert!(missing.is_err());
    }
}
