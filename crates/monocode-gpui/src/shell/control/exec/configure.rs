//! `session_configure` and `session_draft`: the phone's model and access
//! sheets for one session (`control_configure`, which remembers nothing as
//! a default) and Operator's draft (`save_app_draft`).
use super::dispatch::prompt;
use super::*;
use monocode_control::ops::{
    configure::{
        SessionConfigureInput, SessionConfigureOutput, SessionDraftInput, SessionDraftOutput,
    },
    session::SessionStatus as Status,
};

/// The longest model id accepted, as for the phone.
const MAX_MODEL_ID: usize = 256;

fn busy(id: i64) -> ControlError {
    ControlError::new(
        ErrorCode::SessionBusy,
        format!("Session {id} is working; change it once it is idle (see session_wait)."),
    )
}

impl Shell {
    /// The loaded chat of session `id`, refusing a running turn.
    fn control_idle_view(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Result<Entity<Conversation>, ControlError> {
        if self.session_metadata(id, cx).busy {
            return Err(busy(id));
        }
        let view = self.control_view(id, cx)?;
        view.update(cx, |view, cx| view.load(cx));
        if matches!(
            view.read(cx).control_status(),
            Status::Working | Status::Connecting | Status::Stopping
        ) {
            return Err(busy(id));
        }
        Ok(view)
    }

    pub(super) fn control_session_configure(
        &mut self,
        input: SessionConfigureInput,
        cx: &mut Context<Self>,
    ) -> Result<SessionConfigureOutput, ControlError> {
        let id = input.session_id;
        if input
            .model
            .as_ref()
            .is_some_and(|m| m.trim().is_empty() || m.len() > MAX_MODEL_ID)
        {
            return Err(ControlError::invalid_input(
                "model must be a model id from model_list",
            ));
        }
        let view = self.control_idle_view(id, cx)?;
        let kind = Kind::from_id(&self.session_metadata(id, cx).summary.provider);
        let model = input
            .model
            .map(|model| (kind, monocode_core::store::native_model(&model, kind.id())));
        let mut settings: std::collections::HashMap<String, String> = input
            .model_settings
            .unwrap_or_default()
            .into_iter()
            .collect();
        if let Some(effort) = input.effort {
            settings.insert("effort".into(), effort);
        }
        let mode = input.access.map(Into::into);
        view.update(cx, |view, cx| {
            view.control_configure(model, &settings, mode, cx);
        });
        let view = view.read(cx);
        Ok(SessionConfigureOutput {
            session_id: id,
            harness: kind.id().into(),
            model: view.current_model(),
            effort: view.settings.effort.clone(),
            access: view.settings.access.into(),
        })
    }

    pub(super) fn control_session_draft(
        &mut self,
        input: &SessionDraftInput,
        cx: &mut Context<Self>,
    ) -> Result<SessionDraftOutput, ControlError> {
        let text = prompt(&input.text)?;
        let id = input.session_id;
        let view = self.control_idle_view(id, cx)?;
        if !view.read(cx).ready_for_external(cx) {
            return Err(busy(id));
        }
        if view.read(cx).has_saved_draft() || !view.read(cx).input.read(cx).content.is_empty() {
            return Err(ControlError::new(
                ErrorCode::SessionNotIdle,
                format!("Session {id} already has an unsent draft; it was kept."),
            ));
        }
        view.update(cx, |view, cx| view.save_app_draft(text, cx));
        Ok(SessionDraftOutput {
            session_id: id,
            saved: true,
        })
    }
}
