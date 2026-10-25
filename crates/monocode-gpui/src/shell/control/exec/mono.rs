//! `mono_list`, `mono_open` and `mono_send`. Opening goes through the Mono
//! tab a click opens; a message is queued as one typed in the Mono's chat
//! (`Monos::mobile_send`, the composer's path). Memory, soul, habits and
//! artifacts stay inside the Mono.
use super::*;
use monocode_control::ops::mono::{
    MAX_MONO_MESSAGE_BYTES, MonoListOutput, MonoOpenOutput, MonoSendInput, MonoSendOutput,
    MonoSummary,
};
use monocode_core::mono::Mono;

fn missing(id: &str) -> ControlError {
    ControlError::new(
        ErrorCode::NotFound,
        format!("There is no Mono {id}; see mono_list."),
    )
}

impl Shell {
    fn control_monos(&self) -> Result<Entity<crate::monos::Monos>, ControlError> {
        self.monos
            .clone()
            .filter(|_| self.workspace.preferences.monos)
            .ok_or_else(|| {
                ControlError::new(ErrorCode::NotFound, "Monos are turned off in MonoCode.")
            })
    }

    /// Whether the Mono's chat is answering or has messages waiting.
    fn mono_busy(monos: &crate::monos::Monos, mono: &Mono, cx: &App) -> bool {
        !mono.outbox.is_empty()
            || monos
                .open_chat(&mono.id)
                .is_some_and(|chat| chat.read(cx).busy)
    }

    pub(super) fn control_mono_list(&self, cx: &App) -> Result<MonoListOutput, ControlError> {
        let monos = self.control_monos()?;
        let monos = monos.read(cx);
        Ok(MonoListOutput {
            monos: monos
                .roster
                .monos
                .iter()
                .map(|mono| MonoSummary {
                    id: mono.id.clone(),
                    name: mono.name.clone(),
                    projects: mono.projects.clone(),
                    busy: Self::mono_busy(monos, mono, cx),
                })
                .collect(),
        })
    }

    pub(super) fn control_mono_open(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<MonoOpenOutput, ControlError> {
        let monos = self.control_monos()?;
        let name = monos
            .read(cx)
            .roster
            .get(id)
            .map(|m| m.name.clone())
            .ok_or_else(|| missing(id))?;
        self.open_title_tab(&self.mono_tab(id.to_owned()), window, cx);
        Ok(MonoOpenOutput {
            mono_id: id.to_owned(),
            name,
        })
    }

    pub(super) fn control_mono_send(
        &mut self,
        input: MonoSendInput,
        cx: &mut Context<Self>,
    ) -> Result<MonoSendOutput, ControlError> {
        let monos = self.control_monos()?;
        if input.text.trim().is_empty() || input.text.len() > MAX_MONO_MESSAGE_BYTES {
            return Err(ControlError::invalid_input(format!(
                "The message must be 1 to {MAX_MONO_MESSAGE_BYTES} bytes."
            )));
        }
        let (name, busy) = {
            let manager = monos.read(cx);
            let mono = manager
                .roster
                .get(&input.mono_id)
                .ok_or_else(|| missing(&input.mono_id))?;
            (mono.name.clone(), Self::mono_busy(manager, mono, cx))
        };
        if busy {
            return Err(ControlError::new(
                ErrorCode::SessionBusy,
                format!("{name} is answering or has messages waiting; send once it is idle."),
            ));
        }
        let id = input.mono_id;
        monos
            .update(cx, |m, cx| m.mobile_send(&id, input.text, Vec::new(), cx))
            .map_err(|_| missing(&id))?;
        Ok(MonoSendOutput { mono_id: id, name })
    }
}
