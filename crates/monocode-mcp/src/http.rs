//! The app's MCP server over Streamable HTTP, the way desktop apps serve
//! one: `http://127.0.0.1:<port>/mcp`, on the loopback address only. It runs
//! the stdio relay's handler; each MCP session (2025-11-25) and each
//! stateless request (2026-07-28) gets a backend of its own from the app.
//! As the MCP spec asks against DNS rebinding, a request is refused unless
//! its `Host` is this server's own loopback authority and its `Origin`, when
//! it sends one, is too. Dropping the server stops it and frees the port.
use crate::{backend::Connect, log, server::Relay};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::{
    Request, Response, StatusCode, body::Incoming, server::conn::http1, service::service_fn,
};
use hyper_util::rt::TokioIo;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::{
    convert::Infallible,
    net::{Ipv4Addr, SocketAddr, TcpListener},
    sync::Arc,
    thread::JoinHandle,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// Where the server answers MCP.
pub const PATH: &str = "/mcp";
/// The port the installed app's data serves on unless the person picks
/// another. IANA has not assigned it, and it spells "MCMC" on a phone keypad.
pub const RELEASE_PORT: u16 = 6262;
/// The port development data serves on, so a development build runs beside
/// the installed app. Also unassigned.
pub const DEV_PORT: u16 = 6263;

type Service = StreamableHttpService<Relay, LocalSessionManager>;

/// The server's URL on `port`.
#[must_use]
pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}{PATH}")
}

/// A running server. Dropping it stops it and frees the port.
pub struct HttpServer {
    port: u16,
    stop: CancellationToken,
    thread: Option<JoinHandle<()>>,
}

impl HttpServer {
    /// Binds `127.0.0.1:port` (`0` takes any free port) and serves MCP
    /// there on a thread of its own; each session's calls go to the backend
    /// `connect` makes.
    ///
    /// # Errors
    /// When the port cannot be bound (another app uses it) or the server's
    /// runtime or thread cannot start.
    pub fn start(port: u16, connect: Connect) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let stop = CancellationToken::new();
        let serving = stop.clone();
        let thread = std::thread::Builder::new()
            .name("mcp-http".into())
            .spawn(move || {
                runtime.block_on(serve(listener, port, connect, serving));
                // Calls still waiting on the person finish on their own.
                runtime.shutdown_background();
            })?;
        Ok(Self {
            port,
            stop,
            thread: Some(thread),
        })
    }

    /// The port it listens on.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Its URL.
    #[must_use]
    pub fn url(&self) -> String {
        url(self.port)
    }
}

impl Drop for HttpServer {
    /// Ends every session and waits until the port is free again; the
    /// accept loop stops at once.
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The rmcp service: loopback `Host` and `Origin` only, sessions for the
/// 2025-11-25 lifecycle and stateless requests from 2026-07-28 on.
fn config(port: u16, stop: &CancellationToken) -> StreamableHttpServerConfig {
    let authorities = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    StreamableHttpServerConfig::default()
        .with_allowed_hosts(authorities.clone())
        .with_allowed_origins(authorities.iter().map(|a| format!("http://{a}")))
        .with_cancellation_token(stop.child_token())
}

async fn serve(listener: TcpListener, port: u16, connect: Connect, stop: CancellationToken) {
    let listener = match tokio::net::TcpListener::from_std(listener) {
        Ok(listener) => listener,
        Err(error) => return log::line(&format!("the MCP server cannot listen: {error}")),
    };
    let service: Service = StreamableHttpService::new(
        move || Ok(Relay::http(&connect)),
        Arc::new(LocalSessionManager::default()),
        config(port, &stop),
    );
    while let Some(accepted) = stop.run_until_cancelled(listener.accept()).await {
        let Ok((stream, _)) = accepted else {
            // A failing accept (descriptor exhaustion) must not spin.
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        let (service, stop) = (service.clone(), stop.clone());
        tokio::spawn(async move {
            let handler = service_fn(move |request| {
                let service = service.clone();
                async move { Ok::<_, Infallible>(route(&service, request).await) }
            });
            let connection = http1::Builder::new().serve_connection(TokioIo::new(stream), handler);
            let _ = stop.run_until_cancelled(connection).await;
        });
    }
}

/// MCP at [`PATH`]; anything else is not found.
async fn route(
    service: &Service,
    request: Request<Incoming>,
) -> Response<BoxBody<Bytes, Infallible>> {
    if request.uri().path() == PATH {
        return service.handle(request).await;
    }
    let body = Full::new(Bytes::from_static(
        b"Not Found: MonoCode serves MCP at /mcp",
    ))
    .boxed();
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}
