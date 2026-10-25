//! Who calls: an external client, approved by the person and limited to its
//! scopes, or an agent session the app runs itself. A session's relay says
//! hello with the session's [`SessionCredential`] and the name
//! `session:<id>`, which no external client may take; the app decides who
//! calls from the credential, never from the name.
use serde::{Deserialize, Serialize};
use std::fmt;

/// What every session caller's name starts with.
pub const SESSION_CALLER_PREFIX: &str = "session:";

/// An agent session's credential, which the app hands the session's relay
/// in its environment. The token lasts for one app run and never appears in
/// logs: `Debug` leaves it out.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCredential {
    /// The session's id.
    pub id: i64,
    /// The session's token for this app run.
    pub token: String,
}

impl fmt::Debug for SessionCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionCredential")
            .field("id", &self.id)
            .field("token", &"<hidden>")
            .finish()
    }
}

/// The caller name of session `id`: `session:<id>`.
#[must_use]
pub fn session_caller_name(id: i64) -> String {
    format!("{SESSION_CALLER_PREFIX}{id}")
}

/// Whether `name` is kept for session callers, so an external client may
/// not say hello with it.
#[must_use]
pub fn reserved_client_name(name: &str) -> bool {
    name.starts_with(SESSION_CALLER_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_names_round_trip_and_hide_the_token() {
        assert_eq!(session_caller_name(42), "session:42");
        assert!(reserved_client_name("session:anything"));
        assert!(!reserved_client_name("Claude Code"));
        let credential = SessionCredential {
            id: 42,
            token: "secret-token".into(),
        };
        assert!(!format!("{credential:?}").contains("secret-token"));
    }
}
