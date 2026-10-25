//! Helpers the end-to-end tests share: a real `rmcp` client over an
//! in-memory duplex to the relay, and fake apps on real local sockets under
//! the checkout's `target/c/` (named pipes on Windows). Each test file
//! includes this as `#[cfg(test)] pub mod common;`, so test-only lints
//! apply and the helpers a file leaves unused are not dead code.
use monocode_control::{
    Endpoint, Grant, PROTOCOL_VERSION, Scope,
    frame::{ServerFrame, Welcome},
    socket_path,
    transport::{Connection, Listener},
};
use monocode_mcp::{Options, serve};
use rmcp::{
    ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation,
        ProtocolVersion,
    },
    service::{RoleClient, RunningService},
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// An MCP client session with the relay.
pub type Session = RunningService<RoleClient, ClientConfig>;

/// An empty folder `name` under the checkout's `target/c/`, short enough
/// that `<folder>/control/monocode-control.sock` fits `sun_path`.
///
/// # Panics
/// When the folder cannot be made, or its socket path would not fit.
#[must_use]
pub fn scratch(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("target/c");
    std::fs::create_dir_all(&root).unwrap();
    let dir = root.canonicalize().unwrap().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        cfg!(windows) || socket_path(&dir).is_ok(),
        "{dir:?} fits sun_path"
    );
    dir
}

/// A fake app on `endpoint`: each connection is served by `serve` on a
/// thread of its own.
///
/// # Panics
/// When the endpoint cannot be bound.
pub fn fake_app(endpoint: &Endpoint, serve: impl Fn(&Connection) + Clone + Send + 'static) {
    let listener = Listener::bind(endpoint).unwrap();
    std::thread::spawn(move || {
        while let Ok(connection) = listener.accept() {
            let serve = serve.clone();
            std::thread::spawn(move || serve(&connection));
        }
    });
}

/// The fake app's welcome: this protocol, app version 0.16.0, `scopes`,
/// no pushed changes.
#[must_use]
pub fn welcome(scopes: Vec<Scope>) -> ServerFrame {
    ServerFrame::Welcome(Welcome {
        version: PROTOCOL_VERSION,
        app_version: "0.16.0".into(),
        grant: Grant { scopes },
        push: false,
    })
}

/// A client called `name` on `version`, talking to a relay with `options`.
///
/// # Panics
/// When the client cannot start its session.
pub async fn connect(options: Options, version: ProtocolVersion, name: &str) -> Session {
    let (server_io, client_io) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move { serve(options, server_io).await });
    ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new(name, "9.9"),
    )
    .with_protocol_version(version)
    .serve(client_io)
    .await
    .unwrap()
}

/// Calls tool `name` with `arguments`, an object.
///
/// # Panics
/// When `arguments` is not an object or the call cannot be made.
pub async fn call(session: &Session, name: &str, arguments: Value) -> CallToolResult {
    let Value::Object(arguments) = arguments else {
        unreachable!("arguments are an object")
    };
    session
        .peer()
        .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments))
        .await
        .unwrap()
}

/// The text content of `result`.
#[must_use]
pub fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect()
}
