//! What Settings → General → MCP server shows and changes, the commands and
//! links its Add to buttons hand out, and where its copy and open buttons
//! send them.
use base64::Engine as _;
use gpui::{App, ClipboardItem, Global};
use std::{fmt::Write as _, rc::Rc};

/// One logged call or approval decision, as Settings lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ControlActivity {
    /// When, in Unix milliseconds.
    pub at: i64,
    pub client: String,
    /// The operation, or `approve` / `decline` for a connection decision.
    pub operation: String,
    /// What it acted on, such as `session 7` or a project's path.
    pub target: Option<String>,
    /// `ok` or the error code.
    pub outcome: String,
}

/// The MCP server as Settings shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExternalControlStatus {
    /// Whether the person turned the MCP server on.
    pub enabled: bool,
    /// Where clients connect, while the server runs.
    pub url: Option<String>,
    /// The port it serves on, or tried to.
    pub port: u16,
    /// Why it is on but not serving, such as a port another app uses.
    pub error: Option<String>,
    pub clients: Vec<monocode_control::ApprovedClient>,
    /// What clients did, newest first.
    pub activity: Vec<ControlActivity>,
    /// Clients the person denied recently; they are refused without asking.
    pub denied: Vec<String>,
}

/// A change to the MCP server made in Settings; nothing else changes it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ExternalControlEdit {
    Enabled(bool),
    /// Serves on this port instead.
    Port(u16),
    Scope {
        client: String,
        scope: monocode_control::Scope,
        on: bool,
    },
    Revoke(String),
    /// Approves a client the person denied earlier.
    Allow(String),
    ClearActivity,
}

/// Takes a text or link from a button.
pub(crate) type Hand = Rc<dyn Fn(&str, &mut App)>;

/// Where Settings' copy and open buttons hand text and links: the system
/// clipboard and URL opener, unless a test put its own in place.
#[derive(Clone)]
pub(crate) struct Handoff {
    pub copy: Hand,
    pub open: Hand,
}

impl Global for Handoff {}

impl Handoff {
    /// Puts `text` on the clipboard.
    pub(crate) fn copy(text: &str, cx: &mut App) {
        match cx.try_global::<Self>().map(|h| h.copy.clone()) {
            Some(copy) => copy(text, cx),
            None => cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned())),
        }
    }

    /// Opens `url` with the app that handles it.
    pub(crate) fn open(url: &str, cx: &mut App) {
        match cx.try_global::<Self>().map(|h| h.open.clone()) {
            Some(open) => open(url, cx),
            None => cx.open_url(url),
        }
    }
}

/// How the Add to buttons add MonoCode to an MCP client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Install {
    /// A terminal command, copied.
    Copy,
    /// A link the client's app opens.
    Open,
}

/// A client the Add to row offers.
pub(crate) struct Client {
    pub id: &'static str,
    pub label: &'static str,
    pub install: Install,
    /// The command or link for the server's URL.
    pub text: fn(&str) -> String,
}

/// The clients the Add to row offers, in its order.
pub(crate) const CLIENTS: &[Client] = &[
    Client {
        id: "claude-code",
        label: "Claude Code",
        install: Install::Copy,
        text: claude_command,
    },
    Client {
        id: "codex",
        label: "Codex",
        install: Install::Copy,
        text: codex_command,
    },
    Client {
        id: "cursor",
        label: "Cursor",
        install: Install::Open,
        text: cursor_link,
    },
    Client {
        id: "vscode",
        label: "VS Code",
        install: Install::Open,
        text: vscode_link,
    },
];

/// Claude Code's command for an HTTP server.
pub(crate) fn claude_command(url: &str) -> String {
    format!("claude mcp add --transport http monocode {url}")
}

/// Codex's command for a Streamable HTTP server.
pub(crate) fn codex_command(url: &str) -> String {
    format!("codex mcp add monocode --url {url}")
}

/// Cursor's install link: the server's `mcp.json` entry, `{"url": …}` for a
/// remote server, as base64 of its JSON.
pub(crate) fn cursor_link(url: &str) -> String {
    let config = format!(r#"{{"url":{}}}"#, json_string(url));
    let config = base64::engine::general_purpose::STANDARD.encode(config);
    format!("cursor://anysphere.cursor-deeplink/mcp/install?name=monocode&config={config}")
}

/// VS Code's install link: the server's definition as JSON, encoded as
/// JavaScript's `encodeURIComponent` does.
pub(crate) fn vscode_link(url: &str) -> String {
    let config = format!(
        r#"{{"name":"monocode","type":"http","url":{}}}"#,
        json_string(url)
    );
    format!("vscode:mcp/install?{}", encode_uri_component(&config))
}

fn json_string(text: &str) -> String {
    serde_json::Value::String(text.to_owned()).to_string()
}

/// Percent-encodes every byte but the ones `encodeURIComponent` keeps.
fn encode_uri_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}
