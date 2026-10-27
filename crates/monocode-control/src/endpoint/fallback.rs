//! The socket path of a data folder on macOS and Linux: in the folder's
//! `control/` subfolder when that fits `sun_path`, else in a private folder
//! in the user's runtime folder (Linux's `XDG_RUNTIME_DIR`) or under the
//! temporary folder. The temporary folder may be shared by every user
//! (`/tmp`), so an existing fallback folder is used only when it is a real
//! folder, owned by whoever owns the data folder, that admits nobody else;
//! the app creates it `0700` when it binds.
use super::{MAX_SOCKET_PATH, SOCKET_FILE, fallback_dir, socket_path};
use crate::ControlError;
use std::{
    fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// The socket path of the data folder `dir`, with `temp` as the folder the
/// fallback folder goes in.
pub(super) fn socket_path_in(dir: &Path, temp: &Path) -> Result<PathBuf, ControlError> {
    let too_long = match socket_path(dir) {
        Ok(path) => return Ok(path),
        Err(error) => error,
    };
    let folder = fallback_dir(dir, temp);
    let path = folder.join(SOCKET_FILE);
    if path.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(too_long);
    }
    check(dir, &folder)?;
    Ok(path)
}

/// Refuses an existing fallback `folder` that is no private folder of the
/// owner of `dir`. A missing one is fine: binding creates it.
fn check(dir: &Path, folder: &Path) -> Result<(), ControlError> {
    let found = match fs::symlink_metadata(folder) {
        Ok(found) => found,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(refused(folder, &format!("cannot be read ({error})"))),
    };
    if !found.file_type().is_dir() {
        return Err(refused(folder, "is not a folder"));
    }
    let owner = fs::metadata(dir).map(|data| data.uid()).map_err(|error| {
        refused(
            folder,
            &format!("cannot be checked against {} ({error})", dir.display()),
        )
    })?;
    if found.uid() != owner {
        return Err(refused(folder, "belongs to another user"));
    }
    let mode = found.permissions().mode() & 0o777;
    if mode != 0o700 {
        return Err(refused(
            folder,
            &format!("has mode {mode:o}, not 700, so other users may reach it"),
        ));
    }
    Ok(())
}

fn refused(folder: &Path, why: &str) -> ControlError {
    ControlError::invalid_input(format!(
        "The control socket folder {} {why}. Remove it, or use a shorter data folder (MONOCODE_NATIVE_DATA_DIR).",
        folder.display()
    ))
}
