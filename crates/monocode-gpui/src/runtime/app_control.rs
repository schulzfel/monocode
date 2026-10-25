//! MonoCode's own MCP server for each local agent session: the provider
//! starts this executable's `mcp` relay with the window's data flags (the
//! command Settings shows), and the relay reaches this window's control
//! socket as the session (`monocode_core::app_mcp`). Remote sessions run on
//! another host, where neither the relay nor this socket exists, and
//! providers that take no MCP configuration get nothing.
use monocode_control::Endpoint;
use monocode_core::{app_mcp, provider::Kind, store::Store};
use std::collections::HashMap;

/// The environment that gives local `session` (`kind` in `cwd`) the relay;
/// empty when it gets none.
pub(super) fn environment(
    store: &Store,
    session: i64,
    kind: Kind,
    cwd: &str,
) -> HashMap<String, String> {
    if monocode_core::remote::is_remote_path(cwd) || !app_mcp::accepts_servers(kind) {
        return HashMap::new();
    }
    let endpoint = store.data_dir().map(Endpoint::for_data_dir);
    let (Some(Ok(endpoint)), Ok(executable)) = (endpoint, std::env::current_exe()) else {
        return HashMap::new();
    };
    let mut args = vec!["mcp".to_owned()];
    args.extend(crate::data_location::flags());
    app_mcp::control_environment(session, &endpoint.describe(), &executable, &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn local_sessions_of_providers_with_mcp_get_the_relay_and_others_nothing() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/c/rt1");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let store = Store::open_monocode(&dir.join("monocode.db")).unwrap();
        let local = environment(&store, 7, Kind::Codex, "/work/monocode");
        let socket = dir.join("control/monocode-control.sock");
        assert_eq!(
            local[app_mcp::ENV_ENDPOINT],
            socket.display().to_string(),
            "the window's own socket"
        );
        assert_eq!(local[app_mcp::ENV_SESSION], "7");
        assert!(app_mcp::verify(7, &local[app_mcp::ENV_TOKEN]));
        let mut command = std::process::Command::new("codex");
        app_mcp::configure_codex(&mut command, &local);
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(
            args.contains(&r#"mcp_servers.monocode.args=["mcp","--dev-data"]"#.to_owned()),
            "{args:?}"
        );
        for kind in [Kind::Claude, Kind::Cursor, Kind::Grok, Kind::Devin] {
            assert!(!environment(&store, 7, kind, "/work/monocode").is_empty());
        }
        for kind in [Kind::OpenCode, Kind::Pi, Kind::Omp] {
            assert!(environment(&store, 7, kind, "/work/monocode").is_empty());
        }
        let remote = environment(&store, 7, Kind::Claude, "remote://host/work/monocode");
        assert!(remote.is_empty(), "a remote session gets no relay");
        let memory = Store::open(Path::new(":memory:")).unwrap();
        assert!(environment(&memory, 7, Kind::Claude, "/work/monocode").is_empty());
    }
}
