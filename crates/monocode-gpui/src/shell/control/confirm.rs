//! Confirmations: a call that deletes, discards or publishes
//! (`Command::confirmed`) asks the person in the window first, naming the
//! client and exactly what it would do. One prompt per client at a time; no
//! answer within `CONFIRM_TIMEOUT` is a refusal. A refused call fails with
//! `user_declined`, changes nothing and keeps no receipt, so a retry asks
//! again. The activity log records each decision.
use super::exec::vcs;
use super::prompts::{Checked, question};
use super::*;

/// A call waiting on its prompt.
pub(super) struct Waiting {
    id: u64,
    call: Pending,
    target: Option<String>,
    pin: Option<vcs::Pin>,
}

fn declined(client: &str, what: &str) -> ControlError {
    ControlError::new(
        ErrorCode::UserDeclined,
        format!(
            "The person did not confirm {what} for “{client}” in MonoCode (declined, or no answer within two minutes). Nothing was changed."
        ),
    )
}

impl Shell {
    /// Asks the person about `call`, then runs it if they confirm.
    pub(super) fn confirm_control(&mut self, call: Pending, cx: &mut Context<Self>) {
        if self.control.confirmations.contains_key(&call.caller) {
            let error = ControlError::new(
                ErrorCode::RequestConflict,
                format!(
                    "MonoCode is already asking the person to confirm another call from “{}”; retry once it is answered.",
                    call.caller.name()
                ),
            )
            .retryable(true);
            return self.finish_control(call, Err(error), cx);
        }
        let checked = self.control_check(&call.command, cx);
        self.control.next_approval += 1;
        let id = self.control.next_approval;
        let caller = call.caller.clone();
        self.control.confirmations.insert(
            caller.clone(),
            Waiting {
                id,
                call,
                target: None,
                pin: None,
            },
        );
        cx.spawn(async move |this, cx| {
            let checked = checked.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.ask_confirmation(&caller, id, checked, window, cx);
            });
        })
        .detach();
    }

    fn ask_confirmation(
        &mut self,
        caller: &CallerKind,
        id: u64,
        checked: Result<Checked, ControlError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.control.confirmations.get(caller).map(|w| w.id) != Some(id) {
            return;
        }
        let checked = match checked {
            Ok(checked) => checked,
            Err(error) => {
                if let Some(waiting) = self.control.confirmations.remove(caller) {
                    self.finish_control(waiting.call, Err(error), cx);
                }
                return;
            }
        };
        let question = question(&caller.name(), &checked);
        if let Some(waiting) = self.control.confirmations.get_mut(caller) {
            waiting.target = Some(checked.target.clone());
            waiting.pin = checked.into_pin();
        }
        // The person decides where they are: bring the window forward.
        cx.activate(true);
        window.activate_window();
        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            &question.message,
            Some(&question.detail),
            &[question.action, "Cancel"],
            cx,
        );
        let answered = caller.clone();
        cx.spawn(async move |this, cx| {
            let confirmed = prompt.await == Ok(0);
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_confirmation(&answered, id, confirmed, window, cx);
            });
        })
        .detach();
        let timeout = self.control.timeouts.confirm;
        let caller = caller.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(timeout).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_confirmation(&caller, id, false, window, cx);
            });
        })
        .detach();
    }

    /// The prompt was answered or timed out; the first of the two wins.
    fn finish_confirmation(
        &mut self,
        caller: &CallerKind,
        id: u64,
        confirmed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.control.confirmations.get(caller).map(|w| w.id) != Some(id) {
            return;
        }
        let Some(waiting) = self.control.confirmations.remove(caller) else {
            return;
        };
        let target = waiting.target.unwrap_or_default();
        self.record_confirmation(caller, &waiting.call.command, &target, confirmed, cx);
        if !confirmed {
            let error = declined(&caller.name(), waiting.call.command.name());
            return self.finish_control(waiting.call, Err(error), cx);
        }
        // The grant or the target may have changed while the person decided.
        let checked = self
            .control_grant(caller, &waiting.call.command)
            .and_then(|grant| {
                self.control_target(&waiting.call.command, cx)?;
                Ok(grant)
            });
        match checked {
            Ok(grant) => {
                let pin = waiting.pin.as_ref();
                self.execute_control(waiting.call, &grant, pin, window, cx);
            }
            Err(error) => self.finish_control(waiting.call, Err(error), cx),
        }
    }

    /// Control was turned off: every call waiting on a prompt is refused.
    pub(super) fn refuse_confirmations(&mut self, cx: &mut Context<Self>) {
        for (_, waiting) in std::mem::take(&mut self.control.confirmations) {
            self.finish_control(waiting.call, Err(ControlError::control_disabled()), cx);
        }
    }
}
