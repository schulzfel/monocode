//! The desktop app's MCP server, one handler on two transports.
//!
//! The handler publishes one tool per operation in
//! [`monocode_control::catalog`] with `rmcp`, plus read-only resources and
//! their subscriptions, and hands each call to a [`Backend`]. It holds no
//! state of its own beyond the resources a client subscribed to.
//!
//! - [`http::HttpServer`]: the app itself serves Streamable HTTP at
//!   `http://127.0.0.1:<port>/mcp` while the person has the MCP server on;
//!   its backend is the app's own control pipeline.
//! - [`run_stdio`]: `monocode-gpui mcp`, a relay on standard input and
//!   output that forwards each call over the local control socket to the
//!   running app, connecting only when the first tool is called or a
//!   resource is subscribed. `app_launch` is the one tool it answers itself.
//!   Standard output carries the protocol and nothing else; diagnostics go
//!   to standard error.

mod backend;
mod errors;
pub mod http;
mod identity;
mod idle;
mod launch;
mod link;
mod log;
mod resources;
mod server;
mod socket;
mod tools;
mod watch;

pub use backend::{Backend, Connect};
pub use launch::{ControlProbe, LAUNCH_WAIT, Launcher, Spawner};
use monocode_control::{ControlError, Endpoint, SessionCredential};
use rmcp::{RoleServer, ServiceExt, transport::IntoTransport};
use std::{sync::Arc, time::Duration};

/// Decides what a caller is told when no app answers: `control_disabled`
/// when the person turned control off, `app_not_running` otherwise.
pub type UnavailableProbe = Arc<dyn Fn() -> ControlError + Send + Sync>;

/// Which agent a session relay speaks for, as the app hints it in the
/// session's environment (`MONOCODE_CONTROL_ROLE`). It decides only which
/// tools the relay lists: the app decides what each caller may call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    /// Any agent session, and every external client.
    #[default]
    Session,
    /// A Mono's main chat or habit run: its own tools are listed too.
    Mono,
}

/// How the relay finds and identifies itself to the app.
#[derive(Clone)]
pub struct Options {
    endpoint: Result<Endpoint, ControlError>,
    client_name: Option<String>,
    unavailable: UnavailableProbe,
    launcher: Option<Launcher>,
    session: Option<SessionCredential>,
    role: Role,
    release_after: Duration,
}

impl Options {
    /// Relays to the app on `endpoint`. An endpoint error (a socket path that
    /// is too long) is returned by every tool call.
    #[must_use]
    pub fn new(endpoint: Result<Endpoint, ControlError>) -> Self {
        Self {
            endpoint,
            client_name: None,
            unavailable: Arc::new(ControlError::app_not_running),
            launcher: None,
            session: None,
            role: Role::Session,
            release_after: idle::RELEASE_AFTER,
        }
    }
    /// Speaks for the agent session `credential` names: the relay of a
    /// session the app runs. It calls as `session:<id>`, whatever the
    /// client's name, and the server instructions say it runs inside the
    /// app.
    #[must_use]
    pub fn session(mut self, credential: Option<SessionCredential>) -> Self {
        self.session = credential;
        self
    }
    /// Lists the tools `role` calls: a Mono's relay lists the operations
    /// only a Mono may call as well. A hint only: the app still refuses them
    /// to every other caller, and any relay passes any call on.
    #[must_use]
    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }
    /// Calls as `name` instead of the MCP client's reported name.
    #[must_use]
    pub fn client_name(mut self, name: Option<String>) -> Self {
        self.client_name = name;
        self
    }
    /// Asks `probe` what to report when no app answers.
    #[must_use]
    pub fn when_unavailable(mut self, probe: UnavailableProbe) -> Self {
        self.unavailable = probe;
        self
    }
    /// Lets `app_launch` start the app with `launcher`. Without one,
    /// `app_launch` only reports whether the app runs.
    #[must_use]
    pub fn launcher(mut self, launcher: Launcher) -> Self {
        self.launcher = Some(launcher);
        self
    }
    /// Hangs up a control connection that follows nothing once no call has
    /// used it for `after` (a minute unless set), so the app's connection
    /// slots stay free; the next call connects again.
    #[must_use]
    pub fn release_after(mut self, after: Duration) -> Self {
        self.release_after = after;
        self
    }
}

/// Serves MCP on standard input and output until the client disconnects.
///
/// # Errors
/// When the runtime cannot start or the MCP session fails.
pub fn run_stdio(options: Options) -> Result<(), String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Cannot start the MCP runtime: {e}"))?
        .block_on(serve(options, rmcp::transport::stdio()))
}

/// Serves MCP on `transport` until the client disconnects.
///
/// # Errors
/// When the MCP session fails to start or ends with an error.
pub async fn serve<T, E, A>(options: Options, transport: T) -> Result<(), String>
where
    T: IntoTransport<RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let running = server::Relay::stdio(options)
        .serve(transport)
        .await
        .map_err(|e| match e {
            // `rmcp` names only `initialize` here, whichever lifecycle the
            // client was on.
            rmcp::service::ServerInitializeError::ConnectionClosed(_) => {
                "The MCP client closed the connection before a session started: it sent neither initialize (protocol 2025-11-25) nor, after any server/discover, a request with its protocol metadata (2026-07-28).".to_owned()
            }
            e => format!("The MCP session did not start: {e}"),
        })?;
    running
        .waiting()
        .await
        .map_err(|e| format!("The MCP session failed: {e}"))?;
    log::line("the MCP client disconnected");
    Ok(())
}
