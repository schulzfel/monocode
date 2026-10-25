//! Who calls, decided once when the hello is answered: an external client
//! the person approved, or an agent session MonoCode runs. Its grant,
//! whether the person confirms its calls, whether it may follow topics, who
//! a session it starts is started by, and what the activity log names all
//! follow from that.
use super::*;
use crate::shell::session_launch::Origin;
use monocode_control::{ClientInfo, SessionCredential, session_caller_name};

/// Who calls.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum CallerKind {
    /// An external client, by the name it said hello with.
    External(String),
    /// Agent session `id`, whose relay said hello with its credential.
    Session(i64),
}

impl CallerKind {
    /// The caller a hello names: the session its credential is for, else
    /// the client's name.
    pub(super) fn of_hello(client: &ClientInfo, session: Option<&SessionCredential>) -> Self {
        session.map_or_else(
            || Self::External(client.name.clone()),
            |credential| Self::Session(credential.id),
        )
    }
    /// The name its receipts, prompts and running calls are kept under:
    /// the client's, or `session:<id>`, which no client may take.
    pub(super) fn name(&self) -> String {
        match self {
            Self::External(name) => name.clone(),
            Self::Session(id) => session_caller_name(*id),
        }
    }
    /// Whether the person confirms its deletes, discards and pushes in the
    /// window; a session's own provider decides for it instead.
    pub(super) fn confirms(&self) -> bool {
        matches!(self, Self::External(_))
    }
    /// Who a session it starts is started by: a session's launches are
    /// that session's, as Operator's are.
    pub(super) fn origin(&self) -> Origin {
        match self {
            Self::External(name) => Origin::Client(format!("mcp:{name}")),
            Self::Session(id) => Origin::Session(*id),
        }
    }
}

/// Tests call as the names they say hello with.
#[cfg(test)]
impl From<&str> for CallerKind {
    fn from(name: &str) -> Self {
        name.strip_prefix(monocode_control::SESSION_CALLER_PREFIX)
            .and_then(|id| id.parse().ok())
            .map_or_else(|| Self::External(name.to_owned()), Self::Session)
    }
}

/// What a session caller holds: every scope.
pub(super) fn session_grant() -> Grant {
    Grant {
        scopes: Scope::ALL.to_vec(),
    }
}

/// `caller`'s grant under `policy`, if it holds `scope`, which `what`
/// needs: a session holds every scope (whether it still exists is the
/// shell's to check); a client the scopes the person approved, while
/// control is on.
pub(super) fn authorize(
    policy: &Policy,
    caller: &CallerKind,
    scope: Scope,
    what: &str,
) -> Result<Grant, ControlError> {
    let client = match caller {
        CallerKind::Session(_) => return Ok(session_grant()),
        CallerKind::External(client) => client,
    };
    if !policy.enabled {
        return Err(ControlError::control_disabled());
    }
    let grant = policy.grant(client).ok_or_else(|| {
        ControlError::new(
            ErrorCode::ScopeDenied,
            format!("“{client}” is no longer approved in MonoCode. Reconnect to ask again."),
        )
    })?;
    if !grant.allows(scope) {
        return Err(ControlError::new(
            ErrorCode::ScopeDenied,
            format!(
                "{what} needs the {} scope, which “{client}” does not hold. Grant it in Settings → General → MCP server.",
                scope.id()
            ),
        ));
    }
    Ok(grant)
}

impl Shell {
    /// `caller`'s grant, if it holds `scope`, which `what` needs; a session
    /// caller only while its session exists.
    pub(super) fn control_scope(
        &self,
        caller: &CallerKind,
        scope: Scope,
        what: &str,
    ) -> Result<Grant, ControlError> {
        if let CallerKind::Session(id) = caller {
            self.session_exists(*id)?;
        }
        authorize(&self.control.policy, caller, scope, what)
    }

    /// Who the activity log names: a session caller by its session's title
    /// (or `session <id>`), any other client by its name.
    pub(super) fn caller_label(&self, caller: &CallerKind, cx: &App) -> String {
        let id = match caller {
            CallerKind::External(name) => return name.clone(),
            CallerKind::Session(id) => *id,
        };
        let title = self
            .session_exists(id)
            .is_ok()
            .then(|| self.session_metadata(id, cx).display_title());
        title
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| format!("session {id}"))
    }

    /// Records who started session `id`.
    pub(in crate::shell) fn record_started(&mut self, id: i64, origin: Origin) {
        match origin {
            Origin::Session(caller) => {
                self.started_by.insert(id, caller);
            }
            Origin::Client(client) => {
                self.control.started.insert(id, client);
            }
        }
    }
}
