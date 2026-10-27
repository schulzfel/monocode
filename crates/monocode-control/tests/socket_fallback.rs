//! A data folder too deep for its socket path: the socket moves to a private
//! folder in the user's runtime folder on Linux, or else under the temporary
//! folder, named after the data folder, which the app and every relay
//! compute alike. The checkout's `target/c/` stands in for that folder.
#![cfg(unix)]
#[cfg(test)]
pub mod common;

use monocode_control::{
    Endpoint, ErrorCode, MAX_SOCKET_PATH, SOCKET_FILE, fallback_dir, fallback_root, socket_path,
    transport::{Connection, Listener},
};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::{Path, PathBuf},
};

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    /// The checkout's `target/c/`, the tests' temporary folder.
    pub(super) fn temp() -> PathBuf {
        common::scratch("x").parent().unwrap().to_path_buf()
    }

    /// A data folder under [`temp`] whose own socket path does not fit.
    pub(super) fn deep(name: &str) -> PathBuf {
        let dir = temp().join(format!("{name}-a-data-folder-too-deep-for-sun-path"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(socket_path(&dir).is_err(), "{dir:?} must not fit sun_path");
        dir
    }

    /// No fallback folder for `dir` yet.
    pub(super) fn clear(dir: &Path) -> PathBuf {
        let folder = fallback_dir(dir, &temp());
        let _ = std::fs::remove_file(&folder);
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    pub(super) fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    pub(super) fn set_mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// Why the endpoint of `dir` is refused.
    pub(super) fn refusal(dir: &Path) -> String {
        let error = Endpoint::for_data_dir_in(dir, &temp()).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{error}");
        error.message
    }
}
use support::{clear, deep, mode, refusal, set_mode, temp};

#[test]
fn a_deep_data_folder_gets_a_private_socket_folder_under_the_temporary_folder() {
    let dir = deep("f1");
    let folder = clear(&dir);
    let mut name = String::from("monocode-");
    for byte in &Sha256::digest(dir.as_os_str().as_bytes())[..8] {
        let _ = write!(name, "{byte:02x}");
    }
    assert_eq!(folder, temp().join(name), "named after the data folder");
    let endpoint = Endpoint::for_data_dir_in(&dir, &temp()).unwrap();
    assert_eq!(endpoint.path(), folder.join(SOCKET_FILE));
    assert!(endpoint.path().as_os_str().len() <= MAX_SOCKET_PATH);
    assert_eq!(
        Endpoint::parse(&endpoint.describe()).unwrap(),
        endpoint,
        "a session's relay reads it back from its environment"
    );
    let listener = Listener::bind(&endpoint).unwrap();
    assert_eq!(mode(&folder), 0o700, "only this user reaches the socket");
    assert_eq!(mode(endpoint.path()), 0o600);
    let relay = Endpoint::for_data_dir_in(&dir, &temp()).unwrap();
    assert_eq!(relay, endpoint, "a relay finds the folder the app made");
    assert!(Connection::connect(&relay).is_ok());
    drop(listener);
    assert!(
        !endpoint.path().exists(),
        "the socket goes with the listener"
    );
    let other = deep("f2");
    assert_ne!(fallback_dir(&other, &temp()), folder, "one per data folder");
    assert_eq!(
        fallback_dir(&dir.join(""), &temp()),
        folder,
        "a trailing slash names the same folder"
    );
    // Linux's runtime folder, when the session has one, else TMPDIR.
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|_| cfg!(target_os = "linux"))
        .map(PathBuf::from);
    assert_eq!(
        Endpoint::for_data_dir(&dir),
        Endpoint::for_data_dir_in(
            &dir,
            &fallback_root(runtime.as_deref(), &std::env::temp_dir())
        ),
        "without a folder given, the folder is the user's own or TMPDIR's"
    );
}

/// R19: the fallback folder goes in the user's own runtime folder when the
/// session has one (`$XDG_RUNTIME_DIR` on Linux), where no other user can
/// make it first; else in the temporary folder.
#[test]
fn the_fallback_folder_prefers_the_users_runtime_folder() {
    let temp = Path::new("/tmp");
    let runtime = Path::new("/run/user/501");
    assert_eq!(fallback_root(Some(runtime), temp), runtime);
    for unusable in ["", "run/user/501"] {
        assert_eq!(
            fallback_root(Some(Path::new(unusable)), temp),
            temp,
            "{unusable:?}"
        );
    }
    assert_eq!(fallback_root(None, temp), temp);
    assert_eq!(
        fallback_dir(&deep("f5"), runtime).parent(),
        Some(runtime),
        "named as anywhere else"
    );
}

#[test]
fn a_fallback_path_that_is_still_too_long_keeps_the_error() {
    let dir = deep("f3");
    let long = temp().join("t".repeat(MAX_SOCKET_PATH));
    let error = Endpoint::for_data_dir_in(&dir, &long).unwrap_err();
    assert_eq!(error, socket_path(&dir).unwrap_err(), "today's error");
    assert!(
        error.message.contains("MONOCODE_NATIVE_DATA_DIR"),
        "{error}"
    );
}

#[test]
fn an_existing_fallback_folder_must_be_private_and_the_data_folder_owners() {
    let dir = deep("f4");
    let folder = clear(&dir);
    std::fs::create_dir(&folder).unwrap();
    set_mode(&folder, 0o755);
    let open = refusal(&dir);
    assert!(open.contains("mode 755"), "{open}");
    set_mode(&folder, 0o700);
    assert!(Endpoint::for_data_dir_in(&dir, &temp()).is_ok());

    std::fs::remove_dir(&folder).unwrap();
    std::fs::write(&folder, "not a folder").unwrap();
    assert!(refusal(&dir).contains("is not a folder"));
    std::fs::remove_file(&folder).unwrap();
    let elsewhere = common::scratch("f4-elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    set_mode(&elsewhere, 0o700);
    std::os::unix::fs::symlink(&elsewhere, &folder).unwrap();
    let linked = refusal(&dir);
    assert!(
        linked.contains("is not a folder"),
        "a link is not: {linked}"
    );
    std::fs::remove_file(&folder).unwrap();

    // The root folder, spelled long, is another user's data folder.
    let theirs = PathBuf::from(format!("/{}", "./".repeat(MAX_SOCKET_PATH / 2)));
    assert!(socket_path(&theirs).is_err());
    let folder = clear(&theirs);
    std::fs::create_dir(&folder).unwrap();
    set_mode(&folder, 0o700);
    let foreign = refusal(&theirs);
    assert!(foreign.contains("belongs to another user"), "{foreign}");
    std::fs::remove_dir(&folder).unwrap();
}
