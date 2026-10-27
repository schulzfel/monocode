//! OpenCode's commands, run against a stand-in `opencode` script.
use super::*;
use crate::app_mcp;
use std::os::unix::fs::PermissionsExt;

/// A request for a session with the MonoCode control environment, whose
/// `opencode` is `script`.
fn request(dir: &Path, script: &str) -> Request {
    let binary = dir.join("opencode");
    std::fs::write(&binary, script).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let settings = Settings {
        environment: app_mcp::control_environment(
            7,
            "/data/control/monocode-control.sock",
            Path::new("/Applications/MonoCode.app/monocode-gpui"),
            &["mcp".into()],
            app_mcp::ROLE_SESSION,
        ),
        ..Settings::default()
    };
    Request {
        kind: Kind::OpenCode,
        binary,
        cwd: dir.into(),
        prompt: "Hello".into(),
        provider_id: None,
        model: None,
        settings,
    }
}

/// v2's background service is shared by every session, so its commands
/// never carry one session's credential: this `opencode` reports its URL
/// and password only to a command without one, and logs which control
/// variables each got (never their values).
#[test]
fn the_shared_service_never_gets_a_sessions_credential() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let script = format!(
        r#"#!/bin/sh
held=""
for key in TOKEN SESSION ENDPOINT ROLE COMMAND ARGS; do
  eval "[ -n \"\${{MONOCODE_CONTROL_$key+x}}\" ]" && held="$held $key"
done
echo "$* holds:${{held:- none}}" >> '{}'
[ -n "$MONOCODE_CONTROL_TOKEN$MONOCODE_CONTROL_SESSION$MONOCODE_CONTROL_ENDPOINT" ] && exit 0
case "$*" in
  "service get password") echo secret ;;
  *) echo http://127.0.0.1:9 ;;
esac
"#,
        log.display()
    );
    let request = request(dir.path(), &script);
    let connected = connect(&request, &AtomicBool::new(false), true);
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(connected.is_ok(), "{:?}\n{calls}", connected.err());
    let lines: Vec<&str> = calls.lines().collect();
    assert_eq!(
        lines,
        [
            "service status holds: none",
            "service get password holds: none"
        ],
        "{calls}"
    );
}

/// v1's `serve` belongs to one session, so it keeps that session's
/// environment.
#[test]
fn a_sessions_own_server_keeps_its_environment() {
    let dir = tempfile::tempdir().unwrap();
    let request = request(dir.path(), "#!/bin/sh\n");
    let command = command(&request).unwrap();
    let token = command
        .get_envs()
        .find(|(key, _)| *key == app_mcp::ENV_TOKEN)
        .and_then(|(_, value)| value);
    // Compared, never printed: it is a credential.
    assert!(
        token.is_some_and(|t| t.to_string_lossy() == app_mcp::token(7)),
        "the session's own credential"
    );
}
