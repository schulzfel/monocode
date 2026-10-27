//! The relay inside one of MonoCode's own agent sessions. MonoCode puts the
//! control endpoint and the session's credential in the provider's
//! environment (`monocode_core::app_mcp`); the relay then connects there
//! and speaks for that session instead of as an external client, as the
//! `app` command (`app_cli`) does. The session's role
//! (`MONOCODE_CONTROL_ROLE`) only picks the tools it lists.
use monocode_control::{ControlError, Endpoint, SessionCredential};
use monocode_core::app_mcp::{ENV_ENDPOINT, ENV_ROLE, ENV_SESSION, ENV_TOKEN, ROLE_MONO};
use monocode_mcp::Role;

/// The session a relay speaks for. `Debug` leaves the token out, as the
/// credential's does.
#[derive(Debug)]
pub(crate) struct SessionRelay {
    /// The window's control endpoint.
    pub(crate) endpoint: Endpoint,
    /// The session's credential.
    pub(crate) credential: SessionCredential,
    /// Whether it is a Mono's chat or habit run, as the environment hints:
    /// its relay lists the Mono's own tools too. Anything but `mono`, or
    /// nothing, is a session.
    pub(crate) role: Role,
}

/// The session this relay speaks for, read with `var`: `None` when none of
/// the credential's variables is set (an external client, whatever its
/// role says), an error when some are missing or malformed, so a broken
/// session relay never calls as an external client.
pub(crate) fn from_env(
    var: impl Fn(&str) -> Option<String>,
) -> Option<Result<SessionRelay, ControlError>> {
    let [endpoint, session, token] =
        [ENV_ENDPOINT, ENV_SESSION, ENV_TOKEN].map(|key| var(key).filter(|v| !v.is_empty()));
    if endpoint.is_none() && session.is_none() && token.is_none() {
        return None;
    }
    let incomplete = || {
        ControlError::invalid_input(format!(
            "MonoCode's session relay needs {ENV_ENDPOINT}, {ENV_SESSION} and {ENV_TOKEN}; one is missing or malformed."
        ))
    };
    let role = if var(ENV_ROLE).as_deref() == Some(ROLE_MONO) {
        Role::Mono
    } else {
        Role::Session
    };
    let read = || {
        let endpoint = Endpoint::parse(&endpoint.ok_or_else(incomplete)?)?;
        let id = session
            .and_then(|id| id.parse().ok())
            .ok_or_else(incomplete)?;
        let token = token.ok_or_else(incomplete)?;
        Ok(SessionRelay {
            endpoint,
            credential: SessionCredential { id, token },
            role,
        })
    };
    Some(read())
}
