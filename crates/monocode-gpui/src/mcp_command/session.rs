//! The relay inside one of MonoCode's own agent sessions. MonoCode puts the
//! control endpoint and the session's credential in the provider's
//! environment (`monocode_core::app_mcp`); the relay then connects there
//! and speaks for that session instead of as an external client.
use monocode_control::{ControlError, Endpoint, SessionCredential};
use monocode_core::app_mcp::{ENV_ENDPOINT, ENV_SESSION, ENV_TOKEN};

/// The session this relay speaks for, read with `var`: `None` when none of
/// the variables is set (an external client), an error when some are
/// missing or malformed, so a broken session relay never calls as an
/// external client.
pub(super) fn from_env(
    var: impl Fn(&str) -> Option<String>,
) -> Option<Result<(Endpoint, SessionCredential), ControlError>> {
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
    let read = || {
        let endpoint = Endpoint::parse(&endpoint.ok_or_else(incomplete)?)?;
        let id = session
            .and_then(|id| id.parse().ok())
            .ok_or_else(incomplete)?;
        let token = token.ok_or_else(incomplete)?;
        Ok((endpoint, SessionCredential { id, token }))
    };
    Some(read())
}
