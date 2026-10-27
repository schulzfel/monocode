//! Desktop work references belong to conversation blocks, not the Mono roster.
//! Keep projection separate from persistence so opening a Mono never rewrites it.
use crate::{
    app_mcp::CONTROL_SERVER,
    tool::{ToolCall, ToolState},
};
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn session_handle(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().map(crate::store::desktop_session_handle))
}

pub(crate) fn document_cards(block: &Value) -> Vec<Value> {
    block["artifactCards"].as_array().into_iter().flatten().filter_map(|card| {
        let id = card["id"].as_str()?;
        let kind = card["kind"].as_str()?;
        if id.is_empty() || !matches!(kind, "document" | "html") { return None; }
        Some(json!({"id":format!("artifact:{id}"),"type":"document","documentId":id,
            "kind":kind,"title":card["title"].as_str().unwrap_or("Document"),"summary":card["summary"]}))
    }).collect()
}

fn launch(value: &Value) -> Option<Value> {
    let id = value["sessionId"].as_str().or(value["id"].as_str())?;
    if id.is_empty()
        || id.len() > 512
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return None;
    }
    let cwd = value["cwd"].as_str()?.trim();
    let model = value["model"].as_str()?.trim();
    let harness = value["harness"].as_str()?;
    // Match desktop HARNESSES, including providers not yet executable natively.
    if cwd.is_empty()
        || model.is_empty()
        || ![
            "codex",
            "claude",
            "cursor",
            "opencode",
            "pi",
            "omp",
            "fx",
            "hermes",
            "antigravity",
            "devin",
            "grok",
        ]
        .contains(&harness)
    {
        return None;
    }
    let title = value["title"].as_str()?.trim();
    Some(
        json!({"sessionId":id,"cwd":cwd,"title":if title.is_empty(){"Agent session".into()}else{title.chars().take(300).collect::<String>()},"harness":harness,"model":model}),
    )
}

/// Conservative shell tokenization: a CLI mention in echo/output is not a call.
fn words(command: &str) -> Option<Vec<String>> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escape = false;
    for ch in command.chars() {
        if escape {
            word.push(ch);
            escape = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escape = true;
            continue;
        }
        if quote == Some(ch) {
            quote = None;
            continue;
        }
        if (ch == '\'' || ch == '"') && quote.is_none() {
            quote = Some(ch);
            continue;
        }
        if quote.is_none() && ch.is_whitespace() {
            if !word.is_empty() {
                result.push(std::mem::take(&mut word));
            }
            continue;
        }
        if quote != Some('\'') && matches!(ch, '$' | '`') {
            return None;
        }
        if quote.is_none()
            && matches!(
                ch,
                ';' | '&' | '|' | '<' | '>' | '(' | ')' | '#' | '\n' | '\r'
            )
        {
            return None;
        }
        word.push(ch);
    }
    if quote.is_some() || escape {
        return None;
    }
    if !word.is_empty() {
        result.push(word);
    }
    Some(result)
}
fn action(block: &Value) -> Option<String> {
    if block["role"] != "tool"
        || !matches!(
            block["tool"]["status"].as_str(),
            Some("completed" | "success")
        )
    {
        return None;
    }
    let meta = &block["tool"];
    let command = if meta["preview"]["kind"] == "shell" {
        meta["preview"]["title"].as_str()
    } else {
        None
    }
    .or(meta["title"].as_str())
    .or(block["text"].as_str())?;
    command_action(command)
}
/// Recognize the app CLI itself, the desktop app's `monocode` or the native
/// `monocode-gpui`, including the desktop shell wrapper.
pub fn command_action(command: &str) -> Option<String> {
    let mut parts = words(command.trim())?;
    if parts.len() == 3
        && parts[1].starts_with('-')
        && parts[1].contains('c')
        && parts[0]
            .rsplit('/')
            .next()
            .is_some_and(|s| ["sh", "bash", "zsh"].contains(&s))
    {
        parts = words(&parts[2])?;
    }
    let executable = parts
        .first()?
        .rsplit(['/', '\\'])
        .next()?
        .trim_end_matches(".exe");
    if !matches!(executable, "monocode" | "monocode-gpui") || parts.get(1)? != "app" {
        return None;
    }
    let mut flags = parts[3.min(parts.len())..].iter();
    while let Some(flag) = flags.next() {
        // The input object may also be passed as it is.
        if flag.starts_with('{') {
            continue;
        }
        if !["--json", "--input", "--request-id"].contains(&flag.as_str()) || flags.next().is_none()
        {
            return None;
        }
    }
    parts.get(2).cloned()
}

/// What a tool call did to a MonoCode session, for a Mono's launch chips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionCall {
    /// Started one: `session_start`, or the CLI's old `sessions.start`.
    Start,
    /// Sent one a message: `session_send`, or `sessions.send`.
    Send,
}

/// The session call a completed tool row made, with the sessions its
/// successful results name: a run of the app CLI ([`command_action`]),
/// whose output holds one receipt or one per line, or a call of the
/// `monocode` MCP server's tool, as its provider recorded it.
pub fn session_call(tool: &ToolCall) -> Option<(SessionCall, Vec<i64>)> {
    if tool.state != ToolState::Completed {
        return None;
    }
    let input = serde_json::from_str::<Value>(&tool.input).unwrap_or_default();
    if let Some(action) = input["command"].as_str().and_then(command_action) {
        let call = match action.as_str() {
            "sessions.start" | "session_start" => SessionCall::Start,
            "sessions.send" | "session_send" => SessionCall::Send,
            _ => return None,
        };
        return Some((call, receipt_sessions(&tool.output)));
    }
    let call = match tool.mcp_tool()? {
        (CONTROL_SERVER, "session_start") => SessionCall::Start,
        (CONTROL_SERVER, "session_send") => SessionCall::Send,
        _ => return None,
    };
    Some((
        call,
        mcp_session(call, &input, &tool.output)
            .into_iter()
            .collect(),
    ))
}

/// The sessions the successful CLI receipts in `output` name.
fn receipt_sessions(output: &str) -> Vec<i64> {
    let receipts = serde_json::from_str::<Value>(output)
        .map(|v| vec![v])
        .unwrap_or_else(|_| {
            output
                .lines()
                .filter_map(|line| serde_json::from_str(line.trim()).ok())
                .collect()
        });
    receipts
        .iter()
        .filter(|value| value["ok"] == true)
        .filter_map(|value| {
            value["result"]["sessionId"]
                .as_i64()
                .or_else(|| session_handle(&value["result"]["id"]))
                .or_else(|| {
                    (value["result"]["submitted"] == true)
                        .then(|| session_handle(&value["result"]["sessionId"]))
                        .flatten()
                })
        })
        .collect()
}

/// The session a successful MCP `call` with `input` names in its recorded
/// `output`: the structured result's `sessionId`, within the whole result
/// as Codex records it or alone as the result's text. A provider that keeps
/// only the relay's one line of text gets it from that line for a start
/// ("Started session 7 (…"), and from the input for a send.
fn mcp_session(call: SessionCall, input: &Value, output: &str) -> Option<i64> {
    let output = serde_json::from_str::<Value>(output).unwrap_or_else(|_| json!(output));
    if output["isError"] == true {
        return None;
    }
    let result = output.get("structuredContent").unwrap_or(&output);
    if let Some(id) = result["sessionId"].as_i64() {
        return Some(id);
    }
    match call {
        SessionCall::Start => {
            let rest = output
                .as_str()?
                .trim_start()
                .strip_prefix("Started session ")?;
            let end = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            rest[..end].parse().ok()
        }
        SessionCall::Send => input["sessionId"].as_i64(),
    }
}

/// One receipt list per source block. Listings supply details for accepted sends
/// but never create launch cards by themselves (monoSpawnedSessions.ts).
pub(crate) fn block_launches(blocks: &[Value]) -> Vec<Vec<Value>> {
    let mut result = Vec::new();
    let mut known = HashMap::new();
    let mut sent = Vec::new();
    for block in blocks {
        let mut launches: Vec<Value> = block["monoSpawnedSessions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(launch)
            .collect();
        let receipt: Option<Value> = block["tool"]["detail"]
            .as_str()
            .and_then(|text| serde_json::from_str(text).ok());
        if let Some(receipt) = receipt.filter(|r| r["ok"] == true) {
            let value = &receipt["result"];
            match action(block).as_deref() {
                Some("sessions.start")
                    if value["id"]
                        .as_str()
                        .is_some_and(|id| id.starts_with("app-")) =>
                {
                    let mut value = value.clone();
                    if value["title"].is_null() {
                        value["title"] = json!("Agent session");
                    }
                    if let Some(value) = launch(&value) {
                        launches.push(value);
                    }
                }
                Some("sessions.list") => {
                    for value in value["sessions"].as_array().into_iter().flatten() {
                        let mut value = value.clone();
                        value["cwd"] = receipt["result"]["cwd"].clone();
                        if let Some(value) = launch(&value) {
                            known.insert(value["sessionId"].as_str().unwrap().to_owned(), value);
                        }
                    }
                }
                Some("sessions.send") if value["submitted"] == true => {
                    if let Some(id) = value["sessionId"].as_str() {
                        sent.push((result.len(), id.to_owned()));
                    }
                }
                _ => {}
            }
        }
        for value in &launches {
            known.insert(
                value["sessionId"].as_str().unwrap().to_owned(),
                value.clone(),
            );
        }
        result.push(launches);
    }
    for (index, id) in sent {
        if let Some(value) = known.get(&id) {
            result[index].push(value.clone());
        }
    }
    result
}

impl super::Mono {
    /// Refresh references without discarding native-created documents or launch
    /// monitoring state. Removed desktop references disappear on the next read.
    pub fn refresh_work(&mut self, store: &crate::store::Store) -> Result<bool, String> {
        let Some(session) = self.session_id else {
            return Ok(false);
        };
        let Some((documents, launches)) = store.mono_work(session)? else {
            return Ok(false);
        };
        self.apply_work(documents, launches)
    }
    pub fn apply_work(
        &mut self,
        documents: Vec<super::Document>,
        mut launches: Vec<super::Launch>,
    ) -> Result<bool, String> {
        let before = serde_json::to_string(&(self.documents.clone(), self.launches.clone()))
            .map_err(|e| e.to_string())?;
        for launch in &mut launches {
            if let Some(previous) = self
                .launches
                .iter()
                .find(|old| old.session_id == launch.session_id)
            {
                launch.origin_turn = previous.origin_turn;
                launch.notify_on_complete = previous.notify_on_complete;
                launch.completed = previous.completed;
                launch.dismissed = previous.dismissed;
            }
        }
        self.documents.retain(|d| {
            !self.desktop_document_ids.contains(&d.id)
                && !documents.iter().any(|new| new.id == d.id)
        });
        self.launches.retain(|l| {
            !self.desktop_launch_ids.contains(&l.session_id)
                && !launches.iter().any(|new| new.session_id == l.session_id)
        });
        self.desktop_document_ids = documents.iter().map(|d| d.id.clone()).collect();
        self.desktop_launch_ids = launches.iter().map(|l| l.session_id).collect();
        self.documents.extend(documents);
        self.launches.extend(launches);
        Ok(before
            != serde_json::to_string(&(self.documents.clone(), self.launches.clone()))
                .map_err(|e| e.to_string())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn details(id: &str) -> Value {
        json!({"sessionId":id,"cwd":"/repo","title":"Review","harness":"codex","model":"gpt-6"})
    }
    fn receipt(command: &str, result: Value) -> Value {
        json!({"role":"tool","text":command,"tool":{"status":"completed","detail":json!({"ok":true,"result":result}).to_string()}})
    }
    #[test]
    fn desktop_receipts_recover_launches_and_accepted_followups_without_linking_listings() {
        let start = receipt("/bin/zsh -lc 'monocode app sessions.start'", {
            let mut v = details("app-child");
            v["id"] = json!("app-child");
            v
        });
        let listing = receipt(
            "monocode app sessions.list",
            json!({"cwd":"/repo","sessions":[{"id":"existing","title":"Continue","harness":"claude","model":"opus"}]}),
        );
        let send = receipt(
            "monocode app sessions.send",
            json!({"submitted":true,"sessionId":"existing"}),
        );
        let results = block_launches(&[listing.clone(), start.clone(), send.clone()]);
        assert!(results[0].is_empty());
        assert_eq!(results[1][0]["sessionId"], "app-child");
        assert_eq!(results[2][0]["sessionId"], "existing");
        let mut failed = start.clone();
        failed["tool"]["status"] = json!("error");
        let mut truncated = start.clone();
        truncated["tool"]["detail"] = json!("{\"ok\":true,");
        let echo = receipt(
            "echo monocode app sessions.start",
            json!({"id":"app-child"}),
        );
        assert!(block_launches(&[failed, truncated, echo, listing])
            .into_iter()
            .flatten()
            .next()
            .is_none());
        assert!(block_launches(&[send])[0].is_empty());
    }
    /// F-8: the native `monocode-gpui app` command is the app CLI too, by
    /// operation or old name, quoted, wrapped in a shell or given the input
    /// object as it is; a mention or a pipeline is not a call.
    #[test]
    fn the_native_command_is_recognized_like_the_desktop_one() {
        for (command, action) in [
            ("monocode app sessions.start --json {}", "sessions.start"),
            (
                "'/Applications/Mono Code.app/Contents/MacOS/monocode-gpui' app session_start --json '{}'",
                "session_start",
            ),
            (
                "/bin/zsh -lc \"monocode-gpui app sessions.send --json '{}' --request-id r1\"",
                "sessions.send",
            ),
            (r#"C:\\MonoCode\\monocode-gpui.exe app session_start {"prompt":"x"}"#, "session_start"),
        ] {
            assert_eq!(command_action(command).as_deref(), Some(action), "{command}");
        }
        for command in [
            "echo monocode-gpui app session_start",
            "monocode-gpui app session_start --json {} | jq .",
            "monocode-gpui app session_start --json",
            "monocode-gpui app session_start --verbose {}",
            "monocode-gpuix app session_start",
        ] {
            assert_eq!(command_action(command), None, "{command}");
        }
    }
    #[test]
    fn reconcile_work_retains_native_documents_monitoring_and_desktop_removals() {
        let mut mono = super::super::Mono::default();
        let document = super::super::Document {
            id: "desktop".into(),
            kind: "html".into(),
            title: "Visual".into(),
            summary: "Summary".into(),
            body: "<h1>Saved</h1>".into(),
            created_at: 1,
            updated_at: 2,
        };
        let mut native = document.clone();
        native.id = "native".into();
        mono.documents.push(native);
        mono.apply_work(vec![document], Vec::new()).unwrap();
        assert_eq!(mono.documents.len(), 2);
        mono.apply_work(Vec::new(), Vec::new()).unwrap();
        assert_eq!(mono.documents.len(), 1);
        assert_eq!(mono.documents[0].id, "native");
    }
}
