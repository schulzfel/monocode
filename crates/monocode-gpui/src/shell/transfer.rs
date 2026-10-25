//! Response transfers: a response's Hand off and Second opinion actions, and
//! `session_handoff` from external control, make a new session from a
//! response of another.
use super::*;

impl Shell {
    /// Creates the session `transfer` asks for and shows it, unless the
    /// person navigated meanwhile. Resolves to the new session's id; a
    /// failure is also shown in the window.
    pub(super) fn transfer_response(
        &mut self,
        transfer: Transfer,
        cx: &mut Context<Self>,
    ) -> gpui::Task<Result<i64, String>> {
        use monocode_core::{
            agent::{Handoff, Settings},
            response::{self, TransferKind},
        };
        let navigation_revision = self.navigation_revision;
        let store = self.store.clone();
        let target_kind = Kind::from_id(&transfer.target.provider);
        let account = crate::accounts::get(cx)
            .map(|center| center.read(cx).selected(target_kind, &transfer.cwd))
            .unwrap_or_default();
        let work = cx.background_executor().spawn(async move {
            let mut settings = Settings {
                account,
                revision: response::now_ms() * 1_000_000,
                model: transfer.target.model.clone(),
                effort: transfer.target.effort.clone(),
                model_settings: transfer.target.settings.clone(),
                access: transfer.settings.access,
                worktree_cwd: transfer.settings.worktree_cwd.clone(),
                ..Settings::default()
            };
            let prompt = if transfer.kind == TransferKind::SecondOpinion {
                Some(response::second_opinion(&transfer.from, &transfer.turn))
            } else {
                settings.handoff = Some(Handoff {
                    from: transfer.from.clone(),
                    brief: response::handoff(&transfer.through),
                });
                None
            };
            let mut summary = locked(&store)?.create(&transfer.target.provider, &transfer.cwd)?;
            summary.title = if transfer.kind == TransferKind::SecondOpinion {
                "Second opinion".into()
            } else {
                transfer.title
            };
            locked(&store)?.title(summary.id, &summary.title)?;
            locked(&store)?.configure_agent(summary.id, &summary.provider, &settings)?;
            Ok::<_, String>((summary, settings, prompt))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok((summary, settings, prompt)) => {
                    let id = summary.id;
                    this.add(summary, cx);
                    this.sessions[&id]
                        .update(cx, |view, cx| view.install_transfer(settings, prompt, cx));
                    this.order.retain(|s| *s != id);
                    this.order.insert(0, id);
                    if this.navigation_revision == navigation_revision {
                        this.activate_session(id, window, cx);
                    }
                    Ok(id)
                }
                Err(error) => {
                    this.error = Some(error.clone());
                    cx.notify();
                    Err(error)
                }
            })
            .map_err(|_| "The window closed".to_owned())?
        })
    }
}
