//! Unix: a socket file in the data folder's `control/` subfolder. Only that
//! subfolder is made 0700, so other users cannot reach the socket at all; the
//! data folder keeps its permissions (the desktop app may share it). The
//! socket itself is 0600.
use super::BindError;
use crate::Endpoint;
use std::{
    fs::{self, DirBuilder, Permissions},
    io,
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
};

pub(super) type Stream = UnixStream;

pub(super) struct Listener {
    socket: UnixListener,
    path: PathBuf,
    /// The socket file's device and inode, so dropping never removes a
    /// socket another app bound after this one.
    identity: (u64, u64),
}

impl Listener {
    pub(super) fn bind(endpoint: &Endpoint) -> Result<Self, BindError> {
        let path = endpoint.path().to_path_buf();
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            private_folder(dir)?;
        }
        match fs::symlink_metadata(&path) {
            Ok(meta) if !meta.file_type().is_socket() => {
                return Err(BindError::Io(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} exists and is not a socket", path.display()),
                )));
            }
            Ok(_) => {
                if UnixStream::connect(&path).is_ok() {
                    return Err(BindError::InUse(endpoint.describe()));
                }
                // Nothing answers: a crashed app left it behind.
                fs::remove_file(&path)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, Permissions::from_mode(0o600))?;
        let meta = fs::symlink_metadata(&path)?;
        Ok(Self {
            socket: listener,
            path,
            identity: (meta.dev(), meta.ino()),
        })
    }
    pub(super) fn accept(&self) -> io::Result<Stream> {
        self.socket.accept().map(|(stream, _)| stream)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok_and(|m| (m.dev(), m.ino()) == self.identity) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Makes `dir` an owner-only folder. Its parent, the data folder, is created
/// with the usual permissions when missing and is never changed.
fn private_folder(dir: &Path) -> io::Result<()> {
    if let Some(data) = dir.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(data)?;
    }
    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    if !fs::symlink_metadata(dir)?.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists and is not a folder", dir.display()),
        ));
    }
    fs::set_permissions(dir, Permissions::from_mode(0o700))
}

pub(super) fn connect(endpoint: &Endpoint) -> io::Result<Stream> {
    UnixStream::connect(endpoint.path())
}

pub(super) fn nobody_listening(_: &io::Error) -> bool {
    false
}
