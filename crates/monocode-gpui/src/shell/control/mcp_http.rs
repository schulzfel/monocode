//! The MCP server the app serves over Streamable HTTP while the person has
//! it on (Settings → General → MCP server): `http://127.0.0.1:<port>/mcp`,
//! on the port the policy names or the data folder's default. Off, nothing
//! listens. Its sessions reach the shell in-process (`in_app`), on the same
//! channel as the control socket's connections.
use super::{in_app::InApp, *};
use monocode_control::ops::app::DataMode;
use monocode_mcp::{
    Connect,
    http::{DEV_PORT, HttpServer, RELEASE_PORT},
};
use std::sync::Arc;

/// The running server and the port it was asked for (`0`: any free one).
pub(super) struct Serving {
    asked: u16,
    server: HttpServer,
}

/// The default port of the data the app opened: the installed app's and a
/// development build's differ, so both can run at once.
pub(super) fn default_port(mode: DataMode) -> u16 {
    match mode {
        DataMode::Monocode => RELEASE_PORT,
        DataMode::Dev => DEV_PORT,
    }
}

/// Why `port` could not be served, as Settings says it.
fn bind_error(port: u16, error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => {
            format!("Port {port} is in use by another app. Choose another port.")
        }
        std::io::ErrorKind::PermissionDenied => {
            format!("MonoCode may not listen on port {port}. Choose another port.")
        }
        _ => format!("MonoCode cannot listen on port {port}: {error}"),
    }
}

impl Shell {
    /// The port the MCP server should use.
    pub(super) fn mcp_port(&self) -> u16 {
        self.control
            .policy
            .port
            .unwrap_or(self.control.default_port)
    }

    /// Serves MCP over HTTP while the policy has it on, on its port, and
    /// stops otherwise. A port that cannot be bound is kept as the error
    /// Settings shows; the next change tries again.
    pub(super) fn apply_mcp_http(&mut self) {
        let port = self.mcp_port();
        if !self.control.policy.enabled {
            self.control.http = None;
            self.control.http_error = None;
            return;
        }
        if self.control.http.as_ref().is_some_and(|s| s.asked == port) {
            return;
        }
        // The old server goes first, freeing its port.
        self.control.http = None;
        let requests = self.control.requests.clone();
        let connect: Connect =
            Arc::new(move |notices| Box::new(InApp::new(requests.clone(), notices)));
        match HttpServer::start(port, connect) {
            Ok(server) => {
                self.control.http = Some(Serving {
                    asked: port,
                    server,
                });
                self.control.http_error = None;
            }
            Err(error) => self.control.http_error = Some(bind_error(port, &error)),
        }
    }

    /// The URL clients use, while the server runs.
    pub(super) fn mcp_url(&self) -> Option<String> {
        self.control.http.as_ref().map(|s| s.server.url())
    }
}
