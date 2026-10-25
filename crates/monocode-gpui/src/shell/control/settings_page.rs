//! What Settings → General → MCP server shows of external control, and the
//! changes it makes: the only way the policy changes.
use super::*;

impl Shell {
    /// Applies a change made in Settings, keeps it, and listens or stops.
    pub(in crate::shell) fn edit_control(
        &mut self,
        edit: &ExternalControlEdit,
        cx: &mut Context<Self>,
    ) {
        let Control { policy, denied, .. } = &mut self.control;
        match edit {
            ExternalControlEdit::Enabled(on) => policy.enabled = *on,
            ExternalControlEdit::Port(port) => policy.port = Some(*port),
            ExternalControlEdit::Scope { client, scope, on } => {
                policy.set_scope(client, *scope, *on);
            }
            ExternalControlEdit::Revoke(client) => {
                policy.revoke(client);
                denied.remove(client);
            }
            ExternalControlEdit::Allow(client) => {
                policy.approve(client, now_ms());
                denied.remove(client);
            }
            ExternalControlEdit::ClearActivity => {
                self.clear_activity(cx);
                self.sync_settings(cx);
                return;
            }
        }
        if let ExternalControlEdit::Allow(client) = edit {
            self.record_decision(client, true, cx);
        }
        self.save_control_policy(cx);
        self.control_refuse_followers(cx);
        self.apply_control(cx);
        self.sync_settings(cx);
    }

    pub(super) fn save_control_policy(&self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let Ok(policy) = serde_json::to_string(&self.control.policy) else {
            return;
        };
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = locked(&store).and_then(|s| s.save_setting(POLICY_KEY, &policy))
                {
                    eprintln!("Cannot save the external control policy: {error}");
                }
            })
            .detach();
    }

    /// The MCP server as Settings shows it.
    pub(in crate::shell) fn control_status(&self) -> ExternalControlStatus {
        ExternalControlStatus {
            enabled: self.control.policy.enabled,
            url: self.mcp_url(),
            port: self.mcp_port(),
            error: self.control.http_error.clone(),
            clients: self.control.policy.clients.clone(),
            activity: self.activity_rows(),
            denied: self.denied_clients(),
        }
    }
}
