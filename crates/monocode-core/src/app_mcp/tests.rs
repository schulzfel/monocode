//! Each provider's shape of the app servers, inspected without running one.
use super::*;
use crate::provider::Kind;

fn control(session: i64) -> HashMap<String, String> {
    control_environment(
        session,
        "/data/control/monocode-control.sock",
        Path::new("/Applications/MonoCode.app/monocode-gpui"),
        &["mcp".into(), "--dev-data".into()],
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
            r#"mcp_servers.monocode.env_vars=["MONOCODE_CONTROL_ENDPOINT","MONOCODE_CONTROL_SESSION","MONOCODE_CONTROL_TOKEN"]"#,
        ]
    );
    let token = &environment[ENV_TOKEN];
    assert_eq!(token.len(), 64);
    assert!(args.iter().all(|arg| !arg.contains(token.as_str())));
}

#[test]
fn acp_lists_the_relay_with_its_environment() {
    let environment = control(7);
    let servers = acp_servers(&environment);
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
            ]
        }])
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
    let acp = acp_servers(&environment);
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
    assert_eq!(acp_servers(&empty), json!([]));
    let mut partial = control(7);
    partial.remove(ENV_TOKEN);
    assert_eq!(acp_servers(&partial), json!([]), "no credential, no relay");
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
