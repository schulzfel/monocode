//! The MCP servers MonoCode adds to each local agent session, in each
//! provider's configuration shape: the native visual server
//! (`monocode_visual`) and MonoCode's own control relay (`monocode`, the
//! `monocode-gpui mcp` command). Both are described by the session's
//! transport-only environment, which the provider process inherits. The
//! variables that carry credentials reach a server through the provider:
//! Claude passes its environment on, Codex forwards them by name, and ACP
//! sends them in `session/new`. No credential is ever put on a command line.
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
/// The relay's executable, for the server definition only.
const ENV_COMMAND: &str = "MONOCODE_CONTROL_COMMAND";
/// The relay's arguments as a JSON array, for the server definition only.
const ENV_ARGS: &str = "MONOCODE_CONTROL_ARGS";
/// What the relay itself reads.
const RELAY_ENV: &[&str] = &[ENV_ENDPOINT, ENV_SESSION, ENV_TOKEN];
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
/// `command args…` connecting to `endpoint` with the session's credential.
#[must_use]
pub fn control_environment(
    session: i64,
    endpoint: &str,
    command: &Path,
    args: &[String],
) -> HashMap<String, String> {
    HashMap::from([
        (ENV_ENDPOINT.into(), endpoint.to_owned()),
        (ENV_SESSION.into(), session.to_string()),
        (ENV_TOKEN.into(), token(session)),
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

/// Whether `kind` takes app MCP servers in its configuration. OpenCode, Pi
/// and Omp take none, so their sessions get neither servers nor credentials.
#[must_use]
pub fn accepts_servers(kind: crate::provider::Kind) -> bool {
    use crate::provider::Kind;
    !matches!(kind, Kind::OpenCode | Kind::Pi | Kind::Omp)
}

/// Every server this session's environment describes: visuals, then control.
fn servers(environment: &HashMap<String, String>) -> Vec<Server> {
    crate::visual_bridge::app_server(environment)
        .into_iter()
        .chain(control_server(environment))
        .collect()
}

/// Claude: one inline `--mcp-config`. Its servers inherit Claude's
/// environment, so the definitions name no variables.
pub fn configure_claude(command: &mut Command, environment: &HashMap<String, String>) {
    let servers: serde_json::Map<String, Value> = servers(environment)
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
    for server in servers(environment) {
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

/// ACP: the `mcpServers` of `session/new` and `session/load`. The stdio
/// shape has no transport discriminator and lists its environment.
#[must_use]
pub fn acp_servers(environment: &HashMap<String, String>) -> Value {
    servers(environment)
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
