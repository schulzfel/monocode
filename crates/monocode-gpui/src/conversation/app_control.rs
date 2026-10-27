//! The settings of this chat's provider requests outside its runtime turn:
//! `/compact`, the edit-last-turn rewind and a `/btw` target. They get the
//! MonoCode control environment the chat's turn gets
//! (`runtime::app_control::for_turn`), so the agent's `app` command and
//! MonoCode's MCP server work there as in its turns.
use super::*;
use std::collections::HashMap;

impl Conversation {
    /// This chat's settings for a provider request outside its turn: its own,
    /// with the control environment.
    pub(super) fn request_settings(&self) -> Settings {
        let mut settings = self.settings.clone();
        let control = self.control_env(&settings);
        settings.environment.extend(control);
        settings
    }

    /// The control environment a turn of this chat with `settings` gets, as
    /// the runtime gates it: none for text-only settings, a remote chat or
    /// an orchestration worker's chat.
    fn control_env(&self, settings: &Settings) -> HashMap<String, String> {
        if settings.text_only || self.is_remote() || self.orchestration_worker {
            return HashMap::new();
        }
        locked(&self.store)
            .map(|store| {
                crate::runtime::app_control::for_turn(
                    &store,
                    self.summary.id,
                    &self.summary.cwd,
                    settings,
                )
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, WindowHandle};
    use monocode_control::Endpoint;
    use monocode_core::{agent::TurnInfo, app_mcp, store::Store, transcript::Row};
    use std::sync::Mutex;

    /// A chat of a project session in a data folder whose control socket
    /// is served, as the window's listener marks it.
    fn chat(
        name: &str,
        cx: &mut TestAppContext,
    ) -> (
        WindowHandle<Conversation>,
        crate::runtime::app_control::Serving,
    ) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/c")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let store = Store::open_monocode(&dir.join("monocode.db")).unwrap();
        let serving = crate::runtime::app_control::serving(&Endpoint::for_data_dir(&dir).unwrap());
        let summary = store.create("codex", "/work/monocode").unwrap();
        let window = cx.update(|cx| {
            input::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view
                })
            })
            .unwrap()
        });
        (window, serving)
    }

    fn env_of(
        window: WindowHandle<Conversation>,
        cx: &mut TestAppContext,
    ) -> HashMap<String, String> {
        window
            .read_with(cx, |view, _| view.request_settings().environment)
            .unwrap()
    }

    /// T-E2: `/compact` and the edit-last-turn rewind build their provider
    /// request from `request_settings`, so they carry the control
    /// environment as the chat's turns do. A text-only chat, a worker's chat
    /// and a remote chat get none.
    #[gpui::test]
    fn compact_and_rewind_requests_carry_the_control_env(cx: &mut TestAppContext) {
        let (window, _serving) = chat("ce1", cx);
        let id = window.read_with(cx, |v, _| v.summary.id).unwrap();
        let environment = env_of(window, cx);
        assert_eq!(environment[app_mcp::ENV_SESSION], id.to_string());
        assert!(app_mcp::verify(id, &environment[app_mcp::ENV_TOKEN]));
        assert_eq!(environment[app_mcp::ENV_ROLE], "session");
        let saved = window
            .read_with(cx, |v, _| v.settings.environment.clone())
            .unwrap();
        assert!(
            saved.is_empty(),
            "the chat's own settings keep no credential"
        );

        window
            .update(cx, |view, _, _| view.settings.text_only = true)
            .unwrap();
        assert!(
            !env_of(window, cx).contains_key(app_mcp::ENV_TOKEN),
            "text-only"
        );
        window
            .update(cx, |view, _, _| {
                view.settings.text_only = false;
                view.set_orchestration_worker(true);
            })
            .unwrap();
        assert!(
            !env_of(window, cx).contains_key(app_mcp::ENV_TOKEN),
            "a worker"
        );

        window
            .update(cx, |view, _, _| {
                view.set_orchestration_worker(false);
                view.summary.cwd = "remote://host/work/monocode".into();
            })
            .unwrap();
        assert!(
            !env_of(window, cx).contains_key(app_mcp::ENV_TOKEN),
            "remote"
        );
    }

    /// T-E2: a `/btw` target copies the chat's settings with the control
    /// environment, and keeps it for its run.
    #[gpui::test]
    fn a_btw_target_carries_the_control_env(cx: &mut TestAppContext) {
        let (window, _serving) = chat("ce3", cx);
        let rows = vec![
            Row {
                role: "user".into(),
                text: "Why?".into(),
                first: true,
                sent_at: None,
            },
            Row {
                role: "assistant".into(),
                text: "Because.".into(),
                first: true,
                sent_at: None,
            },
        ];
        window
            .update(cx, |view, _, cx| {
                view.transcript.update(cx, |t, cx| t.patch(0, rows, cx));
                view.settings.turns.insert(
                    2,
                    TurnInfo {
                        provider: "codex".into(),
                        completed_at: Some(1),
                        ..Default::default()
                    },
                );
            })
            .unwrap();
        let (id, target) = window
            .read_with(cx, |view, cx| {
                (view.summary.id, view.btw_target(cx).map(|t| t.settings))
            })
            .unwrap();
        let environment = target.expect("a completed turn to ask about").environment;
        assert_eq!(environment[app_mcp::ENV_SESSION], id.to_string());
        assert_eq!(environment[app_mcp::ENV_ROLE], "session");
    }
}
