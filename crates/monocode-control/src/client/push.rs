//! Following topics: the client remembers what it follows, subscribes again
//! on every new connection, and a pump thread reads the connection while
//! nobody else does, so pushed events arrive between calls too.
use super::{Client, Delivery, Ended, Failure, Session, lock};
use crate::{
    ControlError, MAX_TOPICS, Topic,
    frame::{self, ClientFrame, Event, MAX_RESPONSE_BYTES, ServerFrame, Subscription},
};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, atomic::Ordering, mpsc},
};

/// The waiting slot the pump takes; calls are numbered from 1.
const PUMP: u64 = 0;

/// What the client tells its owner about followed topics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// A followed topic changed: read it again.
    Changed(Event),
    /// The connection ended. Topics are followed again when the client next
    /// connects, for a call or [`Client::resubscribe`].
    Lost,
    /// The client connected again and follows its topics anew; anything may
    /// have changed meanwhile.
    Resumed,
}

/// Where notices go. Called on the client's own threads; it must not block.
pub type NoticeSink = Arc<dyn Fn(Notice) + Send + Sync>;

/// Followed topics and their sink, shared by the client and its connections.
#[derive(Default)]
pub(super) struct Push {
    sink: Mutex<Option<NoticeSink>>,
    topics: Mutex<BTreeSet<Topic>>,
}

impl Push {
    fn deliver(&self, notice: Notice) {
        let sink = lock(&self.sink).clone();
        if let Some(sink) = sink {
            sink(notice);
        }
    }
}

fn unsupported() -> ControlError {
    ControlError::internal("This MonoCode does not push changes; update it to follow resources.")
}

impl Client {
    /// Sends notices about followed topics to `sink`.
    pub fn on_notice(&self, sink: NoticeSink) {
        *lock(&self.push.sink) = Some(sink);
    }
    /// The topics this client follows.
    #[must_use]
    pub fn topics(&self) -> Vec<Topic> {
        lock(&self.push.topics).iter().cloned().collect()
    }
    /// Follows `topics` as well, connecting if needed.
    ///
    /// # Errors
    /// The app's refusal (`scope_denied`, `not_found`, too many topics), or
    /// a connection error; then nothing new is followed.
    pub fn subscribe(&self, topics: &[Topic]) -> Result<(), ControlError> {
        let session = self.session()?;
        if !session.welcome.push {
            return Err(unsupported());
        }
        let wanted = lock(&self.push.topics).clone();
        let fresh: BTreeSet<Topic> = topics
            .iter()
            .filter(|t| !wanted.contains(t))
            .cloned()
            .collect();
        let subscribed = session.subscribed.load(Ordering::SeqCst);
        if fresh.is_empty() && subscribed {
            return Ok(());
        }
        if wanted.len() + fresh.len() > MAX_TOPICS {
            return Err(ControlError::invalid_input(format!(
                "A client follows at most {MAX_TOPICS} resources at once"
            )));
        }
        // A connection that lost its earlier topics gets them back too.
        let send: Vec<Topic> = if subscribed {
            fresh.iter().cloned().collect()
        } else {
            wanted.union(&fresh).cloned().collect()
        };
        let id = self.next_id();
        let frame = ClientFrame::Subscribe(Subscription { id, topics: send });
        self.request(id, &frame, true)?;
        lock(&self.push.topics).extend(fresh);
        // The request may have run on a newer connection than `session`.
        if let Ok(current) = self.session() {
            current.subscribed.store(true, Ordering::SeqCst);
            current.start_pump();
        }
        Ok(())
    }
    /// Stops following `topics`.
    ///
    /// # Errors
    /// A connection error; the topics are forgotten all the same.
    pub fn unsubscribe(&self, topics: &[Topic]) -> Result<(), ControlError> {
        let removed: Vec<Topic> = {
            let mut wanted = lock(&self.push.topics);
            topics
                .iter()
                .filter(|t| wanted.remove(t))
                .cloned()
                .collect()
        };
        let current = lock(&self.session).clone();
        let Some(session) = current.filter(|s| !s.ended()) else {
            return Ok(());
        };
        if removed.is_empty() || !session.subscribed.load(Ordering::SeqCst) {
            return Ok(());
        }
        let id = self.next_id();
        let frame = ClientFrame::Unsubscribe(Subscription {
            id,
            topics: removed,
        });
        self.exchange_on(&session, id, &frame, true)
            .map(|_| ())
            .map_err(|failure| match failure {
                Failure::Stale(error) | Failure::Final(error) => error,
            })
    }
    /// Connects if needed and follows the topics again if this connection
    /// does not yet. Ok when nothing is followed.
    ///
    /// # Errors
    /// `app_not_running`, or the app's refusal.
    pub fn resubscribe(&self) -> Result<(), ControlError> {
        if lock(&self.push.topics).is_empty() {
            return Ok(());
        }
        let session = self.session()?;
        if session.subscribed.load(Ordering::SeqCst) {
            return Ok(());
        }
        self.follow_again(&session)
    }
    /// After a new connection opens: follows the topics again, best effort.
    pub(super) fn resume(&self, session: &Arc<Session>) {
        if lock(&self.push.topics).is_empty() {
            return;
        }
        if self.follow_again(session).is_err() {
            self.push.deliver(Notice::Lost);
        }
    }
    fn follow_again(&self, session: &Arc<Session>) -> Result<(), ControlError> {
        if !session.welcome.push {
            return Err(unsupported());
        }
        let topics: Vec<Topic> = lock(&self.push.topics).iter().cloned().collect();
        let id = self.next_id();
        let frame = ClientFrame::Subscribe(Subscription { id, topics });
        self.exchange_on(session, id, &frame, true)
            .map_err(|failure| match failure {
                Failure::Stale(error) | Failure::Final(error) => error,
            })?;
        session.subscribed.store(true, Ordering::SeqCst);
        // Said before the pump reads, so it comes before the new events.
        self.push.deliver(Notice::Resumed);
        session.start_pump();
        Ok(())
    }
}

impl Session {
    /// Passes a notice on, unless the client let go of this connection.
    pub(super) fn notice(&self, notice: Notice) {
        if !self.closed.load(Ordering::SeqCst) {
            self.push.deliver(notice);
        }
    }
    /// Hangs up, waking the pump; it then ends without a notice.
    pub(super) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        if self.pumping.load(Ordering::SeqCst) {
            let _ = self.connection.shutdown();
        }
    }
    fn start_pump(self: &Arc<Self>) {
        if self.pumping.swap(true, Ordering::SeqCst) {
            return;
        }
        let session = Arc::clone(self);
        let started = std::thread::Builder::new()
            .name("control-events".into())
            .spawn(move || session.pump());
        if started.is_err() {
            self.pumping.store(false, Ordering::SeqCst);
        }
    }
    /// Waits like a call that never gets a reply: whenever it is its turn,
    /// it reads for everyone until the connection ends.
    fn pump(&self) {
        let (deliver, delivered) = mpsc::channel();
        let Ok(mut reading) = self.register(PUMP, deliver) else {
            return self.notice(Notice::Lost);
        };
        while !reading {
            match delivered.recv() {
                Ok(Delivery::Lead) => reading = true,
                Ok(Delivery::Reply(_)) => {}
                Ok(Delivery::Ended(_)) | Err(_) => return self.notice(Notice::Lost),
            }
        }
        self.read_all();
        self.notice(Notice::Lost);
    }
    /// Reads frames for everyone until the connection ends. A read timeout
    /// only means nothing happened.
    fn read_all(&self) {
        let mut reader = lock(&self.reader);
        loop {
            let ended = match reader.next_frame::<ServerFrame>(MAX_RESPONSE_BYTES) {
                Ok(Some(ServerFrame::Reply(reply))) => {
                    if let Some(deliver) = lock(&self.calls).waiting.remove(&reply.id) {
                        let _ = deliver.send(Delivery::Reply(reply.result));
                    }
                    None
                }
                Ok(Some(ServerFrame::Event(event))) => {
                    self.notice(Notice::Changed(event));
                    None
                }
                Ok(Some(ServerFrame::Error(error))) => Some(Ended::Failed(error)),
                Ok(Some(ServerFrame::Welcome(_))) => Some(Ended::Failed(ControlError::internal(
                    "MonoCode sent an unexpected welcome",
                ))),
                Ok(None) => Some(Ended::Closed),
                Err(_) if self.closed.load(Ordering::SeqCst) => Some(Ended::Closed),
                Err(error) if frame::timed_out(&error) => None,
                Err(error) => Some(Ended::Failed(error.into())),
            };
            if let Some(ended) = ended {
                drop(reader);
                self.end(&ended);
                return;
            }
        }
    }
}
