//! Settings under a key prefix in the desktop store, where native settings
//! live in one JSON file beside the database.
use super::{atomic_json, Store};

impl Store {
    pub(in crate::store) fn shared_settings_under(&self, prefix: &str) -> Vec<(String, String)> {
        self.shared
            .as_ref()
            .unwrap()
            .extras
            .borrow()
            .range(prefix.to_owned()..)
            .take_while(|(key, _)| key.starts_with(prefix))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }
    pub(in crate::store) fn shared_count_settings_under(&self, prefix: &str) -> usize {
        self.shared
            .as_ref()
            .unwrap()
            .extras
            .borrow()
            .range(prefix.to_owned()..)
            .take_while(|(key, _)| key.starts_with(prefix))
            .count()
    }
    pub(in crate::store) fn shared_delete_settings(&self, keys: &[String]) -> Result<(), String> {
        self.shared_writable()?;
        let shared = self.shared.as_ref().unwrap();
        let mut next = shared.extras.borrow().clone();
        let before = next.len();
        for key in keys {
            next.remove(key);
        }
        if next.len() == before {
            return Ok(());
        }
        if let Some(path) = &shared.extras_path {
            atomic_json(path, &next)?;
        }
        *shared.extras.borrow_mut() = next;
        Ok(())
    }
}
