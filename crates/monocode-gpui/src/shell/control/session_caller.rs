//! Agent sessions MonoCode runs call as session callers
//! (`CallerKind::Session`). Their relay's hello carries the session's
//! credential for this app run (`monocode_core::app_mcp`); a valid one gets
//! every scope, whether or not external control is on, and is never asked
//! about. Whether a call runs is decided by the session's provider (its
//! permission mode and approval prompts), so MonoCode shows no confirmation
//! of its own either. A session may not stop, archive, delete, reconfigure
//! or wait on itself (`Operation::NOT_ON_SELF`).
use super::*;
use monocode_control::SessionCredential;

impl Shell {
    /// Answers the hello of a session's relay: every scope when the
    /// credential is this run's token for a session that exists.
    pub(super) fn session_hello(
        &self,
        credential: &SessionCredential,
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
        self.session_exists(credential.id)?;
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
        let checked = self.control_check(&call.command, cx);
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
