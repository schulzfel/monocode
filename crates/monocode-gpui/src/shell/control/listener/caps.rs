//! How many connections the listener serves at once, by kind. A connection
//! takes a slot when it is accepted, while it says hello; its hello decides
//! its kind and moves it to that kind's slots: external clients, or the agent
//! sessions MonoCode runs (a hello with a session credential), so neither
//! crowds the other out. Over a cap the connection is told, retryably, that
//! MonoCode is busy. A slot is given back when its connection ends, however
//! it ends.
use monocode_control::{
    ControlError,
    frame::{MAX_APP_CONNECTIONS, MAX_CONNECTIONS, MAX_PENDING_HELLOS},
    lock,
};
use std::sync::{Arc, Mutex};

/// The most connections served at once, by kind; tests lower them.
#[derive(Clone, Copy, Debug)]
pub(in crate::shell::control) struct Caps {
    /// Accepted and still saying hello.
    pub(in crate::shell::control) hellos: usize,
    /// External clients, after hello.
    pub(in crate::shell::control) external: usize,
    /// MonoCode's own agent sessions, after hello.
    pub(in crate::shell::control) app: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            hellos: MAX_PENDING_HELLOS,
            external: MAX_CONNECTIONS,
            app: MAX_APP_CONNECTIONS,
        }
    }
}

/// What a connection over a cap is told.
pub(super) fn too_many() -> ControlError {
    ControlError::internal("MonoCode is serving too many control connections").retryable(true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Hello,
    External,
    App,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Served {
    hellos: usize,
    external: usize,
    app: usize,
}

impl Served {
    fn count(&mut self, kind: Kind) -> &mut usize {
        match kind {
            Kind::Hello => &mut self.hellos,
            Kind::External => &mut self.external,
            Kind::App => &mut self.app,
        }
    }
}

/// One listener's slots.
#[derive(Debug)]
pub(super) struct Slots {
    caps: Caps,
    served: Mutex<Served>,
}

impl Slots {
    pub(super) fn new(caps: Caps) -> Arc<Self> {
        Arc::new(Self {
            caps,
            served: Mutex::default(),
        })
    }
    /// A slot for a connection just accepted, while it says hello; `None`
    /// when too many are saying hello, or every slot of both kinds is taken.
    pub(super) fn accept(self: &Arc<Self>) -> Option<Slot> {
        let mut served = lock(&self.served);
        let open = served.hellos + served.external + served.app;
        if served.hellos >= self.caps.hellos || open >= self.caps.external + self.caps.app {
            return None;
        }
        served.hellos += 1;
        Some(Slot {
            slots: Arc::clone(self),
            kind: Kind::Hello,
        })
    }
}

/// One connection's slot, given back when it is dropped.
#[derive(Debug)]
pub(super) struct Slot {
    slots: Arc<Slots>,
    kind: Kind,
}

impl Slot {
    /// Moves a connection that said hello to the slots of its kind: an agent
    /// session's when `session`, else an external client's. When that kind
    /// is full the connection is refused, and its slot is given back.
    pub(super) fn settle(mut self, session: bool) -> Result<Self, ControlError> {
        let (kind, cap) = if session {
            (Kind::App, self.slots.caps.app)
        } else {
            (Kind::External, self.slots.caps.external)
        };
        let mut served = lock(&self.slots.served);
        if *served.count(kind) >= cap {
            drop(served);
            return Err(too_many());
        }
        *served.count(kind) += 1;
        *served.count(self.kind) -= 1;
        drop(served);
        self.kind = kind;
        Ok(self)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut served = lock(&self.slots.served);
        let count = served.count(self.kind);
        *count = count.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn served(slots: &Slots) -> (usize, usize, usize) {
        let served = lock(&slots.served);
        (served.hellos, served.external, served.app)
    }

    #[test]
    fn each_kind_has_its_own_cap_and_every_end_gives_its_slot_back() {
        let slots = Slots::new(Caps {
            hellos: 2,
            external: 1,
            app: 2,
        });
        let first = slots.accept().unwrap();
        let second = slots.accept().unwrap();
        assert!(slots.accept().is_none(), "two may say hello at once");
        let client = first.settle(false).unwrap();
        assert_eq!(served(&slots), (1, 1, 0));
        let refused = second.settle(false).unwrap_err();
        assert_eq!(refused, too_many());
        assert!(refused.retryable);
        assert_eq!(
            served(&slots),
            (0, 1, 0),
            "a refused hello gives its slot back"
        );
        let sessions: Vec<Slot> = (0..2)
            .map(|_| slots.accept().unwrap().settle(true).unwrap())
            .collect();
        assert_eq!(served(&slots), (0, 1, 2));
        assert!(
            slots.accept().is_none(),
            "every slot of both kinds is taken"
        );
        drop(client);
        let waiting = slots.accept().unwrap();
        assert!(
            waiting.settle(true).is_err(),
            "the sessions' slots are still full"
        );
        assert_eq!(served(&slots), (0, 0, 2));
        drop(sessions);
        assert_eq!(served(&slots), (0, 0, 0), "nothing is left counted");
        let defaults = Caps::default();
        assert_eq!(
            (defaults.hellos, defaults.external, defaults.app),
            (32, 16, 256)
        );
    }
}
