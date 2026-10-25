//! Unix socket files: permissions, stale sockets, a live owner, cleanup.
#![cfg(unix)]
#[cfg(test)]
pub mod common;

use monocode_control::{
    Endpoint, MAX_SOCKET_PATH,
    transport::{BindError, Connection, Listener},
};
use std::{
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::Path,
};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    pub(super) fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }
}
use common::scratch;
use support::mode;

#[test]
fn the_socket_is_private_and_removed_when_the_listener_goes() {
    let dir = scratch("u1");
    let endpoint = Endpoint::for_data_dir(&dir).unwrap();
    assert!(endpoint.path().as_os_str().len() <= MAX_SOCKET_PATH);
    assert_eq!(endpoint.path(), dir.join("control/monocode-control.sock"));
    let listener = Listener::bind(&endpoint).unwrap();
    assert_eq!(
        mode(&dir.join("control")),
        0o700,
        "the control folder admits only this user"
    );
    assert_eq!(mode(endpoint.path()), 0o600);
    assert!(Connection::connect(&endpoint).is_ok());
    drop(listener);
    assert!(
        !endpoint.path().exists(),
        "dropping the listener removes the socket"
    );
    assert!(Connection::connect(&endpoint).is_err());
}

#[test]
fn the_data_folder_keeps_its_permissions_and_only_the_control_folder_is_tightened() {
    let dir = scratch("u2");
    let control = dir.join("control");
    std::fs::create_dir_all(&control).unwrap();
    for folder in [&dir, &control] {
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let endpoint = Endpoint::for_data_dir(&dir).unwrap();
    let _listener = Listener::bind(&endpoint).unwrap();
    assert_eq!(mode(&dir), 0o755, "the shared data folder is left alone");
    assert_eq!(mode(&control), 0o700);
}

#[test]
fn a_missing_data_folder_is_created_with_default_permissions() {
    let dir = scratch("u6");
    let reference = scratch("u6r");
    std::fs::create_dir(&reference).unwrap();
    let endpoint = Endpoint::for_data_dir(&dir).unwrap();
    let _listener = Listener::bind(&endpoint).unwrap();
    assert_eq!(
        mode(&dir),
        mode(&reference),
        "a folder the app creates for its data gets the usual permissions"
    );
    assert_eq!(mode(&dir.join("control")), 0o700);
}

#[test]
fn a_control_path_that_is_not_a_folder_is_refused() {
    let dir = scratch("u7");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("control"), "not a folder").unwrap();
    let endpoint = Endpoint::for_data_dir(&dir).unwrap();
    assert!(matches!(Listener::bind(&endpoint), Err(BindError::Io(_))));
    assert_eq!(
        std::fs::read_to_string(dir.join("control")).unwrap(),
        "not a folder"
    );
}

#[test]
fn a_live_owner_is_left_alone_and_a_stale_socket_is_replaced() {
    let dir = scratch("u3");
    let endpoint = Endpoint::for_data_dir(&dir).unwrap();
    let first = Listener::bind(&endpoint).unwrap();
    match Listener::bind(&endpoint) {
        Err(BindError::InUse(at)) => assert_eq!(at, endpoint.describe()),
        other => unreachable!("a second app must not take over: {:?}", other.err()),
    }
    assert!(
        Connection::connect(&endpoint).is_ok(),
        "the first app still answers"
    );
    drop(first);
    // A crashed app leaves its socket file behind with nobody answering.
    drop(UnixListener::bind(endpoint.path()).unwrap());
    assert!(endpoint.path().exists());
    let second = Listener::bind(&endpoint).unwrap();
    assert!(Connection::connect(&endpoint).is_ok());
    drop(second);
}

#[test]
fn a_file_that_is_not_a_socket_is_never_deleted() {
    let dir = scratch("u4");
    std::fs::create_dir_all(dir.join("control")).unwrap();
    let endpoint = Endpoint::for_data_dir(&dir).unwrap();
    std::fs::write(endpoint.path(), "keep me").unwrap();
    assert!(matches!(Listener::bind(&endpoint), Err(BindError::Io(_))));
    assert_eq!(std::fs::read_to_string(endpoint.path()).unwrap(), "keep me");
}

#[test]
fn a_dropped_listener_never_removes_a_successor_socket() {
    let dir = scratch("u5");
    let endpoint = Endpoint::for_data_dir(&dir).unwrap();
    let first = Listener::bind(&endpoint).unwrap();
    // Something else replaced the socket file while the first listener ran;
    // binding beside it and renaming over it guarantees a different inode.
    let beside = dir.join("control/next.sock");
    let successor = UnixListener::bind(&beside).unwrap();
    std::fs::rename(&beside, endpoint.path()).unwrap();
    drop(first);
    assert!(endpoint.path().exists(), "the successor's socket stays");
    drop(successor);
}

#[test]
fn a_pair_carries_frames_without_an_endpoint() {
    use monocode_control::frame;
    use std::io::BufReader;
    let (a, b) = Connection::pair().unwrap();
    frame::write_frame(&mut &a, &serde_json::json!({"n": 1}), 64).unwrap();
    let value: serde_json::Value = frame::read_frame(&mut BufReader::new(&b), 64)
        .unwrap()
        .unwrap();
    assert_eq!(value["n"], 1);
}

#[test]
fn an_endpoint_reads_back_from_its_description() {
    let endpoint = Endpoint::for_data_dir(Path::new("/data/monocode")).unwrap();
    assert_eq!(Endpoint::parse(&endpoint.describe()).unwrap(), endpoint);
    assert!(Endpoint::parse("relative/monocode-control.sock").is_err());
    let long = format!("/{}", "x".repeat(MAX_SOCKET_PATH));
    assert!(Endpoint::parse(&long).is_err());
}
