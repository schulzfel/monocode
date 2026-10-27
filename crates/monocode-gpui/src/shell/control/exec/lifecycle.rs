//! `session_compact`, `session_handoff`, `session_delete` and
//! `project_delete`, through the paths `/compact`, a response's Hand off
//! action and the sidebar's Delete use. Deletes run only once the person has
//! confirmed them (`confirm`).
use super::session::access_or_default;
use super::*;
use monocode_control::ops::{
    lifecycle::{
        ProjectDeleteOutput, SessionCompactOutput, SessionDeleteOutput, SessionHandoffInput,
        SessionHandoffOutput,
    },
    rail::ProjectTarget,
};

fn closed() -> ControlError {
    ControlError::internal("The window closed before the call finished")
}

impl Shell {
    pub(super) fn control_session_compact(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Result<SessionCompactOutput, ControlError> {
        let view = self.control_view(id, cx)?;
        view.update(cx, |view, cx| view.control_compact(cx))?;
        Ok(SessionCompactOutput {
            session_id: id,
            compacting: true,
        })
    }

    pub(super) fn control_session_handoff(
        &mut self,
        input: SessionHandoffInput,
        caller: &Caller,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let id = input.session_id;
        let kind = Kind::parse(&input.harness).ok_or_else(|| {
            ControlError::invalid_input(format!(
                "{} is not a harness MonoCode runs; see model_list.",
                input.harness
            ))
        })?;
        let view = self.control_view(id, cx)?;
        if view.read(cx).busy {
            return Err(ControlError::new(
                ErrorCode::SessionBusy,
                format!("Session {id} is still answering; hand it off once its turn ends."),
            ));
        }
        let mode = access_or_default(input.access, cx);
        let target = monocode_core::response::Target {
            provider: kind.id().to_owned(),
            model: input.model.clone(),
            effort: input.effort.clone(),
            settings: input
                .model_settings
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect(),
        };
        let (built, transfer) = async_channel::bounded(1);
        view.update(cx, |view, cx| {
            view.control_handoff(
                target,
                mode,
                move |result| {
                    let _ = built.try_send(result);
                },
                cx,
            );
        });
        let origin = caller.caller.origin();
        let model = input.model;
        Ok(Exec::Later(cx.spawn(async move |this, cx| {
            let transfer = transfer.recv().await.map_err(|_| closed())??;
            let created = this
                .update(cx, |this, cx| this.transfer_response(transfer, cx))
                .map_err(|_| closed())?;
            let session_id = created.await.map_err(ControlError::internal)?;
            this.update(cx, |this, _| this.record_started(session_id, origin))
                .map_err(|_| closed())?;
            output(&SessionHandoffOutput {
                from_session_id: id,
                session_id,
                harness: kind.id().to_owned(),
                model,
                access: mode.into(),
            })
        })))
    }

    /// The title of session `id` when `caller` may delete it: a local
    /// project session `caller` reaches (`control_session_for`) that is no
    /// orchestration worker.
    pub(in crate::shell::control) fn deletable_session(
        &self,
        caller: &CallerKind,
        id: i64,
        cx: &App,
    ) -> Result<String, ControlError> {
        self.control_local_session_for(caller, id, cx)?;
        if self.orchestration_worker(id) {
            return Err(ControlError::invalid_input(format!(
                "Session {id} is an orchestration worker; it belongs to its lead's orchestration and is not deleted on its own."
            )));
        }
        Ok(self.session_metadata(id, cx).display_title())
    }

    pub(super) fn control_session_delete(
        &mut self,
        id: i64,
        caller: &CallerKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let title = self.deletable_session(caller, id, cx)?;
        // A refusal answers with its own reason, never an older message the
        // window still shows; that one stays for the person.
        let shown = self.error.take();
        let checked = self.delete_session_checked(id, window, cx);
        let reason = self.error.take();
        self.error = reason.clone().or(shown);
        let deleted = checked.map_err(|_| {
            ControlError::invalid_input(
                reason.unwrap_or_else(|| format!("Session {id} could not be deleted")),
            )
        })?;
        Ok(Exec::Later(cx.spawn(async move |_, _| {
            if !deleted.await.map_err(ControlError::internal)? {
                return Err(ControlError::internal(format!(
                    "Session {id} could not be deleted; its data was preserved."
                )));
            }
            output(&SessionDeleteOutput {
                session_id: id,
                title,
            })
        })))
    }

    pub(super) fn control_project_delete(
        &mut self,
        input: &ProjectTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let path = self.control_local_project(Some(&input.project))?;
        let sessions = self.project_sessions(&path);
        self.delete_project(&path, window, cx);
        let written = self.after_group_writes(cx);
        Ok(Exec::Later(cx.spawn(async move |this, cx| {
            written.await;
            let deleted = this
                .read_with(cx, |this, _| {
                    sessions
                        .iter()
                        .filter(|id| !this.session_summaries.contains_key(id))
                        .count()
                })
                .map_err(|_| closed())?;
            output(&ProjectDeleteOutput {
                project: path,
                sessions_deleted: u32::try_from(deleted).unwrap_or(u32::MAX),
            })
        })))
    }
}
