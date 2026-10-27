//! T-M18, F-8: a Mono's turn shows a chip for every session its tool calls
//! started, whichever way the call went: the `monocode` MCP server, as each
//! provider records its tools, the native `monocode-gpui app` command by
//! operation or old name, or the desktop app's `monocode app` command. A
//! message sent to a session gets a chip only when the Mono launched it.
use super::*;
use gpui::TestAppContext;
use serde_json::{Value, json};

fn row(role: &str, text: &str) -> Row {
    Row {
        role: role.into(),
        text: text.into(),
        first: true,
        sent_at: None,
    }
}

/// A completed tool row as the transcript stores it.
fn tool(name: &str, input: &Value, output: &str, display: Option<Value>) -> Row {
    let call = ToolCall {
        id: "t".into(),
        name: name.into(),
        input: input.to_string(),
        output: output.into(),
        state: ToolState::Completed,
        panel: None,
        display,
    };
    row("tool", &serde_json::to_string(&call).unwrap())
}

/// `session_start`'s typed output for session `id`.
fn started(id: i64) -> Value {
    json!({
        "sessionId": id,
        "project": "/repo",
        "harness": "codex",
        "model": "gpt-5",
        "access": "auto",
        "submitted": true,
        "draft": false,
    })
}

/// The relay's text for `started(id)`, the one line an MCP result carries
/// beside its structured content.
fn summary(id: i64) -> String {
    monocode_control::summarize("session_start", &started(id)).unwrap()
}

/// The sessions whose chips one Mono turn of `work` shows, given the
/// sessions the Mono launched (`launched`).
fn chips(work: Vec<Row>, launched: &[i64], cx: &mut TestAppContext) -> Vec<i64> {
    let view = cx.new(|_| {
        let mut view = TranscriptView::new();
        view.mono_mode("cat", 0xe86b5f, "MonoCat");
        view
    });
    view.update(cx, |view, cx| {
        let launches = launched
            .iter()
            .map(|id| {
                let card = LaunchView {
                    title: "Review".into(),
                    state: "working",
                    harness: "codex".into(),
                    cwd: "/repo".into(),
                };
                (*id, card)
            })
            .collect();
        view.set_launches(launches, cx);
        let mut rows = vec![row("user", "Start a review")];
        rows.extend(work);
        rows.push(row("assistant", "Started it."));
        view.patch(0, rows, cx);
        view.rows
            .iter()
            .filter_map(|r| match r.body {
                Body::Launch(id) => Some(id),
                _ => None,
            })
            .collect()
    })
}

#[gpui::test]
fn mcp_session_start_rows_get_chips(cx: &mut TestAppContext) {
    let input = json!({"prompt": "Review the API", "title": "Review"});
    let work = vec![
        // Claude: the structured content as the result's text.
        tool(
            "mcp__monocode__session_start",
            &input,
            &started(7).to_string(),
            None,
        ),
        // Claude: only the relay's one-line text.
        tool("mcp__monocode__session_start", &input, &summary(8), None),
        // Codex: the whole MCP result.
        tool(
            "monocode:session_start",
            &input,
            &serde_json::to_string_pretty(&json!({
                "content": [{"type": "text", "text": summary(9)}],
                "structuredContent": started(9),
            }))
            .unwrap(),
            None,
        ),
        // ACP: the title names the tool; the content is the text.
        tool(
            "monocode/session_start",
            &input,
            &summary(10),
            Some(json!({"title": "monocode/session_start", "kind": "other", "cwd": "/repo"})),
        ),
    ];
    assert_eq!(chips(work, &[], cx), vec![7, 8, 9, 10]);
}

#[gpui::test]
fn native_cli_rows_get_chips_by_operation_and_old_name(cx: &mut TestAppContext) {
    let receipt =
        |id: i64| json!({"ok": true, "result": started(id), "requestId": "r1"}).to_string();
    let work = vec![
        tool(
            "Bash",
            &json!({"command": r#"'/Applications/Mono Code.app/Contents/MacOS/monocode-gpui' app session_start --json '{"prompt":"Review"}'"#}),
            &receipt(11),
            None,
        ),
        tool(
            "exec_command",
            &json!({"command": r#"/bin/zsh -lc "monocode-gpui app sessions.start --json '{\"prompt\":\"Review\"}' --request-id r1""#}),
            &receipt(12),
            None,
        ),
        tool(
            "Bash",
            &json!({"command": r#"monocode-gpui.exe app session_start '{"prompt":"Review"}'"#}),
            &receipt(13),
            None,
        ),
    ];
    assert_eq!(chips(work, &[], cx), vec![11, 12, 13]);
}

#[gpui::test]
fn desktop_cli_rows_still_get_chips(cx: &mut TestAppContext) {
    let work = vec![tool(
        "Bash",
        &json!({"command": "monocode app sessions.start --json {}"}),
        r#"{"ok":true,"result":{"sessionId":14,"accepted":true}}"#,
        None,
    )];
    assert_eq!(chips(work, &[], cx), vec![14]);
}

#[gpui::test]
fn sends_get_chips_only_for_launched_sessions_and_failures_get_none(cx: &mut TestAppContext) {
    let sent = |id: i64| json!({"sessionId": id, "delivery": "queued", "queueLength": 1});
    let work = vec![
        tool(
            "mcp__monocode__session_send",
            &json!({"sessionId": 15, "prompt": "Also check the tests"}),
            &sent(15).to_string(),
            None,
        ),
        tool(
            "mcp__monocode__session_send",
            &json!({"sessionId": 16, "prompt": "Not one of mine"}),
            &sent(16).to_string(),
            None,
        ),
        tool(
            "Bash",
            &json!({"command": "monocode-gpui app sessions.send --json '{\"sessionId\":17,\"prompt\":\"x\"}'"}),
            &json!({"ok": true, "result": sent(17)}).to_string(),
            None,
        ),
        // A refused call, another server's tool of the same name, and a
        // command that only mentions the CLI start nothing.
        tool(
            "monocode:session_start",
            &json!({"prompt": "x"}),
            &json!({"content": [{"type": "text", "text": "Session 18 is busy"}], "structuredContent": {"code": "session_busy", "sessionId": 18}, "isError": true}).to_string(),
            None,
        ),
        tool(
            "mcp__other__session_start",
            &json!({"prompt": "x"}),
            &started(19).to_string(),
            None,
        ),
        tool(
            "Bash",
            &json!({"command": "echo monocode-gpui app session_start"}),
            &json!({"ok": true, "result": started(20)}).to_string(),
            None,
        ),
    ];
    assert_eq!(chips(work, &[15, 17], cx), vec![15, 17]);
}
