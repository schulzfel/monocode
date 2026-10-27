//! The hook's socket: an accept thread and one thread per connection, each
//! reading request lines and writing one answer line per request. Requests
//! reach the window through a channel the UI thread drains.
use super::request::Request;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

/// How long the window may take to answer one request; a capture waits for
/// a frame, a submit for the composer.
const ANSWER_WAIT: Duration = Duration::from_secs(60);

/// One request and where its answer goes.
pub(crate) struct Call {
    pub(crate) request: Request,
    reply: mpsc::SyncSender<Value>,
}

impl Call {
    /// Answers the request; a connection that went away is not an error.
    pub(crate) fn answer(self, answer: Value) {
        let _ = self.reply.send(answer);
    }
}

/// Listens on `path`, replacing a stale socket but never another file, and
/// sends each well-formed request to `calls`. `data` is the data folder a
/// capture must stay in.
pub(crate) fn listen(
    path: &Path,
    data: PathBuf,
    calls: async_channel::Sender<Call>,
) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(found) if found.file_type().is_socket() => {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        Ok(_) => return Err(format!("{} exists and is not a socket", path.display())),
        Err(_) => {}
    }
    let listener = UnixListener::bind(path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    std::thread::Builder::new()
        .name("qa-hook".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let calls = calls.clone();
                let data = data.clone();
                let _ = std::thread::Builder::new()
                    .name("qa-hook-connection".into())
                    .spawn(move || serve(stream, &data, &calls));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn serve(stream: UnixStream, data: &Path, calls: &async_channel::Sender<Call>) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else {
            return;
        };
        if line.trim().is_empty() {
            continue;
        }
        let answer = match Request::parse(&line, data) {
            Ok(request) => {
                let (reply, answer) = mpsc::sync_channel(1);
                if calls.send_blocking(Call { request, reply }).is_err() {
                    json!({"ok": false, "error": "the window is gone"})
                } else {
                    answer.recv_timeout(ANSWER_WAIT).unwrap_or_else(
                        |_| json!({"ok": false, "error": "the window did not answer in time"}),
                    )
                }
            }
            Err(error) => json!({"ok": false, "error": error}),
        };
        if writeln!(writer, "{answer}").is_err() {
            return;
        }
    }
}
