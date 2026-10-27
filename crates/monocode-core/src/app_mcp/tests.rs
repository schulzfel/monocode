//! Each provider's shape of the app servers, inspected without running one.
use super::*;
use crate::provider::Kind;

fn control(session: i64) -> HashMap<String, String> {
    control_environment(
        session,
        "/data/control/monocode-control.sock",
        Path::new("/Applications/MonoCode.app/monocode-gpui"),
        &["mcp".into(), "--dev-data".into()],
        ROLE_SESSION,
    )
}

fn arguments(command: &Command) -> Vec<String> {
    command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn claude_gets_the_relay_inline_and_inherits_the_credential() {
    let environment = control(7);
    let mut command = Command::new("claude");
    configure_claude(&mut command, &environment);
    let args = arguments(&command);
    assert_eq!(args.len(), 2, "{args:?}");
    assert_eq!(args[0], "--mcp-config");
    let config: Value = serde_json::from_str(&args[1]).unwrap();
    assert_eq!(
        config["mcpServers"]["monocode"],
        json!({
            "type": "stdio",
            "command": "/Applications/MonoCode.app/monocode-gpui",
            "args": ["mcp", "--dev-data"]
        })
    );
    assert!(
        !args[1].contains(&environment[ENV_TOKEN]),
        "no token in argv"
    );
}

#[test]
fn codex_forwards_the_credential_by_name_and_never_puts_the_token_in_argv() {
    let environment = control(7);
    let mut command = Command::new("codex");
    configure_codex(&mut command, &environment);
    let args = arguments(&command);
    let overrides: Vec<&str> = args
        .iter()
        .filter(|arg| *arg != "-c")
        .map(String::as_str)
        .collect();
    assert_eq!(
        overrides,
        [
            r#"mcp_servers.monocode.command="/Applications/MonoCode.app/monocode-gpui""#,
            r#"mcp_servers.monocode.args=["mcp","--dev-data"]"#,
            r#"mcp_servers.monocode.env_vars=["MONOCODE_CONTROL_ENDPOINT","MONOCODE_CONTROL_SESSION","MONOCODE_CONTROL_TOKEN","MONOCODE_CONTROL_ROLE"]"#,
        ]
    );
    let token = &environment[ENV_TOKEN];
    assert_eq!(token.len(), 64);
    assert!(args.iter().all(|arg| !arg.contains(token.as_str())));
}

#[test]
fn acp_lists_the_relay_with_its_environment() {
    let environment = control(7);
    let servers = acp_servers(Kind::Cursor, &environment);
    assert_eq!(
        servers,
        json!([{
            "name": "monocode",
            "command": "/Applications/MonoCode.app/monocode-gpui",
            "args": ["mcp", "--dev-data"],
            "env": [
                {"name": "MONOCODE_CONTROL_ENDPOINT", "value": "/data/control/monocode-control.sock"},
                {"name": "MONOCODE_CONTROL_SESSION", "value": "7"},
                {"name": "MONOCODE_CONTROL_TOKEN", "value": environment[ENV_TOKEN]},
                {"name": "MONOCODE_CONTROL_ROLE", "value": "session"},
            ]
        }])
    );
}

/// A Mono's chat and habit runs are told they are a Mono's, for their
/// relay's tool list; Codex forwards it by name and ACP lists its value.
#[test]
fn a_monos_relay_gets_its_role_like_its_credential() {
    let environment = control_environment(
        9,
        "/data/control/monocode-control.sock",
        Path::new("/Applications/MonoCode.app/monocode-gpui"),
        &["mcp".into()],
        ROLE_MONO,
    );
    assert_eq!(environment[ENV_ROLE], "mono");
    let mut command = Command::new("codex");
    configure_codex(&mut command, &environment);
    assert!(arguments(&command)
        .iter()
        .any(|arg| arg.ends_with(r#""MONOCODE_CONTROL_TOKEN","MONOCODE_CONTROL_ROLE"]"#)));
    let acp = acp_servers(Kind::Cursor, &environment);
    assert_eq!(
        acp[0]["env"][3],
        json!({"name": "MONOCODE_CONTROL_ROLE", "value": "mono"})
    );
}

#[test]
fn visuals_and_control_share_one_claude_config() {
    let mut environment = control(7);
    environment.extend([
        ("MONOCODE_VISUAL_ENDPOINT".into(), "127.0.0.1:1".into()),
        ("MONOCODE_VISUAL_TOKEN".into(), "visual".into()),
        ("MONOCODE_VISUAL_SESSION".into(), "7".into()),
        ("MONOCODE_VISUAL_EXECUTABLE".into(), "/app".into()),
    ]);
    let mut command = Command::new("claude");
    configure_claude(&mut command, &environment);
    let args = arguments(&command);
    assert_eq!(args.len(), 2, "one --mcp-config: {args:?}");
    let config: Value = serde_json::from_str(&args[1]).unwrap();
    let names: Vec<&String> = config["mcpServers"].as_object().unwrap().keys().collect();
    assert_eq!(names, ["monocode", "monocode_visual"]);
    let acp = acp_servers(Kind::Cursor, &environment);
    assert_eq!(acp[0]["name"], "monocode_visual");
    assert_eq!(acp[1]["name"], "monocode");
}

#[test]
fn a_session_without_app_servers_configures_nothing() {
    let empty = HashMap::new();
    let mut command = Command::new("provider");
    configure_claude(&mut command, &empty);
    configure_codex(&mut command, &empty);
    assert!(arguments(&command).is_empty());
    assert_eq!(acp_servers(Kind::Cursor, &empty), json!([]));
    let mut partial = control(7);
    partial.remove(ENV_TOKEN);
    assert_eq!(
        acp_servers(Kind::Cursor, &partial),
        json!([]),
        "no credential, no relay"
    );
}

#[test]
fn only_providers_that_take_mcp_configuration_get_servers() {
    let takes: Vec<Kind> = Kind::ALL
        .into_iter()
        .filter(|kind| accepts_servers(*kind))
        .collect();
    assert_eq!(
        takes,
        [
            Kind::Claude,
            Kind::Codex,
            Kind::Cursor,
            Kind::Grok,
            Kind::Fx,
            Kind::Hermes,
            Kind::Antigravity,
            Kind::Devin
        ]
    );
}

/// T-E1: the control environment no longer implies the control server. A
/// provider that takes none is configured without it, however complete the
/// environment it gets for the `app` command.
#[test]
fn a_provider_without_mcp_gets_no_servers_from_a_complete_environment() {
    let environment = control(7);
    for kind in Kind::ALL {
        let listed = acp_servers(kind, &environment);
        if accepts_servers(kind) {
            assert_eq!(listed[0]["name"], "monocode", "{kind:?}");
        } else {
            assert_eq!(listed, json!([]), "{kind:?}");
        }
    }
    assert!(servers(Kind::Pi, &environment).is_empty());
    assert!(servers(Kind::OpenCode, &environment).is_empty());
    assert!(servers(Kind::Omp, &environment).is_empty());
    assert_eq!(servers(Kind::Claude, &environment).len(), 1);
}

/// §10.1: what a turn's prompt names follows what the turn gets: the MCP
/// tools where the provider takes the server on every turn, the `app`
/// command where it takes none or drops it on resume, and neither without
/// the control environment.
#[test]
fn a_turns_access_follows_its_control_env_and_provider() {
    let environment = control(7);
    for kind in [Kind::Grok, Kind::Fx] {
        assert_eq!(
            Access::of(kind, &environment, "why"),
            Access::Cli,
            "{kind:?} resumes without its servers"
        );
    }
    for kind in Kind::ALL {
        let expected = if accepts_servers(kind) && keeps_servers_on_resume(kind) {
            Access::Mcp
        } else {
            Access::Cli
        };
        assert_eq!(Access::of(kind, &environment, "why"), expected, "{kind:?}");
        assert_eq!(
            Access::of(kind, &HashMap::new(), "why"),
            Access::Unavailable("why"),
            "{kind:?}"
        );
    }
}

#[test]
fn a_token_belongs_to_one_session_in_one_run() {
    let seven = token(7);
    assert_eq!(seven, token(7), "stable for the run");
    assert_ne!(seven, token(8));
    assert!(verify(7, &seven));
    assert!(!verify(8, &seven));
    assert!(!verify(7, &seven[..63]));
    assert!(!verify(7, ""));
}

/// `CONTROL_ENV` names every variable a session's control environment
/// holds, so a process that must not inherit one can drop them all.
#[test]
fn control_env_lists_every_variable_of_the_environment() {
    let mut listed: Vec<&str> = CONTROL_ENV.to_vec();
    let mut held: Vec<String> = control(7).into_keys().collect();
    listed.sort_unstable();
    held.sort_unstable();
    assert_eq!(listed, held);
}
