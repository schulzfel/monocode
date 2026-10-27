//! Where the app listens for one data folder: a socket file in the folder's
//! own `control/` subfolder on macOS and Linux, a named pipe derived from the
//! folder on Windows. The data folder itself may be shared with the desktop
//! (Tauri) app, so only the subfolder is made private. A data folder too deep
//! for a socket path gets a private folder named after it instead, in the
//! user's runtime folder on Linux or else under the temporary folder, so the
//! app and its relays still agree.
use crate::ControlError;
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

/// The subfolder of the data folder that holds the socket. Only this folder's
/// permissions are changed (to 0700), never the data folder's.
pub const SOCKET_DIR: &str = "control";
/// The socket file's name inside [`SOCKET_DIR`].
pub const SOCKET_FILE: &str = "monocode-control.sock";

/// The longest socket path the platform accepts, in bytes, without the
/// terminating NUL of `sockaddr_un.sun_path`.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub const MAX_SOCKET_PATH: usize = 107;
/// The longest socket path the platform accepts, in bytes, without the
/// terminating NUL of `sockaddr_un.sun_path`.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub const MAX_SOCKET_PATH: usize = 103;

/// The control endpoint of one data folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    #[cfg(unix)]
    path: PathBuf,
    #[cfg(windows)]
    name: String,
}

impl Endpoint {
    /// The endpoint of the data folder `dir`, the folder holding the
    /// database the app opened. On macOS and Linux, when the socket path in
    /// `dir` is longer than the platform allows, the socket lives in
    /// a private folder named after `dir` instead (`fallback_dir`): on
    /// Linux in the user's runtime folder (`XDG_RUNTIME_DIR`) when the
    /// session has one, else under the temporary folder (`TMPDIR`, else
    /// `/tmp`; `fallback_root`).
    ///
    /// # Errors
    /// `invalid_input` when the fallback path is too long as well, or the
    /// fallback folder exists but is not a private folder of the data
    /// folder's owner.
    pub fn for_data_dir(dir: &Path) -> Result<Self, ControlError> {
        // Only Linux has a runtime folder of the user's own to prefer.
        #[cfg(unix)]
        let temp = fallback_root(
            std::env::var_os("XDG_RUNTIME_DIR")
                .filter(|_| cfg!(target_os = "linux"))
                .map(PathBuf::from)
                .as_deref(),
            &std::env::temp_dir(),
        );
        #[cfg(windows)]
        let temp = std::env::temp_dir();
        Self::for_data_dir_in(dir, &temp)
    }
    /// [`for_data_dir`](Self::for_data_dir), with `temp` as the folder the
    /// fallback folder goes in. Named pipes have no length limit, so
    /// Windows ignores `temp`.
    ///
    /// # Errors
    /// As [`for_data_dir`](Self::for_data_dir).
    pub fn for_data_dir_in(dir: &Path, temp: &Path) -> Result<Self, ControlError> {
        #[cfg(unix)]
        return Ok(Self {
            path: fallback::socket_path_in(dir, temp)?,
        });
        #[cfg(windows)]
        {
            let _ = temp;
            Ok(Self {
                name: pipe_name(dir),
            })
        }
    }
    /// The socket file.
    #[cfg(unix)]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// The pipe's name, without the `\\.\pipe\` prefix.
    #[cfg(windows)]
    #[must_use]
    pub fn pipe(&self) -> &str {
        &self.name
    }
    /// The endpoint [`describe`](Self::describe) wrote, as a session's relay
    /// reads it from its environment.
    ///
    /// # Errors
    /// `invalid_input` when `text` is no endpoint of this platform.
    pub fn parse(text: &str) -> Result<Self, ControlError> {
        #[cfg(unix)]
        {
            let path = PathBuf::from(text);
            if !path.is_absolute() || text.len() > MAX_SOCKET_PATH {
                return Err(ControlError::invalid_input(format!(
                    "{text} is not a control socket path"
                )));
            }
            Ok(Self { path })
        }
        #[cfg(windows)]
        {
            let name = text.strip_prefix(r"\\.\pipe\").unwrap_or(text);
            if !name.starts_with("monocode-control-") {
                return Err(ControlError::invalid_input(format!(
                    "{text} is not a control pipe"
                )));
            }
            Ok(Self { name: name.into() })
        }
    }
    /// The endpoint for messages.
    #[must_use]
    pub fn describe(&self) -> String {
        #[cfg(unix)]
        return self.path.display().to_string();
        #[cfg(windows)]
        return format!(r"\\.\pipe\{}", self.name);
    }
}

/// The socket path of the data folder `dir`:
/// `<dir>/control/monocode-control.sock`.
///
/// # Errors
/// `invalid_input` when the path is longer than [`MAX_SOCKET_PATH`] bytes.
pub fn socket_path(dir: &Path) -> Result<PathBuf, ControlError> {
    let path = dir.join(SOCKET_DIR).join(SOCKET_FILE);
    let length = path.as_os_str().len();
    if length > MAX_SOCKET_PATH {
        return Err(ControlError::invalid_input(format!(
            "The control socket path {} is {length} bytes; this platform allows at most {MAX_SOCKET_PATH}. Use a shorter data folder (MONOCODE_NATIVE_DATA_DIR).",
            path.display()
        )));
    }
    Ok(path)
}

/// The named pipe of the data folder `dir`: `monocode-control-` and 16 hex
/// digits of a SHA-256 of the folder, compared case-insensitively as
/// Windows compares paths.
#[must_use]
pub fn pipe_name(dir: &Path) -> String {
    let folder = dir.to_string_lossy();
    let folder = folder.trim_end_matches(['/', '\\']).to_lowercase();
    let mut name = String::from("monocode-control-");
    push_digest(&mut name, folder.as_bytes());
    name
}

/// Appends 16 hex digits of a SHA-256 of `bytes` to `name`.
fn push_digest(name: &mut String, bytes: &[u8]) {
    let digest = Sha256::digest(bytes);
    for byte in &digest[..8] {
        let _ = write!(name, "{byte:02x}");
    }
}

/// The folder the fallback folder ([`fallback_dir`]) goes in: the user's
/// own runtime folder `runtime` (`$XDG_RUNTIME_DIR`, which only Linux
/// passes) when it is an absolute path, since no other user may create
/// anything there; else the temporary folder `temp` (`$TMPDIR`, else
/// `/tmp`).
#[cfg(unix)]
#[must_use]
pub fn fallback_root(runtime: Option<&Path>, temp: &Path) -> PathBuf {
    runtime
        .filter(|folder| folder.is_absolute())
        .unwrap_or(temp)
        .to_path_buf()
}

/// The folder in `temp` ([`fallback_root`]) that holds the socket of the
/// data folder `dir` when `<dir>/control/` is too deep: `monocode-` and 16
/// hex digits of a SHA-256 of the folder's path. The app makes it `0700`
/// when it binds.
#[cfg(unix)]
#[must_use]
pub fn fallback_dir(dir: &Path, temp: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    let bytes = dir.as_os_str().as_bytes();
    let trimmed = match bytes.iter().rposition(|b| *b != b'/') {
        Some(last) => &bytes[..=last],
        None => bytes,
    };
    let mut name = String::from("monocode-");
    push_digest(&mut name, trimmed);
    temp.join(name)
}

#[cfg(unix)]
mod fallback;
