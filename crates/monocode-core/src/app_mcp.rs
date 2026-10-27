//! The MCP servers MonoCode adds to each local agent session, in each
//! provider's configuration shape: the native visual server
//! (`monocode_visual`) and MonoCode's own control relay (`monocode`, the
//! `monocode-gpui mcp` command). Both are described by the session's
//! transport-only environment, which the provider process inherits. The
//! variables that carry credentials reach a server through the provider:
//! Claude passes its environment on, Codex forwards them by name, and ACP
//! sends them in `session/new`. No credential is ever put on a command line.
//! A provider that takes no MCP servers ([`accepts_servers`]) still gets the
//! control environment, for the `app` command its agent runs, but no server
//! configuration.
use crate::provider::Kind;
use serde_json::{json, Value};
use std::{collections::HashMap, path::Path, process::Command};

mod credential;
#[cfg(test)]
mod tests;

pub use credential::{token, verify};

/// The control endpoint the relay connects to (`Endpoint::describe`).
pub const ENV_ENDPOINT: &str = "MONOCODE_CONTROL_ENDPOINT";
/// The session the relay speaks for.
pub const ENV_SESSION: &str = "MONOCODE_CONTROL_SESSION";
/// The session's credential.
pub const ENV_TOKEN: &str = "MONOCODE_CONTROL_TOKEN";
/// What kind of agent the session is, [`ROLE_SESSION`] or [`ROLE_MONO`]: a
/// hint for the relay's tool list, never used to decide what it may call.
pub const ENV_ROLE: &str = "MONOCODE_CONTROL_ROLE";
/// [`ENV_ROLE`] of a Mono's main chat or habit run.
pub const ROLE_MONO: &str = "mono";
/// [`ENV_ROLE`] of every other session.
pub const ROLE_SESSION: &str = "session";
/// The relay's executable, for the server definition only.
const ENV_COMMAND: &str = "MONOCODE_CONTROL_COMMAND";
/// The relay's arguments as a JSON array, for the server definition only.
const ENV_ARGS: &str = "MONOCODE_CONTROL_ARGS";
/// What the relay itself reads.
const RELAY_ENV: &[&str] = &[ENV_ENDPOINT, ENV_SESSION, ENV_TOKEN, ENV_ROLE];
/// Every variable of a session's control environment
/// ([`control_environment`]): what a process shared by several sessions, or
/// one started outside any, must not inherit from one of them.
pub const CONTROL_ENV: [&str; 6] = [
    ENV_ENDPOINT,
    ENV_SESSION,
    ENV_TOKEN,
    ENV_ROLE,
    ENV_COMMAND,
    ENV_ARGS,
];
/// The control relay's server name.
pub(crate) const CONTROL_SERVER: &str = "monocode";

/// One stdio MCP server a session gets.
pub(crate) struct Server {
    pub(crate) name: &'static str,
    pub(crate) command: String,
    pub(crate) args: Vec<String>,
    /// The variables the server reads, passed on by name.
    pub(crate) env: &'static [&'static str],
}

/// The environment that gives local session `session` the control relay:
/// `command args…` connecting to `endpoint` with the session's credential,
/// and `role` ([`ENV_ROLE`]) for its tool list.
#[must_use]
pub fn control_environment(
    session: i64,
    endpoint: &str,
    command: &Path,
    args: &[String],
    role: &str,
) -> HashMap<String, String> {
    HashMap::from([
        (ENV_ENDPOINT.into(), endpoint.to_owned()),
        (ENV_SESSION.into(), session.to_string()),
        (ENV_TOKEN.into(), token(session)),
        (ENV_ROLE.into(), role.to_owned()),
        (ENV_COMMAND.into(), command.to_string_lossy().into_owned()),
        (ENV_ARGS.into(), json!(args).to_string()),
    ])
}

/// The control relay, when the environment describes it completely.
fn control_server(environment: &HashMap<String, String>) -> Option<Server> {
    if !RELAY_ENV.iter().all(|key| environment.contains_key(*key)) {
        return None;
    }
    Some(Server {
        name: CONTROL_SERVER,
        command: environment.get(ENV_COMMAND)?.clone(),
        args: serde_json::from_str(environment.get(ENV_ARGS)?).ok()?,
        env: RELAY_ENV,
    })
}

/// Whether `kind` takes MonoCode's control server in its configuration.
/// OpenCode, Pi and Omp take none: their sessions get the control
/// environment for the `app` command, and no control server.
#[must_use]
pub fn accepts_servers(kind: Kind) -> bool {
    !matches!(kind, Kind::OpenCode | Kind::Pi | Kind::Omp)
}

/// Whether a resumed `kind` session still has the servers it started
/// with. Grok and Fx resume with the session ID alone (`session/resume`),
/// so a resumed turn has no control server, only the `app` command.
#[must_use]
pub fn keeps_servers_on_resume(kind: Kind) -> bool {
    !matches!(kind, Kind::Grok | Kind::Fx)
}

/// How a turn's agent reaches MonoCode's tools, as its prompt tells it
/// (`/operator`'s instructions, a Mono's context).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// The `monocode` MCP tools: the turn has the control environment and
    /// its provider takes the server on every turn.
    Mcp,
    /// The `app` command only: the turn has the control environment and
    /// its provider takes no MCP servers, or drops them when it resumes
    /// ([`keeps_servers_on_resume`]), so only the command always works.
    Cli,
    /// Neither, for the reason given: the turn has no control environment.
    Unavailable(&'static str),
}

impl Access {
    /// The access of a `kind` turn that gets `control`, its control
    /// environment; `why` says why that is empty when it is.
    #[must_use]
    pub fn of(kind: Kind, control: &HashMap<String, String>, why: &'static str) -> Self {
        if control.is_empty() {
            Self::Unavailable(why)
        } else if accepts_servers(kind) && keeps_servers_on_resume(kind) {
            Self::Mcp
        } else {
            Self::Cli
        }
    }
}

/// Every server this session's environment describes for a `kind` session:
/// visuals, then control when `kind` takes it. The visual server is
/// configured whatever the provider, as before.
fn servers(kind: Kind, environment: &HashMap<String, String>) -> Vec<Server> {
    crate::visual_bridge::app_server(environment)
        .into_iter()
        .chain(control_server(environment).filter(|_| accepts_servers(kind)))
        .collect()
}

/// Claude: one inline `--mcp-config`. Its servers inherit Claude's
/// environment, so the definitions name no variables.
pub fn configure_claude(command: &mut Command, environment: &HashMap<String, String>) {
    let servers: serde_json::Map<String, Value> = servers(Kind::Claude, environment)
        .into_iter()
        .map(|server| {
            let definition = json!({
                "type": "stdio", "command": server.command, "args": server.args
            });
            (server.name.to_owned(), definition)
        })
        .collect();
    if !servers.is_empty() {
        let config = json!({ "mcpServers": servers });
        command.args(["--mcp-config", &config.to_string()]);
    }
}

/// Codex: `-c mcp_servers.<name>.*` overrides that forward each server's
/// variables by name (`env_vars`), never their values.
pub fn configure_codex(command: &mut Command, environment: &HashMap<String, String>) {
    for server in servers(Kind::Codex, environment) {
        for (key, value) in [
            ("command", json!(server.command)),
            ("args", json!(server.args)),
            ("env_vars", json!(server.env)),
        ] {
            // JSON's strings and arrays are valid TOML literals for these values.
            let name = server.name;
            command.args(["-c", &format!("mcp_servers.{name}.{key}={value}")]);
        }
    }
}

/// ACP: the `mcpServers` of `session/new` and `session/load` for a `kind`
/// session. The stdio shape has no transport discriminator and lists its
/// environment.
#[must_use]
pub fn acp_servers(kind: Kind, environment: &HashMap<String, String>) -> Value {
    servers(kind, environment)
        .into_iter()
        .map(|server| {
            let env: Vec<Value> = server
                .env
                .iter()
                .map(|key| json!({"name": key, "value": environment[*key]}))
                .collect();
            json!({
                "name": server.name, "command": server.command, "args": server.args, "env": env
            })
        })
        .collect::<Vec<_>>()
        .into()
}
