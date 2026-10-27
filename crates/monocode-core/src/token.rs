//! Per-session tokens: each a digest of a secret made once per app run and
//! the session's id, so it lasts as long as the run and is never stored. The
//! control relay's session credentials are derived so; every check compares
//! the one a caller sends in constant time.

/// Session `session`'s token under `secret`: the hex SHA-256 of
/// `<secret>:<session>`.
pub(crate) fn session_token(secret: &str, session: i64) -> String {
    crate::mono::files::content_hash(&format!("{secret}:{session}"))
}

/// Whether `given` is `expected`, in time that does not depend on where
/// they first differ.
pub(crate) fn same(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_per_session_and_checked_whole() {
        let token = session_token("secret", 7);
        assert_eq!(token.len(), 64);
        assert_ne!(token, session_token("secret", 8));
        assert_ne!(token, session_token("other", 7));
        assert!(same(&token, &session_token("secret", 7)));
        assert!(!same(&token[..63], &token));
        assert!(!same("abc", "abd"));
    }
}
