//! The app's MCP server over HTTP: it listens on 127.0.0.1 only while it is
//! on, and a real MCP client's calls, on either protocol version, go through
//! the person's approval and the control socket's pipeline.
use super::tests::setup;
use super::*;
use gpui::{TestAppContext, WindowHandle};
use rmcp::{
    ClientLifecycleMode,
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation,
        ProtocolVersion,
    },
    service::ClientServiceExt as _,
    transport::StreamableHttpClientTransport,
};
use std::{net::TcpStream, thread::JoinHandle};

/// Turns the MCP server on or off; its URL while it serves.
fn turn(window: WindowHandle<Shell>, on: bool, cx: &mut TestAppContext) -> Option<String> {
    window
        .update(cx, |shell, _, cx| {
            shell.edit_control(&ExternalControlEdit::Enabled(on), cx);
            shell.control_status().url
        })
        .unwrap()
}

/// A real MCP client called `name` on `version`, on a thread of its own:
/// it connects to `url` and calls `tool` once.
fn client(
    url: &str,
    name: &'static str,
    version: ProtocolVersion,
    tool: &'static str,
) -> JoinHandle<Result<CallToolResult, String>> {
    let url = url.to_owned();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let lifecycle = if version.has_initialize() {
                ClientLifecycleMode::Initialize
            } else {
                ClientLifecycleMode::Discover {
                    preferred_versions: vec![version.clone()],
                }
            };
            let session = ClientConfig::new(
                ClientCapabilities::default(),
                Implementation::new(name, "1"),
            )
            .with_protocol_version(version)
            .serve_with_lifecycle(StreamableHttpClientTransport::from_uri(url), lifecycle)
            .await
            .map_err(|e| e.to_string())?;
            let result = session
                .peer()
                .call_tool(CallToolRequestParams::new(tool))
                .await
                .map_err(|e| e.to_string());
            let _ = session.cancel().await;
            result
        })
    })
}

/// Answers the next hello or call the HTTP server hands the shell.
fn answer(window: WindowHandle<Shell>, cx: &mut TestAppContext) {
    let received = window
        .update(cx, |shell, _, _| shell.control.received.clone())
        .unwrap();
    let (forward, forwarded) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = forward.send(received.recv_blocking());
    });
    let request = forwarded
        .recv_timeout(Duration::from_secs(60))
        .expect("the MCP server asks the shell")
        .unwrap();
    window
        .update(cx, |shell, window, cx| {
            shell.handle_control(request, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
}

fn code(result: &CallToolResult) -> Option<String> {
    let content = result.structured_content.as_ref()?;
    content["code"].as_str().map(str::to_owned)
}

#[gpui::test]
fn the_mcp_server_listens_on_localhost_only_while_it_is_on(cx: &mut TestAppContext) {
    let window = setup(cx);
    let off = window
        .update(cx, |shell, _, _| shell.control_status().url)
        .unwrap();
    assert_eq!(off, None, "off by default: nothing listens");
    let url = turn(window, true, cx).unwrap();
    let port = url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.strip_suffix("/mcp"))
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap();
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_ok(),
        "on: it listens"
    );
    assert_eq!(turn(window, false, cx), None);
    let refused = TcpStream::connect(("127.0.0.1", port)).unwrap_err();
    assert_eq!(
        refused.kind(),
        std::io::ErrorKind::ConnectionRefused,
        "off: no listener at all"
    );
}

#[gpui::test]
fn a_new_http_client_is_asked_once_then_its_calls_run(cx: &mut TestAppContext) {
    let window = setup(cx);
    let url = turn(window, true, cx).unwrap();
    let called = client(
        &url,
        "HTTP Tester",
        ProtocolVersion::V_2025_11_25,
        "project_list",
    );
    answer(window, cx);
    let prompt = cx.pending_prompt().expect("the person is asked").0;
    assert!(
        prompt.contains("Allow “HTTP Tester” to control MonoCode?"),
        "{prompt}"
    );
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    answer(window, cx);
    let result = called.join().unwrap().unwrap();
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let output = result.structured_content.unwrap();
    assert!(output["projects"].is_array(), "{output}");
    let approved = window
        .update(cx, |shell, _, _| {
            shell.control.policy.client("HTTP Tester").cloned()
        })
        .unwrap();
    assert_eq!(approved.unwrap().scopes, Scope::DEFAULT_GRANT);
    let activity = window
        .update(cx, |shell, _, _| shell.control_status().activity)
        .unwrap();
    assert!(
        activity
            .iter()
            .any(|a| a.client == "HTTP Tester" && a.operation == "approve"),
        "the approval is in the activity log (reads are not): {activity:?}"
    );
}

#[gpui::test]
fn a_denied_http_client_is_refused_and_not_asked_again(cx: &mut TestAppContext) {
    let window = setup(cx);
    let url = turn(window, true, cx).unwrap();
    let called = client(&url, "Unwelcome", ProtocolVersion::V_2026_07_28, "app_info");
    answer(window, cx);
    assert!(cx.has_pending_prompt(), "a stateless client is asked too");
    cx.simulate_prompt_answer("Deny");
    cx.run_until_parked();
    let result = called.join().unwrap().unwrap();
    assert_eq!(code(&result).as_deref(), Some("user_declined"));
    let again = client(&url, "Unwelcome", ProtocolVersion::V_2026_07_28, "app_info");
    answer(window, cx);
    assert!(
        !cx.has_pending_prompt(),
        "a denied client is not asked again"
    );
    let result = again.join().unwrap().unwrap();
    assert_eq!(code(&result).as_deref(), Some("user_declined"));
    let denied = window
        .update(cx, |shell, _, _| shell.control_status().denied)
        .unwrap();
    assert_eq!(denied, ["Unwelcome"]);
}
