//! External control of the running desktop app.
//!
//! This crate holds the one typed command set that an outside caller (the
//! `monocode-gpui mcp` relay today) uses to see and drive the app: the
//! [`Operation`] trait and the [`Command`] registry, their JSON Schemas, the
//! caller and scope model, the change [`Topic`]s a client can follow, the
//! newline-delimited JSON wire frames, and a local-socket transport with a
//! blocking client. It has no UI and no store.
//!
//! Adding an operation is a one-line change to the registry in
//! [`command`](crate::Command) plus its input and output types in a domain
//! module under `ops`.

mod caller;
pub mod caption;
mod client;
mod command;
mod endpoint;
mod error;
pub mod frame;
mod operation;
pub mod ops;
mod policy;
mod schema;
mod scope;
mod topic;
pub mod transport;

pub use caller::{
    SESSION_CALLER_PREFIX, SessionCredential, reserved_client_name, session_caller_name,
};
pub use caption::{APP_LAUNCH, APP_LAUNCH_TITLE, Caption, SET_FIELDS, caption};
pub use client::{Client, Notice, NoticeSink};
pub use command::{Command, Registered, catalog, hints_of, summarize};
pub use endpoint::{Endpoint, MAX_SOCKET_PATH, SOCKET_DIR, SOCKET_FILE, pipe_name, socket_path};
pub use error::{ControlError, ErrorCode};
pub use frame::{ClientInfo, Grant};
pub use operation::{CONFIRM_TIMEOUT, Descriptor, Hints, Operation, REQUEST_ID, valid_tool_name};
pub use policy::{
    ACTIVITY_KEY, ApprovedClient, POLICY_KEY, Policy, RECEIPT_PREFIX, valid_client_name,
};
pub use scope::{AccessMode, Scope};
pub use topic::{MAX_TOPICS, Topic};

/// The control protocol's major version. A peer speaking another major
/// version is refused with a typed error, so an old relay never misdrives a
/// new app.
pub const PROTOCOL_VERSION: u32 = 1;

/// Locks `mutex`, carrying on with its data when a thread panicked while
/// holding it: the values kept behind one here stay whole between writes.
pub fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
