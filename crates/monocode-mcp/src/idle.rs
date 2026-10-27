//! Letting go of an unused control connection. The app serves a bounded
//! number of connections, and an agent session's relay lives as long as its
//! provider process, so a relay that follows no topics hangs up once no call
//! has run for a while ([`RELEASE_AFTER`]). The client hangs up before it
//! sends anything, so the next call simply connects again; a change never
//! meets a connection the app already closed (no unknown outcome).
use monocode_control::lock;
use std::{
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};

/// How long a relay keeps a connection that no call uses.
pub(crate) const RELEASE_AFTER: Duration = Duration::from_secs(60);

/// The calls running on one relay's connection, and since when none has.
#[derive(Debug, Default)]
struct State {
    running: usize,
    /// When the last call ended, while none runs.
    since: Option<Instant>,
    /// Whether a timer waits to let the connection go.
    timing: bool,
}

impl State {
    fn begin(&mut self) {
        self.running += 1;
        self.since = None;
    }
    /// Returns whether a timer must start: nothing runs and none waits.
    fn end(&mut self, now: Instant) -> bool {
        self.running = self.running.saturating_sub(1);
        if self.running > 0 {
            return false;
        }
        self.since = Some(now);
        !std::mem::replace(&mut self.timing, true)
    }
    /// When the connection may go, if no call runs.
    fn due(&self, after: Duration) -> Option<Instant> {
        self.since
            .filter(|_| self.running == 0)
            .map(|since| since + after)
    }
}

/// When one relay's connection has gone unused long enough.
pub(crate) struct Idle {
    after: Duration,
    state: Mutex<State>,
    wake: Condvar,
}

impl Idle {
    pub(crate) fn new(after: Duration) -> Self {
        Self {
            after,
            state: Mutex::default(),
            wake: Condvar::new(),
        }
    }
    /// A call starts; nothing is let go while it runs.
    pub(crate) fn begin(&self) {
        lock(&self.state).begin();
        self.wake.notify_all();
    }
    /// A call ended. Returns whether the caller must start a timer thread
    /// that runs [`wait_then`](Self::wait_then).
    pub(crate) fn end(&self) -> bool {
        lock(&self.state).end(Instant::now())
    }
    /// A timer thread could not start; the next call's end tries again.
    pub(crate) fn untimed(&self) {
        lock(&self.state).timing = false;
    }
    /// Waits until no call has run for the release delay, then runs
    /// `release`, with calls held off until it returns. Returns early,
    /// without releasing, when a call starts meanwhile: its end starts the
    /// next timer.
    pub(crate) fn wait_then(&self, release: impl FnOnce()) {
        let mut state = lock(&self.state);
        loop {
            let Some(due) = state.due(self.after) else {
                state.timing = false;
                return;
            };
            let now = Instant::now();
            if now >= due {
                state.since = None;
                state.timing = false;
                release();
                return;
            }
            state = match self.wake.wait_timeout(state, due - now) {
                Ok((state, _)) => state,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_connection_goes_only_after_the_delay_without_any_call_running() {
        let after = RELEASE_AFTER;
        assert_eq!(after, Duration::from_secs(60));
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut state = State::default();
        assert_eq!(state.due(after), None, "nothing to let go before a call");
        state.begin();
        assert_eq!(state.due(after), None, "a call runs");
        assert!(state.end(at(1)), "the first idle moment starts a timer");
        assert_eq!(state.due(after), Some(at(61)));
        state.begin();
        state.begin();
        assert_eq!(state.due(after), None, "a new call holds it");
        assert!(!state.end(at(5)), "another call still runs");
        assert_eq!(state.due(after), None);
        assert!(!state.end(at(9)), "the timer is still waiting");
        assert_eq!(state.due(after), Some(at(69)), "counted from the last call");
    }

    #[test]
    fn a_timer_releases_once_and_a_call_cancels_it() {
        let idle = Idle::new(Duration::ZERO);
        idle.begin();
        assert!(idle.end());
        let mut released = 0;
        idle.wait_then(|| released += 1);
        assert_eq!(released, 1);
        idle.wait_then(|| released += 1);
        assert_eq!(released, 1, "nothing ran since: nothing to let go");
        idle.begin();
        idle.wait_then(|| released += 1);
        assert_eq!(released, 1, "a running call is never cut off");
        assert!(idle.end(), "its end starts the next timer");
    }
}
