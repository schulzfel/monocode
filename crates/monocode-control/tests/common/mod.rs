//! Helpers the integration tests share. Each test file includes this as
//! `#[cfg(test)] pub mod common;`, so test-only lints apply and the helpers
//! a file leaves unused are not dead code.
use monocode_control::{ClientInfo, socket_path};
use std::path::{Path, PathBuf};

pub mod samples;

/// An empty folder `name` under the checkout's `target/c/`, short enough
/// that `<folder>/control/monocode-control.sock` fits `sun_path`.
///
/// # Panics
/// When the folder cannot be made, or its socket path would not fit.
#[must_use]
pub fn scratch(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("target/c");
    std::fs::create_dir_all(&root).unwrap();
    let dir = root.canonicalize().unwrap().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let socket = socket_path(&dir);
    assert!(
        cfg!(windows) || socket.is_ok(),
        "the test socket path must fit sun_path: {socket:?}"
    );
    dir
}

/// A client called `name`, version 1.
#[must_use]
pub fn info(name: &str) -> ClientInfo {
    ClientInfo {
        name: name.into(),
        version: "1".into(),
    }
}
