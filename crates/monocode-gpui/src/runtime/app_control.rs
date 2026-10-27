//! MonoCode's control environment for each local agent session: the
//! window's control endpoint and the session's credential, which the
//! `app` command reads, and the `mcp` relay command with the window's data
//! flags (the command Settings shows), which providers that take MCP
//! servers start (`monocode_core::app_mcp`). Every local session gets it,
//! whatever its provider; only the server configuration depends on the
//! provider. Remote sessions run on another host, where neither the relay
//! nor this socket exists, and get nothing. Nor does any session while the
//! window's socket is not served (the bind failed), so nothing calls a
//! socket nobody answers. Orchestration workers get nothing either: they
//! keep their lead's scoped workflow. A Mono's chat and habit runs are told
//! they are a Mono's (`MONOCODE_CONTROL_ROLE=mono`), so their relay and
//! `app help` list the Mono's own tools; that is a hint, and the app still
//! decides from the credential alone who may call what.
use monocode_control::{Endpoint, lock};
use monocode_core::{agent::Settings, app_mcp, store::Store};
use std::{collections::HashMap, sync::Mutex};

/// The endpoints this process's control listeners serve now.
static SERVING: Mutex<Vec<Endpoint>> = Mutex::new(Vec::new());

/// Counts `endpoint` as served while the returned mark lives; the control
/// listener holds it from when it starts serving until it is dropped.
pub(crate) fn serving(endpoint: &Endpoint) -> Serving {
    lock(&SERVING).push(endpoint.clone());
    Serving(endpoint.clone())
}

/// A listener's mark that its endpoint is served.
pub(crate) struct Serving(Endpoint);

impl Drop for Serving {
    fn drop(&mut self) {
        let mut serving = lock(&SERVING);
        if let Some(at) = serving.iter().position(|e| *e == self.0) {
            serving.swap_remove(at);
        }
    }
}

/// Whether a control listener of this process serves `endpoint`.
pub(crate) fn served(endpoint: &Endpoint) -> bool {
    lock(&SERVING).contains(endpoint)
}

/// The control environment of local `session` in `cwd`, as `role`
/// (`app_mcp::ROLE_SESSION` or `ROLE_MONO`); empty for a remote session, or
/// while the window's socket is not served.
pub(super) fn control_env(
    store: &Store,
    session: i64,
    cwd: &str,
    role: &str,
) -> HashMap<String, String> {
    if monocode_core::remote::is_remote_path(cwd) {
        return HashMap::new();
    }
    let endpoint = store.data_dir().map(Endpoint::for_data_dir);
    let (Some(Ok(endpoint)), Ok(executable)) = (endpoint, std::env::current_exe()) else {
        return HashMap::new();
    };
    if !served(&endpoint) {
        return HashMap::new();
    }
    let mut args = vec!["mcp".to_owned()];
    args.extend(crate::data_location::flags());
    app_mcp::control_environment(session, &endpoint.describe(), &executable, &args, role)
}

/// What a turn of local `session` with `settings` gets: [`control_env`] as
/// a Mono's when the turn is a Mono's chat or habit run (its settings carry
/// the Mono), else as a session's; nothing for an orchestration worker.
pub(crate) fn for_turn(
    store: &Store,
    session: i64,
    cwd: &str,
    settings: &Settings,
) -> HashMap<String, String> {
    if settings.orchestration_worker {
        return HashMap::new();
    }
    let role = if settings.mono.is_some() {
        app_mcp::ROLE_MONO
    } else {
        app_mcp::ROLE_SESSION
    };
    control_env(store, session, cwd, role)
}

/// Why a turn of a session in `cwd` with `settings` gets no control
/// environment, for the prompts that tell its agent MonoCode tools are
/// unavailable in this run (§8.3).
pub(crate) fn unavailable(cwd: &str, settings: &Settings) -> &'static str {
    if monocode_core::remote::is_remote_path(cwd) {
        "this session runs on a remote host"
    } else if settings.orchestration_worker {
        "orchestration workers keep their lead's workflow"
    } else if settings.text_only {
        "this turn runs without tools"
    } else {
        "MonoCode's control socket is not served"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::provider::Kind;
    use std::path::Path;

    /// T-E1, §4.2: every local session gets the control environment, for
    /// its `app` command; only a provider that takes MCP servers is also
    /// configured with the relay. Pi, OpenCode and Omp get the environment
    /// and no server. A remote session, or a store without a data folder,
    /// gets nothing.
    #[test]
    fn every_local_session_gets_the_control_env_and_only_mcp_providers_the_relay() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/c/rt1");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let store = Store::open_monocode(&dir.join("monocode.db")).unwrap();
        let socket = dir.join("control/monocode-control.sock");
        let endpoint = Endpoint::for_data_dir(&dir).unwrap();
        let serving = serving(&endpoint);
        let local = control_env(&store, 7, "/work/monocode", app_mcp::ROLE_SESSION);
        assert_eq!(
            local[app_mcp::ENV_ENDPOINT],
            socket.display().to_string(),
            "the window's own socket"
        );
        assert_eq!(local[app_mcp::ENV_SESSION], "7");
        assert!(app_mcp::verify(7, &local[app_mcp::ENV_TOKEN]));
        assert_eq!(local[app_mcp::ENV_ROLE], "session");
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
            assert_eq!(
                app_mcp::acp_servers(kind, &local)[0]["name"],
                "monocode",
                "{kind:?} gets the relay"
            );
        }
        for kind in [Kind::OpenCode, Kind::Pi, Kind::Omp] {
            assert_eq!(
                app_mcp::acp_servers(kind, &local),
                serde_json::json!([]),
                "{kind:?} takes no MCP servers, and keeps the environment for `app`"
            );
        }
        let remote = control_env(
            &store,
            7,
            "remote://host/work/monocode",
            app_mcp::ROLE_SESSION,
        );
        assert!(remote.is_empty(), "a remote session gets nothing");
        let memory = Store::open(Path::new(":memory:")).unwrap();
        assert!(control_env(&memory, 7, "/work/monocode", app_mcp::ROLE_SESSION).is_empty());
        drop(serving);
        assert!(!served(&endpoint));
    }

    #[test]
    fn orchestration_workers_get_no_relay() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/c/rt3");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let store = Store::open_monocode(&dir.join("monocode.db")).unwrap();
        let served_now = serving(&Endpoint::for_data_dir(&dir).unwrap());
        let mut settings = Settings::default();
        assert!(!for_turn(&store, 7, "/work/monocode", &settings).is_empty());
        settings.orchestration_worker = true;
        assert!(
            for_turn(&store, 7, "/work/monocode", &settings).is_empty(),
            "a worker keeps its lead's scoped workflow"
        );
        drop(served_now);
    }

    /// T-M17's rule: a turn whose settings carry a Mono (its main chat or a
    /// habit run, `configure_mono`) is told it is a Mono's; any other turn
    /// a session's. Codex and ACP pass the hint on like the credential.
    #[test]
    fn a_monos_turns_are_told_they_are_a_monos() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/c/rt4");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let store = Store::open_monocode(&dir.join("monocode.db")).unwrap();
        let served_now = serving(&Endpoint::for_data_dir(&dir).unwrap());
        let mut settings = Settings::default();
        let role = |settings: &Settings| {
            for_turn(&store, 7, "/work/monocode", settings)[app_mcp::ENV_ROLE].clone()
        };
        assert_eq!(role(&settings), "session");
        let mono = monocode_core::mono::Roster::default()
            .create(vec!["/work/monocode".into()])
            .clone();
        settings.mono = Some(monocode_core::agent::MonoContext {
            mono,
            root: dir.clone(),
            executable: dir.join("monocode-gpui"),
        });
        assert_eq!(role(&settings), "mono");
        let local = for_turn(&store, 7, "/work/monocode", &settings);
        let mut command = std::process::Command::new("codex");
        app_mcp::configure_codex(&mut command, &local);
        assert!(
            command
                .get_args()
                .any(|arg| arg.to_string_lossy().contains("\"MONOCODE_CONTROL_ROLE\"]")),
            "Codex forwards it by name"
        );
        assert_eq!(
            app_mcp::acp_servers(Kind::Cursor, &local)[0]["env"][3]["value"],
            "mono",
            "ACP lists it"
        );
        drop(served_now);
    }

    /// §8.3: a turn without the control environment is told why, in the
    /// Operator instructions and a Mono's context.
    #[test]
    fn a_turn_without_the_control_env_is_told_why() {
        let mut settings = Settings::default();
        assert_eq!(
            unavailable("/work/monocode", &settings),
            "MonoCode's control socket is not served"
        );
        assert_eq!(
            unavailable("remote://host/work/monocode", &settings),
            "this session runs on a remote host"
        );
        settings.text_only = true;
        assert_eq!(
            unavailable("/work/monocode", &settings),
            "this turn runs without tools"
        );
        settings.orchestration_worker = true;
        assert_eq!(
            unavailable("/work/monocode", &settings),
            "orchestration workers keep their lead's workflow"
        );
    }

    #[test]
    fn no_session_gets_the_relay_while_the_socket_is_not_served() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/c/rt2");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let store = Store::open_monocode(&dir.join("monocode.db")).unwrap();
        let endpoint = Endpoint::for_data_dir(&dir).unwrap();
        assert!(!served(&endpoint));
        assert!(
            control_env(&store, 7, "/work/monocode", app_mcp::ROLE_SESSION).is_empty(),
            "a socket nobody serves gets no relay"
        );
        let served_now = serving(&endpoint);
        assert!(!control_env(&store, 7, "/work/monocode", app_mcp::ROLE_SESSION).is_empty());
        drop(served_now);
        assert!(
            control_env(&store, 7, "/work/monocode", app_mcp::ROLE_SESSION).is_empty(),
            "nor once it stops"
        );
    }
}
