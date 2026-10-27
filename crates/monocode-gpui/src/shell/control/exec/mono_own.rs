//! The operations only a Mono calls, on its own memory, soul, habits,
//! artifacts and chat: the Monos run them (`Monos::control_own`), as the
//! bridge's actions ran there. A session card's session is looked up in the
//! window first, as the bridge asked the window for it.
use super::*;
use crate::monos::control_own::{Answer, Own};
use monocode_control::{Command, ops::mono_card::MonoCardPostInput};

impl Shell {
    /// Runs `command`, which only a Mono may call, for the Mono `from` is.
    pub(super) fn control_mono_own(
        &mut self,
        command: Command,
        from: &CallerKind,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let CallerKind::Mono { session, mono, .. } = from else {
            return Err(gate::mono_only_denied(command.name()));
        };
        let monos = self
            .monos
            .clone()
            .filter(|_| self.caller_mono(mono, cx).is_some())
            .ok_or_else(|| ControlError::new(ErrorCode::NotFound, caller::MONO_GONE))?;
        let card_session = match &command {
            Command::MonoCardPost(MonoCardPostInput::Session { session_id, .. }) => {
                self.mono_ensure_session(*session_id, cx)
            }
            _ => None,
        };
        let own = Own {
            mono: mono.clone(),
            caller: *session,
            command,
            session: card_session,
        };
        Ok(
            match monos.update(cx, |monos, cx| monos.control_own(own, cx)) {
                Answer::Now(result) => Exec::Ready(result),
                Answer::Later(task) => Exec::Later(task),
            },
        )
    }
}
