//! Session credentials for the control relay. Each session's token is a
//! digest of a secret made once per app run and the session's id
//! (`token::session_token`): it lasts as long as the run, is never stored,
//! and verifies in constant time.
use std::sync::OnceLock;

/// This run's secret; a restarted app gives every session a new token.
fn secret() -> &'static str {
    static SECRET: OnceLock<String> = OnceLock::new();
    SECRET.get_or_init(|| {
        format!(
            "{:032x}{:032x}",
            rand::random::<u128>(),
            rand::random::<u128>()
        )
    })
}

/// Session `session`'s token in this run.
#[must_use]
pub fn token(session: i64) -> String {
    crate::token::session_token(secret(), session)
}

/// Whether `token` is session `session`'s token in this run.
#[must_use]
pub fn verify(session: i64, token: &str) -> bool {
    crate::token::same(token, &self::token(session))
}
