//! Agent sessions MonoCode runs call as session callers
//! (`CallerKind::Session`). Their relay's hello carries the session's
//! credential for this app run (`monocode_core::app_mcp`); a valid one gets
//! every scope, whether or not external control is on, and is never asked
//! about. Whether a call runs is decided by the session's provider (its
//! permission mode and approval prompts), so MonoCode shows no confirmation
//! of its own either. What a session may do on each call is `control_grant`'s
//! (`gate`). An orchestration's workers keep their own scoped workflow and
//! get no MonoCode tools: their hello is refused. A Mono's main chat and its
//! running habits call as that Mono (`CallerKind::Mono`), for as long as the
//! Mono owns the session.
use super::*;
use monocode_control::SessionCredential;

impl Shell {
    /// Answers the hello of a session's relay: every scope when the
    /// credential is this run's token for a Mono's main chat or running
    /// habit, or for a session that exists and is no orchestration worker.
    pub(super) fn session_hello(
        &self,
        credential: &SessionCredential,
        cx: &App,
    ) -> Result<Grant, ControlError> {
        if !monocode_core::app_mcp::verify(credential.id, &credential.token) {
            return Err(ControlError::new(
                ErrorCode::ScopeDenied,
                format!(
                    "The credential for session {} is not valid in this MonoCode run. Only the relay MonoCode starts inside its own sessions can use one.",
                    credential.id
                ),
            ));
        }
        match self.resolve_caller(&CallerKind::Session(credential.id), cx)? {
            CallerKind::Mono { .. } => return Ok(caller::session_grant()),
            CallerKind::Session(_) | CallerKind::External(_) => {}
        }
        self.session_exists(credential.id)?;
        if self.orchestration_worker(credential.id) {
            return Err(ControlError::new(
                ErrorCode::ScopeDenied,
                format!(
                    "Session {} is an orchestration worker; workers keep their lead's scoped workflow and have no MonoCode tools.",
                    credential.id
                ),
            ));
        }
        Ok(caller::session_grant())
    }

    /// Whether session `id` exists, which a session caller's grant lasts as
    /// long as.
    pub(super) fn session_exists(&self, id: i64) -> Result<(), ControlError> {
        if self.sessions.contains_key(&id) || self.session_summaries.contains_key(&id) {
            Ok(())
        } else {
            Err(ControlError::new(
                ErrorCode::NotFound,
                format!("MonoCode has no session {id}."),
            ))
        }
    }

    /// Runs a confirmed call from a session caller without asking: its
    /// target is checked, and pinned, exactly as for the prompt.
    pub(super) fn run_unconfirmed(&mut self, call: Pending, grant: Grant, cx: &mut Context<Self>) {
        let checked = self.control_check(&call.caller, &call.command, cx);
        cx.spawn(async move |this, cx| {
            let checked = checked.await;
            let _ = this.update_in(cx, |this, window, cx| match checked {
                Ok(checked) => {
                    let pin = checked.into_pin();
                    this.execute_control(call, &grant, pin.as_ref(), window, cx);
                }
                Err(error) => this.finish_control(call, Err(error), cx),
            });
        })
        .detach();
    }
}
