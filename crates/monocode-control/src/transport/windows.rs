//! Windows: a named pipe, `\\.\pipe\monocode-control-<hash of the data
//! folder>`, through `interprocess`.
//!
//! - Access: the pipe's DACL is protected and has one entry, full access for
//!   the pipe's owner (`OW`, the creating user), so other users cannot open it.
//! - Remote clients: `interprocess` creates pipes with
//!   `PIPE_REJECT_REMOTE_CLIENTS` unless asked otherwise.
//! - Squatting: the first instance is created with
//!   `FILE_FLAG_FIRST_PIPE_INSTANCE`, so the app never joins a pipe someone
//!   else created; it reports [`BindError::InUse`] instead.
//! - Stale pipes cannot exist: a pipe disappears with its last handle. A
//!   relay still holding a connection to an app that quit keeps the name
//!   alive, so a restarted app cannot listen until that relay reconnects.
//! - Timeouts: named pipes have no read or write timeout, so these are no-ops;
//!   the app answers every call within its own deadlines.
use super::BindError;
use crate::Endpoint;
use interprocess::{
    local_socket::{GenericNamespaced, ListenerOptions, prelude::*},
    os::windows::{local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor},
};
use std::io;
use widestring::U16CString;

pub(super) type Stream = interprocess::local_socket::Stream;

/// Protected DACL: generic all for the owner (the creating user) only.
const OWNER_ONLY: &str = "D:P(A;;GA;;;OW)";

pub(super) struct Listener {
    socket: interprocess::local_socket::Listener,
}

impl Listener {
    pub(super) fn bind(endpoint: &Endpoint) -> Result<Self, BindError> {
        let sddl = U16CString::from_str(OWNER_ONLY)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        let descriptor = SecurityDescriptor::deserialize(&sddl)?;
        let name = endpoint.pipe().to_ns_name::<GenericNamespaced>()?;
        match ListenerOptions::new()
            .name(name)
            .security_descriptor(descriptor)
            .create_sync()
        {
            Ok(socket) => Ok(Self { socket }),
            Err(_) if connect(endpoint).is_ok() => Err(BindError::InUse(endpoint.describe())),
            Err(error) => Err(error.into()),
        }
    }
    pub(super) fn accept(&self) -> io::Result<Stream> {
        self.socket.accept()
    }
}

pub(super) fn connect(endpoint: &Endpoint) -> io::Result<Stream> {
    Stream::connect(endpoint.pipe().to_ns_name::<GenericNamespaced>()?)
}

/// `ERROR_PIPE_BUSY` (231) with every instance taken still means the app is
/// there; `ERROR_FILE_NOT_FOUND` (2) arrives as `NotFound` and is handled by
/// the caller.
pub(super) fn nobody_listening(error: &io::Error) -> bool {
    error.raw_os_error() == Some(2)
}
