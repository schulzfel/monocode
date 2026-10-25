use super::{Session, Store};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Settings naming who started a session from outside the app, by session.
/// Distinct from `started-by:`, which holds a caller session's numeric ID.
const STARTED_CLIENT_PREFIX: &str = "started-by-client:";

impl Store {
    /// A launch is recoverable as an unsent draft if the app exits before its
    /// provider accepts it. Queue delivery uses this same stable message ID.
    pub fn prepare_app_prompt(&self, id: i64, request: &str, prompt: &str) -> Result<(), String> {
        let mut state = self.follow_ups(id)?;
        let message_id = format!("app-prompt-{}", crate::mono::files::content_hash(request));
        if state.draft.is_none() && !state.messages.iter().any(|m| m.id == message_id) {
            state.revision += 1;
            state.draft = Some(crate::follow_up::Message {
                id: message_id,
                text: prompt.into(),
                ..Default::default()
            });
            self.save_follow_ups(id, &state)?;
        }
        Ok(())
    }
    /// Reserve a request's child durably before its UI/provider is installed.
    /// Shared stores use a deterministic desktop ID; legacy stores reserve their
    /// numeric ID and mapping in the same SQLite transaction.
    pub fn create_app_session(
        &self,
        caller: i64,
        request: &str,
        fingerprint: &str,
        provider: &str,
        project: &str,
    ) -> Result<(Session, bool), String> {
        let key = format!(
            "operator-app-launch:{caller}:{}",
            crate::mono::files::content_hash(request)
        );
        let original = format!(
            "operator-{}",
            crate::mono::files::content_hash(&format!("{caller}:{request}"))
        );
        self.create_keyed_session(&key, &original, fingerprint, provider, project)
    }
    /// [`Self::create_app_session`] for any launcher: `key` is the setting
    /// that remembers the launch and its input `fingerprint`, and `original`
    /// names the session deterministically in shared stores. The same key
    /// returns the session it created; the same key with another fingerprint
    /// is refused.
    pub fn create_keyed_session(
        &self,
        key: &str,
        original: &str,
        fingerprint: &str,
        provider: &str,
        project: &str,
    ) -> Result<(Session, bool), String> {
        let previous = self
            .setting(key)?
            .map(|raw| serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string()))
            .transpose()?;
        if previous
            .as_ref()
            .is_some_and(|v| v["fingerprint"] != fingerprint)
        {
            return Err("Request ID was already used with different input".into());
        }
        if let Some(id) = previous.as_ref().and_then(|v| v["sessionId"].as_i64()) {
            return self
                .sessions()?
                .into_iter()
                .find(|s| s.id == id)
                .map(|s| (s, false))
                .ok_or("The session created by this request was removed".into());
        }
        if self.shared.is_some() {
            self.save_setting(key, &json!({"fingerprint":fingerprint}).to_string())?;
            let result = self.shared_create_named(provider, project, original)?;
            self.save_setting(
                key,
                &json!({"fingerprint":fingerprint,"sessionId":result.0.id}).to_string(),
            )?;
            return Ok(result);
        }
        let transaction = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let session = self.create(provider, project)?;
        self.save_setting(
            key,
            &json!({"fingerprint":fingerprint,"sessionId":session.id}).to_string(),
        )?;
        transaction.commit().map_err(|e| e.to_string())?;
        Ok((session, true))
    }

    /// Each session an external control client started, and that client as
    /// `mcp:<name>`.
    pub fn started_clients(&self) -> Result<HashMap<i64, String>, String> {
        Ok(self
            .settings_with_prefix(STARTED_CLIENT_PREFIX)?
            .into_iter()
            .collect())
    }
    pub fn save_started_client(&self, child: i64, client: &str) -> Result<(), String> {
        self.save_setting(&format!("{STARTED_CLIENT_PREFIX}{child}"), client)
    }
    /// Session `session`'s rows from its last user message on, which its
    /// last reply is read from; all its rows when it has none. Only the
    /// app's own store reads just those rows.
    pub fn last_turn_rows(&self, session: i64) -> Result<Vec<crate::transcript::Row>, String> {
        if self.shared.is_some() || self.remote_session(session)?.is_some() {
            let mut rows = self.load(session)?.rows;
            let start = rows.iter().rposition(|row| row.role == "user").unwrap_or(0);
            return Ok(rows.split_off(start));
        }
        let mut statement = self
            .conn
            .prepare(
                "SELECT role,body,first,sent_at FROM native_rows WHERE session_id=?1 AND row_ix >= \
                 COALESCE((SELECT MAX(row_ix) FROM native_rows WHERE session_id=?1 AND role='user'), 0) \
                 ORDER BY row_ix",
            )
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([session], |r| {
                Ok(crate::transcript::Row {
                    role: r.get(0)?,
                    text: r.get(1)?,
                    first: r.get(2)?,
                    sent_at: r
                        .get::<_, Option<i64>>(3)?
                        .and_then(|at| u64::try_from(at).ok()),
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    pub fn app_conversation(
        &self,
        id: i64,
        input: &Value,
        busy: bool,
        has_draft: bool,
    ) -> Result<Value, String> {
        let blocks = if self.shared.is_some() {
            self.shared_blocks(id)?
        } else {
            let mut blocks: Vec<Value> = Vec::new();
            for (i, row) in self.load(id)?.rows.into_iter().enumerate() {
                if !row.first && !blocks.is_empty() {
                    let block = blocks.last_mut().unwrap();
                    let text = block["text"].as_str().unwrap_or("").to_owned() + &row.text;
                    block["text"] = json!(text);
                } else {
                    blocks.push(json!({"id":format!("row-{i}"),"role":row.role,"text":row.text}));
                }
            }
            blocks
        };
        let mut page = crate::app_access::conversation_page(&blocks, input)?;
        page["sessionId"] = json!(id);
        page["busy"] = json!(busy);
        page["hasDraft"] = json!(has_draft || blocks.iter().any(|b| b["draft"] == true));
        if let Some(session) = self.sessions()?.into_iter().find(|s| s.id == id) {
            page["title"] = json!(session.title);
        }
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use crate::{store::Store, transcript::Row};
    use std::path::Path;

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }

    #[test]
    fn the_last_turn_is_read_from_the_last_user_message_on() {
        let mut store = Store::open(Path::new(":memory:")).unwrap();
        let session = store.create("codex", "/work").unwrap().id;
        assert!(store.last_turn_rows(session).unwrap().is_empty());
        let rows = [
            row("assistant", "Before any prompt"),
            row("user", "First"),
            row("assistant", "One"),
            row("user", "Second"),
            row("assistant", "Two"),
            row("tool", "{}"),
        ];
        store.write_rows(session, 0, &rows[..1]).unwrap();
        assert_eq!(store.last_turn_rows(session).unwrap(), rows[..1]);
        store.write_rows(session, 0, &rows).unwrap();
        let all = store.load(session).unwrap().rows;
        let start = all.iter().rposition(|row| row.role == "user").unwrap();
        assert_eq!(store.last_turn_rows(session).unwrap(), all[start..]);
        assert_eq!(store.last_turn_rows(session).unwrap()[0].text, "Second");
    }
}
