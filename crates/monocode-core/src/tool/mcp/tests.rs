use crate::tool::{ToolCall, ToolState};
use serde_json::{json, Value};

fn tool(name: &str, input: &Value) -> ToolCall {
    let mut call = ToolCall::new("t", name);
    call.input = serde_json::to_string_pretty(input).unwrap();
    call.state = ToolState::Completed;
    call
}

/// What the transcript row shows: action, target and source.
fn shown(call: &ToolCall) -> (String, String, Option<String>) {
    let shown = call.presentation();
    let source = shown.source().map(str::to_owned);
    (shown.action, shown.target, source)
}

fn caption(action: &str, target: &str, source: Option<&str>) -> (String, String, Option<String>) {
    (action.into(), target.into(), source.map(Into::into))
}

/// The tool call `provider/acp.rs` records for a `session/update`
/// `tool_call`: the title as the name and as display metadata, the raw
/// input unchanged.
fn acp_call(title: &str, kind: &str, raw: &Value) -> ToolCall {
    let mut call = tool(title, raw);
    call.display = Some(json!({"title": title, "kind": kind, "cwd": "/repo"}));
    call
}

#[test]
fn claude_names_monocode_tools_by_title_and_subject() {
    let input = json!({"project": "/Users/x/github.com/repost-sh/monorepo"});
    let call = tool("mcp__monocode__project_delete", &input);
    assert_eq!(
        shown(&call),
        caption("Remove project", "repost-sh/monorepo", None)
    );
    assert_eq!(
        call.summary(),
        ("Remove project".into(), "repost-sh/monorepo".into())
    );
    // Display only: the recorded call is untouched.
    assert_eq!(call.name, "mcp__monocode__project_delete");
    assert_eq!(serde_json::from_str::<Value>(&call.input).unwrap(), input);
    let launch = call_named("mcp__monocode__app_launch");
    assert_eq!(shown(&launch), caption("Launch MonoCode", "", None));
}

fn call_named(name: &str) -> ToolCall {
    tool(name, &json!({}))
}

#[test]
fn codex_server_colon_tool_names_read_the_same() {
    let call = tool("monocode:project_add", &json!({"path": "/work/acme/app"}));
    assert_eq!(shown(&call), caption("Add project", "acme/app", None));
    let visual = tool("monocode_visual:visual_check", &json!({"source": "x"}));
    assert_eq!(shown(&visual), caption("Check visual", "", None));
}

#[test]
fn acp_titles_name_the_server_and_tool() {
    let raw = json!({"sessionId": 7, "timeoutMs": 5000});
    for title in [
        "monocode/session_wait",
        "Tool: monocode/session_wait",
        "mcp__monocode__session_wait",
    ] {
        let call = acp_call(title, "other", &raw);
        assert_eq!(
            shown(&call),
            caption("Wait for session", "#7", None),
            "{title}"
        );
    }
    let linear = acp_call(
        "linear/create_issue",
        "other",
        &json!({"title": "Fix login"}),
    );
    assert_eq!(
        shown(&linear),
        caption("Create issue", "Fix login", Some("Linear"))
    );
    // A semantic kind keeps the built-in caption, even for a path-like title.
    let read = acp_call("src/lib", "read", &json!({"path": "src/lib.rs"}));
    assert_eq!(read.presentation().action, "Read");
    assert_eq!(read.presentation().source(), None);
    assert_eq!(read.builtin_summary().unwrap().0, "Read");
}

#[test]
fn the_visual_server_has_curated_titles() {
    let show = tool(
        "mcp__monocode_visual__visual_show",
        &json!({"source": "return () => visual.text('Hi');", "title": "Build times"}),
    );
    assert_eq!(shown(&show), caption("Show visual", "Build times", None));
    let docs = call_named("mcp__monocode_visual__visual_docs");
    assert_eq!(shown(&docs), caption("Read visual docs", "", None));
}

#[test]
fn other_servers_read_as_words_with_the_server_as_source() {
    let issue = tool(
        "mcp__linear__create_issue",
        &json!({"title": "Fix login\nmore", "team": "ENG"}),
    );
    assert_eq!(
        shown(&issue),
        caption("Create issue", "Fix login", Some("Linear"))
    );
    let camel = tool("mcp__linear__createIssue", &json!({}));
    assert_eq!(shown(&camel), caption("Create issue", "", Some("Linear")));
    let gmail = tool(
        "mcp__claude_ai_Gmail__search_threads",
        &json!({"query": "from:ci failed"}),
    );
    assert_eq!(
        shown(&gmail),
        caption("Search threads", "from:ci failed", Some("Gmail"))
    );
    let browser = tool(
        "mcp__integrated-browser-mcp__browser_navigate",
        &json!({"url": "http://localhost:3000"}),
    );
    assert_eq!(
        shown(&browser),
        caption(
            "Browser navigate",
            "http://localhost:3000",
            Some("Integrated Browser")
        )
    );
    assert_eq!(
        issue.presentation().label(),
        "Create issue Fix login · Linear"
    );
    // A server's "Search" is never the built-in project search.
    let search = tool("mcp__linear__search", &json!({"query": "login"}));
    assert_eq!(shown(&search), caption("Search", "login", Some("Linear")));
    assert_eq!(search.presentation().builtin_action(), None);
    assert_eq!(search.builtin_summary(), None);
    assert_eq!(search.summary(), ("Search".into(), "login".into()));
}

#[test]
fn tool_search_loads_tools_by_their_titles() {
    let select = tool(
        "ToolSearch",
        &json!({"query": "select:mcp__monocode__project_delete,mcp__monocode__project_archive", "max_results": 2}),
    );
    assert_eq!(
        shown(&select),
        caption("Load tools", "Remove project, Archive project", None)
    );
    let mixed = tool(
        "ToolSearch",
        &json!({"query": "select:WebFetch,mcp__linear__create_issue,mcp__monocode__session_wait,Monitor"}),
    );
    assert_eq!(
        shown(&mixed),
        caption(
            "Load tools",
            "WebFetch, Create issue, Wait for session +1",
            None
        )
    );
    let keyword = tool("ToolSearch", &json!({"query": "+slack send"}));
    assert_eq!(shown(&keyword), caption("Find tools", "+slack send", None));
}

#[test]
fn built_in_tools_keep_their_captions() {
    let read = tool("Read", &json!({"file_path": "src/lib.rs"}));
    assert_eq!(shown(&read), caption("Read", "src/lib.rs", None));
    let bash = tool("Bash", &json!({"command": "cargo test"}));
    assert_eq!(shown(&bash), caption("Run", "cargo test", None));
    let unknown = tool("WebFetch", &json!({"url": "https://example.com"}));
    assert_eq!(unknown.presentation().action, "WebFetch");
    // Lookalikes that are not MCP names stay as they are.
    for name in ["mcp__", "mcp__monocode", "a:b:c", "src/lib.rs"] {
        assert_eq!(call_named(name).presentation().action, name);
    }
}

#[test]
fn claude_approvals_name_mcp_tools_by_title() {
    assert_eq!(
        super::title("mcp__monocode__project_delete"),
        "Remove project"
    );
    assert_eq!(
        super::title("mcp__linear__create_issue"),
        "Create issue · Linear"
    );
    assert_eq!(super::title("Write"), "Write");
}
