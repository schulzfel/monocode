//! Hellos: a session's relay is answered by its credential alone
//! (`session_caller`); known clients get their stored grant; the first
//! hello from an unknown name asks the person in the window, for at most
//! two minutes. A client the person denied is refused without a prompt (and
//! without bringing the window forward) for [`DENIAL_MEMORY`], or until
//! Settings allows or revokes it.
use super::*;
use monocode_control::{ClientInfo, SessionCredential, reserved_client_name, valid_client_name};
use std::time::Duration;

/// How long the approval prompt waits before the hello is declined.
pub(super) const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a client the person denied is refused without asking again.
pub(super) const DENIAL_MEMORY: Duration = Duration::from_secs(600);

const APPROVAL_DETAIL: &str = "It will be able to list and open your projects and sessions, start, message and stop agent sessions, and organize projects. Changing settings, source control and deleting stay off until you turn them on in Settings → General → MCP server.";

/// Hellos from one client waiting on the same prompt.
pub(super) struct Approval {
    id: u64,
    waiting: Vec<SyncSender<Result<Grant, ControlError>>>,
}

impl Approval {
    pub(super) fn refuse(self, error: &ControlError) {
        for reply in self.waiting {
            let _ = reply.send(Err(error.clone()));
        }
    }
}

impl Shell {
    pub(super) fn control_hello(
        &mut self,
        client: ClientInfo,
        session: Option<SessionCredential>,
        reply: SyncSender<Result<Grant, ControlError>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(credential) = session {
            let _ = reply.send(self.session_hello(&credential));
            return;
        }
        if !self.control.policy.enabled {
            let _ = reply.send(Err(ControlError::control_disabled()));
            return;
        }
        if !valid_client_name(&client.name) {
            let _ = reply.send(Err(ControlError::invalid_input(
                "The client name must be 1 to 128 printable characters",
            )));
            return;
        }
        if reserved_client_name(&client.name) {
            let _ = reply.send(Err(ControlError::invalid_input(
                "Client names starting with “session:” belong to MonoCode's own agent sessions; choose another name (--client NAME)",
            )));
            return;
        }
        let name = client.name;
        if self.control.denied.contains_key(&name) && self.control.policy.client(&name).is_none() {
            let _ = reply.send(Err(ControlError::new(
                ErrorCode::UserDeclined,
                format!(
                    "The person denied “{name}” in MonoCode recently, so it is not asked again for now. They can allow it in Settings → General → MCP server."
                ),
            )));
            return;
        }
        if self.control.policy.client(&name).is_some() {
            self.control.policy.approve(&name, now_ms());
            self.save_control_policy(cx);
            self.sync_settings(cx);
            let _ = reply.send(
                self.control
                    .policy
                    .grant(&name)
                    .ok_or_else(|| ControlError::internal("The approval was lost")),
            );
            return;
        }
        if let Some(approval) = self.control.approvals.get_mut(&name) {
            approval.waiting.push(reply);
            return;
        }
        self.control.next_approval += 1;
        let id = self.control.next_approval;
        self.control.approvals.insert(
            name.clone(),
            Approval {
                id,
                waiting: vec![reply],
            },
        );
        // The person decides where they are: bring the window forward.
        cx.activate(true);
        window.activate_window();
        let prompt = window.prompt(
            gpui::PromptLevel::Info,
            &format!("Allow “{name}” to control MonoCode?"),
            Some(APPROVAL_DETAIL),
            &["Allow", "Deny"],
            cx,
        );
        let answered = name.clone();
        cx.spawn(async move |this, cx| {
            let answer = Some(prompt.await == Ok(0));
            let _ = this.update(cx, |this, cx| {
                this.finish_approval(&answered, id, answer, cx);
            });
        })
        .detach();
        let timeout = self.control.timeouts.approval;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(timeout).await;
            let _ = this.update(cx, |this, cx| this.finish_approval(&name, id, None, cx));
        })
        .detach();
    }

    /// Remembers that the person denied `name`, for [`DENIAL_MEMORY`].
    fn remember_denial(&mut self, name: &str, cx: &mut Context<Self>) {
        self.control.next_approval += 1;
        let denial = self.control.next_approval;
        self.control.denied.insert(name.to_owned(), denial);
        let memory = self.control.timeouts.denial_memory;
        let name = name.to_owned();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(memory).await;
            let _ = this.update(cx, |this, cx| {
                if this.control.denied.get(&name) == Some(&denial) {
                    this.control.denied.remove(&name);
                    this.sync_settings(cx);
                }
            });
        })
        .detach();
    }

    /// Denied clients, as Settings lists them.
    pub(super) fn denied_clients(&self) -> Vec<String> {
        let mut names: Vec<String> = self.control.denied.keys().cloned().collect();
        names.sort();
        names
    }

    /// The prompt was answered (`Some`: allowed or not) or timed out
    /// (`None`); the first of the two wins. Only the person's own Deny is
    /// remembered: a prompt nobody answered asks again next time.
    fn finish_approval(
        &mut self,
        name: &str,
        id: u64,
        answer: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let allowed = answer == Some(true);
        if self.control.approvals.get(name).map(|a| a.id) != Some(id) {
            return;
        }
        let Some(approval) = self.control.approvals.remove(name) else {
            return;
        };
        if !(allowed && self.control.policy.enabled) {
            if answer == Some(false) {
                self.remember_denial(name, cx);
            }
            self.record_decision(name, false, cx);
            approval.refuse(&ControlError::new(
                ErrorCode::UserDeclined,
                format!("MonoCode did not allow “{name}” to control it."),
            ));
            return;
        }
        self.control.policy.approve(name, now_ms());
        self.save_control_policy(cx);
        self.record_decision(name, true, cx);
        self.sync_settings(cx);
        let grant = self.control.policy.grant(name);
        for reply in approval.waiting {
            let _ = reply.send(
                grant
                    .clone()
                    .ok_or_else(|| ControlError::internal("The approval was lost")),
            );
        }
    }
}
