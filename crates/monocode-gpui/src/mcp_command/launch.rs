//! How `app_launch` starts the app that owns this relay: on macOS the
//! enclosing `.app` bundle through LaunchServices (`open -a`), so the bundle
//! reads its own Info.plist data mode as when the person opens it; otherwise
//! this executable, detached, with the relay's own data flags.
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// The `.app` bundle `exe` runs from: `<X>.app/Contents/MacOS/<exe>`.
pub(super) fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let named = |dir: &Path, name: &str| dir.file_name().is_some_and(|n| n == name);
    (named(macos, "MacOS")
        && named(contents, "Contents")
        && bundle.extension().is_some_and(|e| e == "app"))
    .then(|| bundle.to_path_buf())
}

/// The command that starts the app owning `exe` with `data_args`, the data
/// flags the relay itself was given.
pub(super) fn command(exe: &Path, data_args: &[String]) -> Command {
    if cfg!(target_os = "macos")
        && let Some(bundle) = bundle_of(exe)
    {
        let mut open = Command::new("/usr/bin/open");
        open.arg("-a").arg(bundle);
        // LaunchServices does not pass the relay's environment on.
        if let Some(dir) = std::env::var_os("MONOCODE_NATIVE_DATA_DIR") {
            let mut pair = std::ffi::OsString::from("MONOCODE_NATIVE_DATA_DIR=");
            pair.push(dir);
            open.arg("--env").arg(pair);
        }
        if !data_args.is_empty() {
            open.arg("--args").args(data_args);
        }
        return open;
    }
    let mut direct = Command::new(exe);
    direct.args(data_args);
    direct
}

/// Starts the app owning this executable and returns once it is on its way.
pub(super) fn spawn(data_args: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = command(&exe, data_args);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        // Its own process group: the MCP client's signals do not reach it.
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    if bundle_of(&exe).is_some() && cfg!(target_os = "macos") {
        // `open` returns once LaunchServices has the app starting.
        let status = child.wait().map_err(|e| e.to_string())?;
        return if status.success() {
            Ok(())
        } else {
            Err(format!("open exited with {status}"))
        };
    }
    // Reap the app when it quits, so it never lingers as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
