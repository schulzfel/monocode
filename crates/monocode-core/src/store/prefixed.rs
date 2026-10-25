//! Settings kept one record per key under a shared prefix, such as external
//! control's idempotency receipts, so their owner can list and prune them.
use super::Store;
use rusqlite::params;

/// The first key after every key that starts with `prefix`, if any: keys
/// compare byte by byte, which for UTF-8 is by code point.
fn after(prefix: &str) -> Option<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    while let Some(last) = chars.pop() {
        let next = match last {
            '\u{D7FF}' => Some('\u{E000}'),
            last => char::from_u32(u32::from(last) + 1),
        };
        if let Some(next) = next {
            chars.push(next);
            return Some(chars.into_iter().collect());
        }
    }
    None
}

/// The key range of `prefix`, for the primary key index.
const RANGE: &str = "key >= ?1 AND (?2 IS NULL OR key < ?2)";

impl Store {
    /// Every setting whose key starts with `prefix`, with its key.
    pub fn settings_under(&self, prefix: &str) -> Result<Vec<(String, String)>, String> {
        if self.shared.is_some() {
            return Ok(self.shared_settings_under(prefix));
        }
        let mut statement = self
            .conn
            .prepare(&format!(
                "SELECT key, value FROM native_settings WHERE {RANGE}"
            ))
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map(params![prefix, after(prefix)], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }
    /// How many settings have a key that starts with `prefix`.
    pub fn count_settings_under(&self, prefix: &str) -> Result<usize, String> {
        if self.shared.is_some() {
            return Ok(self.shared_count_settings_under(prefix));
        }
        self.conn
            .query_row(
                &format!("SELECT COUNT(*) FROM native_settings WHERE {RANGE}"),
                params![prefix, after(prefix)],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| usize::try_from(count).unwrap_or(0))
            .map_err(|e| e.to_string())
    }
    /// Removes the settings named `keys`; keys that are not set are skipped.
    pub fn delete_settings(&self, keys: &[String]) -> Result<(), String> {
        if self.shared.is_some() {
            return self.shared_delete_settings(keys);
        }
        let transaction = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        for key in keys {
            transaction
                .execute("DELETE FROM native_settings WHERE key = ?1", params![key])
                .map_err(|e| e.to_string())?;
        }
        transaction.commit().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn settings_under_a_prefix_are_listed_and_deleted_and_no_others() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        for (key, value) in [
            ("control-receipt:a:1", "one"),
            ("control-receipt:a:2", "two"),
            ("control-receipt:b:1", "other"),
            ("control-receipt_a:3", "a lookalike"),
        ] {
            store.save_setting(key, value).unwrap();
        }
        assert_eq!(store.count_settings_under("control-receipt:a:").unwrap(), 2);
        assert_eq!(store.count_settings_under("control-receipt:").unwrap(), 3);
        let mut listed = store.settings_under("control-receipt:a:").unwrap();
        listed.sort();
        assert_eq!(
            listed,
            [
                ("control-receipt:a:1".to_owned(), "one".to_owned()),
                ("control-receipt:a:2".to_owned(), "two".to_owned())
            ],
            "the prefix is literal: `_` matches only itself"
        );
        store
            .delete_settings(&["control-receipt:a:1".into(), "never-set".into()])
            .unwrap();
        assert_eq!(store.setting("control-receipt:a:1").unwrap(), None);
        assert_eq!(
            store.setting("control-receipt:a:2").unwrap().as_deref(),
            Some("two")
        );
        assert_eq!(
            store.setting("control-receipt:b:1").unwrap().as_deref(),
            Some("other")
        );
    }

    #[test]
    fn the_range_after_a_prefix_holds_exactly_its_keys() {
        assert_eq!(after("a:").as_deref(), Some("a;"));
        assert_eq!(after("a\u{D7FF}").as_deref(), Some("a\u{E000}"));
        assert_eq!(after("a\u{10FFFF}").as_deref(), Some("b"));
        assert_eq!(after(""), None);
        let store = Store::open(Path::new(":memory:")).unwrap();
        store.save_setting("z", "last").unwrap();
        assert_eq!(store.count_settings_under("").unwrap(), 1, "no upper bound");
    }
}
