//! The relay's answer when no app listens, read from the store read-only.
use super::*;
use monocode_control::ErrorCode;
use monocode_core::store::Store;
use std::path::PathBuf;

/// A short folder under this crate's target directory, inside the checkout.
fn scratch(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/c");
    std::fs::create_dir_all(&root).unwrap();
    let dir = root.canonicalize().unwrap().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn store_with(dir: &Path, policy: Option<&str>) -> PathBuf {
    let database = dir.join("monocode.db");
    let store = Store::open_monocode(&database).unwrap();
    if let Some(policy) = policy {
        store.save_setting(POLICY_KEY, policy).unwrap();
    }
    database
}

#[test]
fn control_off_or_never_set_is_control_disabled() {
    let off = store_with(&scratch("p1"), Some(r#"{"enabled":false}"#));
    assert_eq!(unavailable(&off).code, ErrorCode::ControlDisabled);
    let unset = store_with(&scratch("p2"), None);
    assert_eq!(unavailable(&unset).code, ErrorCode::ControlDisabled);
}

#[test]
fn control_on_or_no_data_is_app_not_running() {
    let on = store_with(&scratch("p3"), Some(r#"{"enabled":true}"#));
    assert_eq!(unavailable(&on).code, ErrorCode::AppNotRunning);
    let missing = scratch("p4").join("monocode.db");
    assert_eq!(unavailable(&missing).code, ErrorCode::AppNotRunning);
    assert!(!missing.exists(), "the probe never creates a database");
}

#[test]
fn the_relay_and_the_window_resolve_the_same_database() {
    let mut location = crate::data_location::DataLocation::new();
    let mut rest = vec!["/x/desktop.db".to_owned()].into_iter();
    assert!(location.apply("--monocode-db", &mut rest));
    assert!(location.monocode());
    assert_eq!(location.database(), PathBuf::from("/x/desktop.db"));
    assert!(location.apply("--dev-data", &mut rest));
    assert_eq!(location.database(), crate::data_dir().join("monocode.db"));
    #[cfg(unix)]
    assert_eq!(
        endpoint(Path::new("/x/desktop.db")).unwrap().path(),
        Path::new("/x/control/monocode-control.sock"),
        "the relay looks in the control subfolder beside the database"
    );
    assert!(!location.apply("--client", &mut rest));
    assert_eq!(run(&["--bogus".into()]), 2);
    assert_eq!(run(&["--client".into()]), 2);
}

#[test]
fn the_launch_probe_is_on_only_when_the_stored_policy_says_so() {
    let on = store_with(&scratch("p5"), Some(r#"{"enabled":true}"#));
    assert_eq!(stored_enabled(&on), Some(true));
    let off = store_with(&scratch("p6"), Some(r#"{"enabled":false}"#));
    assert_eq!(stored_enabled(&off), Some(false));
    let missing = scratch("p7").join("monocode.db");
    assert_eq!(stored_enabled(&missing), None);
    assert!(!missing.exists(), "the probe never creates a database");
}

#[test]
fn a_bundle_is_opened_through_launch_services_and_a_bare_binary_runs_detached() {
    let exe = Path::new("/Applications/MonoCode.app/Contents/MacOS/monocode-gpui");
    let bundle = Path::new("/Applications/MonoCode.app");
    assert_eq!(launch::bundle_of(exe).as_deref(), Some(bundle));
    for bare in [
        "/work/target/native-dev/monocode-gpui",
        "/Applications/MonoCode/Contents/MacOS/monocode-gpui",
        "/x/MonoCode.app/Contents/Resources/monocode-gpui",
    ] {
        assert_eq!(launch::bundle_of(Path::new(bare)), None, "{bare}");
    }
    let flags = vec!["--monocode-db".to_owned(), "/x/desktop.db".to_owned()];
    let args = |command: &std::process::Command| -> Vec<String> {
        command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    };
    let opened = launch::command(exe, &flags);
    if cfg!(target_os = "macos") {
        assert_eq!(opened.get_program(), "/usr/bin/open");
        let opened = args(&opened);
        assert_eq!(opened[..2], ["-a", "/Applications/MonoCode.app"]);
        assert_eq!(
            opened[opened.len() - 3..],
            ["--args", "--monocode-db", "/x/desktop.db"]
        );
    }
    let bare = Path::new("/work/target/native-dev/monocode-gpui");
    let direct = launch::command(bare, &flags);
    assert_eq!(direct.get_program(), bare.as_os_str());
    assert_eq!(args(&direct), flags, "the same data flags as the relay");
}

#[test]
fn the_relay_speaks_for_the_session_its_environment_names() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        }
    };
    assert!(session::from_env(env(&[])).is_none(), "an external client");
    let full = env(&[
        (
            "MONOCODE_CONTROL_ENDPOINT",
            "/data/control/monocode-control.sock",
        ),
        ("MONOCODE_CONTROL_SESSION", "7"),
        ("MONOCODE_CONTROL_TOKEN", "c0ffee"),
    ]);
    let relay = session::from_env(full).unwrap().unwrap();
    assert_eq!(
        relay.endpoint.describe(),
        "/data/control/monocode-control.sock"
    );
    let credential = relay.credential;
    assert_eq!((credential.id, credential.token.as_str()), (7, "c0ffee"));
    assert_eq!(
        relay.role,
        monocode_mcp::Role::Session,
        "no role is a session"
    );
    let broken = env(&[
        (
            "MONOCODE_CONTROL_ENDPOINT",
            "/data/control/monocode-control.sock",
        ),
        ("MONOCODE_CONTROL_SESSION", "seven"),
        ("MONOCODE_CONTROL_TOKEN", "c0ffee"),
    ]);
    let error = session::from_env(broken).unwrap().unwrap_err();
    assert_eq!(
        error.code,
        ErrorCode::InvalidInput,
        "never an external hello"
    );
    let partial = env(&[("MONOCODE_CONTROL_TOKEN", "c0ffee")]);
    assert!(session::from_env(partial).unwrap().is_err());
}

/// `MONOCODE_CONTROL_ROLE=mono` makes a session relay list a Mono's own
/// tools; any other value is a session, and without a credential the role
/// names nobody: an external client stays one.
#[test]
fn the_relay_reads_its_role_with_its_credential_only() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        }
    };
    let role = |pairs| session::from_env(env(pairs)).unwrap().unwrap().role;
    let credential = [
        (
            "MONOCODE_CONTROL_ENDPOINT",
            "/data/control/monocode-control.sock",
        ),
        ("MONOCODE_CONTROL_SESSION", "7"),
        ("MONOCODE_CONTROL_TOKEN", "c0ffee"),
    ];
    let with = |value: &'static str| -> &'static [(&'static str, &'static str)] {
        let mut pairs = credential.to_vec();
        pairs.push(("MONOCODE_CONTROL_ROLE", value));
        pairs.leak()
    };
    assert_eq!(role(with("mono")), monocode_mcp::Role::Mono);
    assert_eq!(role(with("session")), monocode_mcp::Role::Session);
    assert_eq!(role(with("admin")), monocode_mcp::Role::Session);
    assert!(session::from_env(env(&[("MONOCODE_CONTROL_ROLE", "mono")])).is_none());
}

/// A data folder too deep for its socket path: the relay an MCP client
/// starts finds the same fallback socket that the window binds, in Linux's
/// `XDG_RUNTIME_DIR` or under `TMPDIR` (`monocode-control/tests/
/// socket_fallback.rs` pins the folder).
#[test]
fn the_relay_and_the_window_agree_on_the_socket_of_a_deep_data_folder() {
    let dir = scratch("p8").join("a-data-folder-too-deep-for-its-own-control-socket");
    std::fs::create_dir_all(&dir).unwrap();
    assert!(monocode_control::socket_path(&dir).is_err(), "too deep");
    let database = store_with(&dir, None);
    let store = Store::open_monocode(&database).unwrap();
    let window = store.data_dir().map(Endpoint::for_data_dir);
    assert_eq!(Some(endpoint(&database)), window, "what the window binds");
    let folder = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|_| cfg!(target_os = "linux"))
        .map(PathBuf::from)
        .filter(|folder| folder.is_absolute())
        .unwrap_or_else(std::env::temp_dir);
    assert_eq!(
        endpoint(&database),
        Endpoint::for_data_dir_in(&dir, &folder),
        "the fallback in the user's runtime folder or the temporary folder"
    );
}
