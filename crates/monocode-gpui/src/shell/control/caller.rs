//! Who calls, decided once when the hello is answered: an external client
//! the person approved, or an agent session MonoCode runs, which is a Mono's
//! main chat or running habit when the Mono owns it. Its grant, whether the
//! person confirms its calls, whether it may follow topics, who a session it
//! starts is started by, and what the activity log names all follow from
//! that.
use super::*;
use crate::shell::session_launch::Origin;
use monocode_control::{ClientInfo, SessionCredential, session_caller_name};

/// What a Mono's chat that was reset, or a habit run that ended, is told,
/// in the old bridge's words.
pub(super) const MONO_GONE: &str = "This Mono no longer owns an active app capability";

/// Who calls.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum CallerKind {
    /// An external client, by the name it said hello with.
    External(String),
    /// Agent session `id`, whose relay said hello with its credential.
    Session(i64),
    /// Session `session`, the main chat (or, with `run`, a running habit)
    /// of Mono `mono`.
    Mono {
        session: i64,
        mono: String,
        run: bool,
    },
}

impl CallerKind {
    /// The caller a hello names: the session its credential is for, else
    /// the client's name. Whether that session is a Mono's is the shell's
    /// to tell ([`Shell::resolve_caller`]).
    pub(super) fn of_hello(client: &ClientInfo, session: Option<&SessionCredential>) -> Self {
        session.map_or_else(
            || Self::External(client.name.clone()),
            |credential| Self::Session(credential.id),
        )
    }
    /// The name its receipts, prompts and running calls are kept under:
    /// the client's, or `session:<id>`, which no client may take. A Mono
    /// calls under its session's name.
    pub(super) fn name(&self) -> String {
        match self {
            Self::External(name) => name.clone(),
            Self::Session(id) | Self::Mono { session: id, .. } => session_caller_name(*id),
        }
    }
    /// Whether the person confirms its deletes, discards and pushes in the
    /// window; a session's own provider decides for it instead.
    pub(super) fn confirms(&self) -> bool {
        match self {
            Self::External(_) => true,
            Self::Session(_) | Self::Mono { .. } => false,
        }
    }
    /// Who a session it starts is started by: a session's launches are
    /// that session's, as Operator's are; a Mono's are its own launches.
    pub(super) fn origin(&self) -> Origin {
        match self {
            Self::External(name) => Origin::Client(format!("mcp:{name}")),
            Self::Session(id) => Origin::Session(*id),
            Self::Mono { session, mono, .. } => Origin::Mono {
                caller: *session,
                mono: mono.clone(),
            },
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
/// needs: a session or Mono holds every scope (whether it still exists is
/// the shell's to check); a client the scopes the person approved, while
/// control is on.
pub(super) fn authorize(
    policy: &Policy,
    caller: &CallerKind,
    scope: Scope,
    what: &str,
) -> Result<Grant, ControlError> {
    let client = match caller {
        CallerKind::Session(_) | CallerKind::Mono { .. } => return Ok(session_grant()),
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

/// A Mono's session that it no longer calls from.
fn mono_gone() -> ControlError {
    ControlError::new(ErrorCode::NotFound, MONO_GONE)
}

impl Shell {
    /// Who `caller` is now: a session's credential stands for the Mono
    /// whose main chat or running habit the session is, as the bridge looked
    /// it up (MO-7), else for the session itself. A Mono's session it no
    /// longer owns (its chat was reset, or the run ended) is refused.
    pub(super) fn resolve_caller(
        &self,
        caller: &CallerKind,
        cx: &App,
    ) -> Result<CallerKind, ControlError> {
        let id = match caller {
            CallerKind::External(_) => return Ok(caller.clone()),
            CallerKind::Session(id) | CallerKind::Mono { session: id, .. } => *id,
        };
        let monos = self.monos.as_ref().map(|m| m.read(cx));
        match monos.and_then(|m| m.caller_of(id)) {
            Some((mono, run)) => Ok(CallerKind::Mono {
                session: id,
                mono,
                run,
            }),
            None if monos.is_some_and(|m| m.is_mono_session(id))
                || matches!(caller, CallerKind::Mono { .. }) =>
            {
                Err(mono_gone())
            }
            None => Ok(CallerKind::Session(id)),
        }
    }

    /// Checks that Mono `mono` still calls from session `session`, as its
    /// main chat or (`run`) a running habit.
    pub(super) fn mono_caller_exists(
        &self,
        session: i64,
        mono: &str,
        run: bool,
        cx: &App,
    ) -> Result<(), ControlError> {
        let owner = self
            .monos
            .as_ref()
            .and_then(|m| m.read(cx).caller_of(session));
        if owner.is_some_and(|(owner, is_run)| owner == mono && is_run == run) {
            Ok(())
        } else {
            Err(mono_gone())
        }
    }

    /// `caller`'s grant, if it holds `scope`, which `what` needs; a session
    /// caller only while its session exists, a Mono only while it still
    /// calls from its session.
    pub(super) fn control_scope(
        &self,
        caller: &CallerKind,
        scope: Scope,
        what: &str,
        cx: &App,
    ) -> Result<Grant, ControlError> {
        match caller {
            CallerKind::Session(id) => self.session_exists(*id)?,
            CallerKind::Mono { session, mono, run } => {
                self.mono_caller_exists(*session, mono, *run, cx)?;
            }
            CallerKind::External(_) => {}
        }
        authorize(&self.control.policy, caller, scope, what)
    }

    /// Who the activity log names: a session caller by its session's title
    /// (or `session <id>`), a Mono by its name (a habit run by its run's
    /// title, "{Mono} · {habit}"), any other client by its name.
    pub(super) fn caller_label(&self, caller: &CallerKind, cx: &App) -> String {
        let id = match caller {
            CallerKind::External(name) => return name.clone(),
            CallerKind::Mono { session, mono, run } => {
                let label = self
                    .monos
                    .as_ref()
                    .and_then(|m| m.read(cx).caller_label(*session, mono, *run, cx));
                return label
                    .filter(|label| !label.trim().is_empty())
                    .unwrap_or_else(|| format!("session {session}"));
            }
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
            // A Mono's launch is recorded in its roster instead (§7.2).
            Origin::Mono { .. } => {}
        }
    }
}
