//! The app's MCP server over real Streamable HTTP on an ephemeral
//! 127.0.0.1 port: a real `rmcp` client on each protocol version
//! initializes, lists and calls tools and follows a resource, with a fake
//! backend in place of the app; raw requests check the `Host` and `Origin`
//! guard and the `User-Agent` fallback name; a stopped server refuses
//! connections.
use monocode_control::{
    ClientInfo, Command, ControlError, Notice, NoticeSink, Topic,
    frame::Event,
    ops::project::{ProjectListOutput, ProjectLocation, ProjectSummary},
};
use monocode_mcp::{Backend, Connect, http::HttpServer};
use rmcp::{
    ClientHandler, ClientLifecycleMode,
    model::{
        CallToolRequestParams, ClientCapabilities, ClientConfig, ClientRequest, Implementation,
        ProtocolVersion, Request, ResourceUpdatedNotificationParam, ServerNotification,
        SubscribeRequestParams, SubscriptionFilter,
    },
    service::{ClientServiceExt, NotificationContext, RoleClient, RunningService},
    transport::StreamableHttpClientTransport,
};
use serde_json::Value;
use std::{
    fmt::Write as _,
    io::{Read, Write},
    net::TcpStream,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{
    Notify,
    mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    /// The fake app behind every session: who called what, the sinks of
    /// the sessions that follow something, and how many backends live.
    #[derive(Clone, Default)]
    pub(super) struct App {
        pub(super) seen: Arc<Mutex<Vec<(String, String)>>>,
        sinks: Arc<Mutex<Vec<NoticeSink>>>,
        live: Arc<AtomicUsize>,
        ended: Arc<Notify>,
    }

    struct Fake {
        app: App,
        sink: NoticeSink,
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.app.live.fetch_sub(1, Ordering::SeqCst);
            self.app.ended.notify_one();
        }
    }

    impl Backend for Fake {
        fn call(
            &self,
            client: &ClientInfo,
            _request_id: Option<String>,
            command: Command,
        ) -> Result<Value, ControlError> {
            let name = command.name().to_owned();
            self.app
                .seen
                .lock()
                .unwrap()
                .push((client.name.clone(), name.clone()));
            match name.as_str() {
                "project_list" => Ok(serde_json::to_value(projects()).unwrap()),
                _ => Err(ControlError::invalid_input(format!("no fake {name}"))),
            }
        }
        fn subscribe(&self, client: &ClientInfo, _topics: &[Topic]) -> Result<(), ControlError> {
            self.app
                .seen
                .lock()
                .unwrap()
                .push((client.name.clone(), "subscribe".into()));
            self.app.sinks.lock().unwrap().push(Arc::clone(&self.sink));
            Ok(())
        }
        fn unsubscribe(&self, _topics: &[Topic]) -> Result<(), ControlError> {
            Ok(())
        }
        fn resubscribe(&self) -> Result<(), ControlError> {
            Ok(())
        }
    }

    impl App {
        pub(super) fn connect(&self) -> Connect {
            let app = self.clone();
            Arc::new(move |sink| {
                app.live.fetch_add(1, Ordering::SeqCst);
                Box::new(Fake {
                    app: app.clone(),
                    sink,
                })
            })
        }
        /// Waits until every session's backend is gone.
        pub(super) async fn all_ended(&self) {
            while self.live.load(Ordering::SeqCst) > 0 {
                let ended = tokio::time::timeout(Duration::from_secs(20), self.ended.notified());
                assert!(ended.await.is_ok(), "a backend outlived its session");
            }
        }
        /// The projects changed: every follower hears it.
        pub(super) fn change(&self) {
            for sink in self.sinks.lock().unwrap().iter() {
                sink(Notice::Changed(Event {
                    topic: Topic::Projects,
                    revision: 1,
                }));
            }
        }
        pub(super) fn callers(&self, operation: &str) -> Vec<String> {
            let seen = self.seen.lock().unwrap();
            seen.iter()
                .filter(|(_, op)| op == operation)
                .map(|(client, _)| client.clone())
                .collect()
        }
    }

    pub(super) fn projects() -> ProjectListOutput {
        ProjectListOutput {
            projects: vec![ProjectSummary {
                path: "/work/monocode".into(),
                key: "/work/monocode".into(),
                name: "monocode".into(),
                color: "#459cf7".into(),
                current: true,
                archived: false,
                location: ProjectLocation::Local,
            }],
            total: 1,
            next_cursor: None,
        }
    }

    /// A client that records `notifications/resources/updated`.
    pub(super) struct Recorder {
        config: ClientConfig,
        updates: UnboundedSender<String>,
    }

    impl ClientHandler for Recorder {
        fn get_info(&self) -> ClientConfig {
            self.config.clone()
        }
        async fn on_resource_updated(
            &self,
            params: ResourceUpdatedNotificationParam,
            _context: NotificationContext<RoleClient>,
        ) {
            let _ = self.updates.send(params.uri);
        }
    }

    pub(super) type Session = RunningService<RoleClient, Recorder>;

    /// A client called `name` on `version` over HTTP to `url`.
    pub(super) async fn connect(
        url: String,
        version: ProtocolVersion,
        name: &str,
    ) -> (Session, UnboundedReceiver<String>) {
        let (updates, heard) = unbounded_channel();
        let config = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new(name, "1"),
        )
        .with_protocol_version(version.clone());
        let lifecycle = if version.has_initialize() {
            ClientLifecycleMode::Initialize
        } else {
            ClientLifecycleMode::Discover {
                preferred_versions: vec![version],
            }
        };
        let session = Recorder { config, updates }
            .serve_with_lifecycle(StreamableHttpClientTransport::from_uri(url), lifecycle)
            .await
            .unwrap();
        (session, heard)
    }

    /// One raw HTTP/1.1 request to the server on `port`; the status and the
    /// whole response.
    pub(super) fn raw(
        port: u16,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut request = format!("POST {path} HTTP/1.1\r\n");
        for (name, value) in headers {
            let _ = write!(request, "{name}: {value}\r\n");
        }
        let _ = write!(
            request,
            "Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        let status = response
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        (status, response)
    }

    pub(super) const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"raw","version":"1"}}}"#;
}
use support::{App, INITIALIZE, connect, projects, raw};

#[tokio::test]
async fn both_protocol_versions_list_call_and_follow_over_http() {
    let app = App::default();
    let server = HttpServer::start(0, app.connect()).unwrap();
    let url = server.url();
    assert!(
        url.starts_with("http://127.0.0.1:") && url.ends_with("/mcp"),
        "{url}"
    );
    for (version, name) in [
        (ProtocolVersion::V_2025_11_25, "Sessions client"),
        (ProtocolVersion::V_2026_07_28, "Stateless client"),
    ] {
        let (session, mut updates) = connect(url.clone(), version.clone(), name).await;
        let tools = session.peer().list_all_tools().await.unwrap();
        assert!(tools.iter().any(|t| t.name == "project_list"));
        assert!(
            !tools.iter().any(|t| t.name == "app_launch"),
            "the app serving HTTP is running: nothing to launch"
        );
        let result = session
            .peer()
            .call_tool(CallToolRequestParams::new("project_list"))
            .await
            .unwrap();
        let expected = serde_json::to_value(projects()).unwrap();
        assert_eq!(result.structured_content, Some(expected), "{version:?}");
        assert!(app.callers("project_list").contains(&name.to_owned()));

        let uri = "monocode://projects";
        if version.has_initialize() {
            let subscribe =
                ClientRequest::SubscribeRequest(Request::new(SubscribeRequestParams::new(uri)));
            session.peer().send_request(subscribe).await.unwrap();
            app.change();
            let heard = tokio::time::timeout(Duration::from_secs(20), updates.recv()).await;
            assert_eq!(heard.unwrap().as_deref(), Some(uri));
        } else {
            let filter = SubscriptionFilter::builder()
                .resource_subscriptions([uri])
                .build();
            let mut listening = session.peer().listen(filter).await.unwrap();
            app.change();
            let heard = tokio::time::timeout(Duration::from_secs(20), listening.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let ServerNotification::ResourceUpdatedNotification(notice) = heard else {
                unreachable!("an update: {heard:?}")
            };
            assert_eq!(notice.params.uri, uri);
        }
        assert!(app.callers("subscribe").contains(&name.to_owned()));
        session.cancel().await.unwrap();
        // The session's backend, and with it what it followed, goes too.
        app.all_ended().await;
    }
}

#[tokio::test]
async fn requests_from_another_host_or_origin_are_refused() {
    let server = HttpServer::start(0, App::default().connect()).unwrap();
    let port = server.port();
    let own = format!("127.0.0.1:{port}");
    let local = format!("localhost:{port}");
    let local_origin = format!("http://localhost:{port}");
    let other_port = format!("127.0.0.1:{}", port.wrapping_add(1));
    let refused: [&[(&str, &str)]; 5] = [
        &[("Host", "evil.example")],
        &[("Host", "127.0.0.1")],
        &[("Host", &other_port)],
        &[("Host", &own), ("Origin", "http://evil.example")],
        &[("Host", &own), ("Origin", "null")],
    ];
    for headers in refused {
        let (status, response) = raw(port, "/mcp", headers, INITIALIZE);
        assert_eq!(status, 403, "{headers:?}: {response}");
    }
    let allowed: [&[(&str, &str)]; 3] = [
        &[("Host", &own)],
        &[("Host", &local)],
        &[("Host", &local), ("Origin", &local_origin)],
    ];
    for headers in allowed {
        let (status, response) = raw(port, "/mcp", headers, INITIALIZE);
        assert_eq!(status, 200, "{headers:?}: {response}");
        assert!(response.contains("\"serverInfo\""), "{response}");
    }
    let (status, _) = raw(port, "/other", &[("Host", &own)], INITIALIZE);
    assert_eq!(status, 404, "MCP lives at /mcp only");
}

#[tokio::test]
async fn a_request_without_a_client_name_is_named_after_its_user_agent() {
    let app = App::default();
    let server = HttpServer::start(0, app.connect()).unwrap();
    let port = server.port();
    let own = format!("127.0.0.1:{port}");
    let call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"project_list","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#;
    let headers = [
        ("Host", own.as_str()),
        ("User-Agent", "curl/8.7.1"),
        ("MCP-Protocol-Version", "2026-07-28"),
        ("Mcp-Method", "tools/call"),
        ("Mcp-Name", "project_list"),
    ];
    let (status, response) = raw(port, "/mcp", &headers, call);
    assert_eq!(status, 200, "{response}");
    assert!(response.contains("/work/monocode"), "{response}");
    assert_eq!(app.callers("project_list"), ["curl"]);
}

#[tokio::test]
async fn a_stopped_server_refuses_connections_and_frees_its_port() {
    let server = HttpServer::start(0, App::default().connect()).unwrap();
    let port = server.port();
    assert!(TcpStream::connect(("127.0.0.1", port)).is_ok());
    drop(server);
    let refused = TcpStream::connect(("127.0.0.1", port)).unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::ConnectionRefused);
    let again = HttpServer::start(port, App::default().connect()).unwrap();
    assert_eq!(again.port(), port, "the port is free again");
    let taken = HttpServer::start(port, App::default().connect())
        .err()
        .unwrap();
    assert_eq!(taken.kind(), std::io::ErrorKind::AddrInUse);
}
