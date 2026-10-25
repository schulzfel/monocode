//! The person's external-control policy, as the app stores it.
use crate::{Grant, Scope};
use serde::{Deserialize, Serialize};

/// The store setting holding the [`Policy`].
pub const POLICY_KEY: &str = "external-control.v1";
/// The store setting holding the recent activity ring buffer.
pub const ACTIVITY_KEY: &str = "external-control-activity.v1";
/// The prefix of the store settings holding idempotency receipts.
pub const RECEIPT_PREFIX: &str = "control-receipt:";

/// Whether the MCP server is on, its port, and the approved clients. Only
/// the app's Settings change it; no operation can. Fields it no longer has,
/// such as an earlier permission ceiling, are ignored when a stored policy
/// is read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    /// Off by default. While off, the app serves no MCP over HTTP and
    /// refuses external clients on its control socket.
    pub enabled: bool,
    /// The localhost port the MCP server listens on; `None` is the default
    /// port for the app's data folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Clients the person approved, in approval order.
    pub clients: Vec<ApprovedClient>,
}

/// A client the person approved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovedClient {
    /// The client's reported name.
    pub name: String,
    /// The scopes it holds.
    pub scopes: Vec<Scope>,
    /// When it was approved, in Unix milliseconds.
    pub approved_at: i64,
    /// When it last connected, in Unix milliseconds.
    pub last_seen: i64,
}

impl Policy {
    /// Parses a stored policy; a missing or unreadable value is the default,
    /// which is off.
    #[must_use]
    pub fn parse(raw: Option<&str>) -> Self {
        raw.and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default()
    }
    /// The approved client called `name`.
    #[must_use]
    pub fn client(&self, name: &str) -> Option<&ApprovedClient> {
        self.clients.iter().find(|c| c.name == name)
    }
    /// The approved client called `name`, to change.
    pub fn client_mut(&mut self, name: &str) -> Option<&mut ApprovedClient> {
        self.clients.iter_mut().find(|c| c.name == name)
    }
    /// The grant of the approved client called `name`.
    #[must_use]
    pub fn grant(&self, name: &str) -> Option<Grant> {
        self.client(name).map(|client| Grant {
            scopes: client.scopes.clone(),
        })
    }
    /// Approves `name` with the default scopes, or refreshes its last-seen time.
    pub fn approve(&mut self, name: &str, now: i64) -> &ApprovedClient {
        if let Some(index) = self.clients.iter().position(|c| c.name == name) {
            self.clients[index].last_seen = now;
            return &self.clients[index];
        }
        self.clients.push(ApprovedClient {
            name: name.to_owned(),
            scopes: Scope::DEFAULT_GRANT.to_vec(),
            approved_at: now,
            last_seen: now,
        });
        &self.clients[self.clients.len() - 1]
    }
    /// Turns one scope of `name` on or off. Returns whether the client exists.
    pub fn set_scope(&mut self, name: &str, scope: Scope, on: bool) -> bool {
        let Some(client) = self.client_mut(name) else {
            return false;
        };
        client.scopes.retain(|s| *s != scope);
        if on {
            client.scopes.push(scope);
            client.scopes.sort();
        }
        true
    }
    /// Removes `name`. Returns whether it was approved.
    pub fn revoke(&mut self, name: &str) -> bool {
        let before = self.clients.len();
        self.clients.retain(|c| c.name != name);
        self.clients.len() != before
    }
}

/// Whether `name` is an acceptable client name: 1 to 128 characters, no
/// control characters, not only whitespace.
#[must_use]
pub fn valid_client_name(name: &str) -> bool {
    (1..=128).contains(&name.chars().count())
        && !name.trim().is_empty()
        && !name.chars().any(char::is_control)
}
