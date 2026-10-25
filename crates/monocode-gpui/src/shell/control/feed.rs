//! One connection's followed topics, as the listener holds them: its event
//! queue once it has one, the subscription requests it hands the shell, and
//! the clean-up when the connection ends.
use super::{
    caller::CallerKind,
    hub::Events,
    listener::{Request, WRITE_WAIT, busy},
};
use monocode_control::{ControlError, ErrorCode, frame::Subscription};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc,
};

/// Numbers connections, so the shell knows whose topics are whose.
static CONNECTIONS: AtomicU64 = AtomicU64::new(1);

/// One connection's followed topics: its event queue, once it has one.
/// Dropping it ends the event writer and tells the shell to forget them.
pub(super) struct Feed {
    connection: u64,
    requests: async_channel::Sender<Request>,
    /// Where the shell queues this connection's events.
    pub(super) events: Option<Events>,
}

impl Feed {
    pub(super) fn new(requests: async_channel::Sender<Request>) -> Self {
        Self {
            connection: CONNECTIONS.fetch_add(1, Ordering::Relaxed),
            requests,
            events: None,
        }
    }

    /// Asks the shell to follow (or stop following) `subscription`'s topics.
    pub(super) fn follow(
        &self,
        caller: &CallerKind,
        subscription: Subscription,
        on: bool,
    ) -> Result<Value, ControlError> {
        let (reply, answer) = mpsc::sync_channel(1);
        let topics = subscription.topics;
        let request = match (&self.events, on) {
            (Some(events), true) => Request::Subscribe {
                connection: self.connection,
                caller: caller.clone(),
                topics,
                events: events.clone(),
                reply,
            },
            (None, true) => return Err(busy()),
            // Nothing was ever followed here.
            (None, false) => return Ok(serde_json::json!({ "topics": [] })),
            (Some(_), false) => Request::Unsubscribe {
                connection: self.connection,
                topics,
                reply,
            },
        };
        if self.requests.send_blocking(request).is_err() {
            return Err(ControlError::control_disabled());
        }
        answer.recv_timeout(WRITE_WAIT).unwrap_or_else(|_| {
            Err(ControlError::new(
                ErrorCode::Timeout,
                "MonoCode did not answer the subscription in time",
            ))
        })
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        if let Some(events) = self.events.take() {
            // Blocks only until the writer drains or its write times out.
            let _ = events.send(super::hub::Push::End);
            let _ = self.requests.send_blocking(Request::Closed {
                connection: self.connection,
            });
        }
    }
}
