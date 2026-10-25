//! A blocking client, for a relay's `spawn_blocking` or a plain thread. It
//! connects on first use and reconnects after the app restarts.
//!
//! One connection carries many calls at once: each call registers its `id`,
//! writes its frame, and waits for the reply with that `id`. Whichever caller
//! is waiting reads the connection for everyone (one at a time); a reader
//! that gets its own reply hands reading to another waiting caller. So a long
//! `session_wait` never holds up a quick read, and no thread outlives a call
//! until the client follows topics: then one pump thread per connection
//! joins the waiting callers for good and reads pushed events (`push`).
use crate::{
    ClientInfo, Command, ControlError, Endpoint, ErrorCode, PROTOCOL_VERSION, Registered,
    SessionCredential,
    frame::{
        self, Call, ClientFrame, FrameError, Hello, Lines, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES,
        Outcome, ServerFrame, Welcome,
    },
    lock,
    transport::{Connection, nobody_listening},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::Read,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::{Duration, Instant},
};

mod push;
mod session;

pub use push::{Notice, NoticeSink};

/// How long the app may take to answer Hello: it may be asking the person.
const HELLO_TIMEOUT: Duration = Duration::from_secs(130);
/// How long the app may take to answer a call; it times calls out sooner.
const CALL_TIMEOUT: Duration = Duration::from_secs(100);
// The longest `session_wait`, plus a margin for the app's own work, fits.
const _: () = assert!(
    CALL_TIMEOUT.as_secs() >= crate::ops::dispatch::MAX_WAIT_MS as u64 / 1000 + 30,
    "a session_wait must finish before the client gives up"
);
/// A call the person confirms first may also wait for the prompt.
const CONFIRMED_CALL_TIMEOUT: Duration =
    Duration::from_secs(CALL_TIMEOUT.as_secs() + crate::CONFIRM_TIMEOUT.as_secs() + 10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// A change cut off mid-call: not retryable, since it may already have run.
const UNKNOWN_OUTCOME: &str = "MonoCode closed the control connection while the call was running, so it may or may not have taken effect. Check the current state (for example with session_list or session_status) before calling it again; a call with a requestId can be retried safely.";

/// One client of one app. Calls may come from several threads at once.
pub struct Client {
    endpoint: Endpoint,
    info: ClientInfo,
    /// The agent session this client speaks for, if any.
    credential: Option<SessionCredential>,
    session: Mutex<Option<Arc<Session>>>,
    next_id: AtomicU64,
    /// The followed topics and where their events go, kept across reconnects.
    push: Arc<push::Push>,
}

/// The connection's read side, owned so it can sit behind a mutex.
struct Shared(Arc<Connection>);

impl Read for Shared {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        (&*self.0).read(buf)
    }
}

/// How a connection ended, for every call still waiting on it.
#[derive(Clone)]
enum Ended {
    /// The app hung up: a call that is safe to repeat may retry.
    Closed,
    /// The app refused the connection or it broke.
    Failed(ControlError),
}

/// What a waiting call is told.
enum Delivery {
    Reply(Outcome),
    Ended(Ended),
    /// Read the connection for everyone now.
    Lead,
}

#[derive(Default)]
struct Calls {
    waiting: HashMap<u64, Sender<Delivery>>,
    /// Whether some call is reading the connection.
    reading: bool,
    ended: Option<Ended>,
}

struct Session {
    connection: Arc<Connection>,
    reader: Mutex<Lines<Shared>>,
    writing: Mutex<()>,
    welcome: Welcome,
    calls: Mutex<Calls>,
    push: Arc<push::Push>,
    /// Whether a pump thread reads this connection.
    pumping: AtomicBool,
    /// Whether the followed topics are subscribed on this connection.
    subscribed: AtomicBool,
    /// Whether the client let go of this connection; it sends no more notices.
    closed: AtomicBool,
}

enum Failure {
    /// Nothing was sent, or the call is safe to repeat: reconnect and retry.
    Stale(ControlError),
    Final(ControlError),
}

fn timed_out() -> ControlError {
    ControlError::new(ErrorCode::Timeout, "MonoCode did not answer in time")
}

impl Client {
    /// A client of the app on `endpoint`, calling as `info`. Nothing connects
    /// until the first call.
    #[must_use]
    pub fn new(endpoint: Endpoint, info: ClientInfo) -> Self {
        Self {
            endpoint,
            info,
            credential: None,
            session: Mutex::new(None),
            next_id: AtomicU64::new(1),
            push: Arc::default(),
        }
    }
    /// Calls as the agent session `credential` names; the app then grants
    /// what it grants its own sessions, or refuses the hello.
    #[must_use]
    pub fn with_session(mut self, credential: SessionCredential) -> Self {
        self.credential = Some(credential);
        self
    }
    /// The app's welcome, once connected.
    #[must_use]
    pub fn welcome(&self) -> Option<Welcome> {
        lock(&self.session).as_ref().map(|s| s.welcome.clone())
    }
    /// Drops the connection; the next call connects again. Calls still
    /// waiting keep the old connection until they finish; its pushed events
    /// are no longer passed on.
    pub fn disconnect(&self) {
        if let Some(session) = lock(&self.session).take() {
            session.closed.store(true, Ordering::SeqCst);
        }
    }
    /// Connects and says hello, unless already connected.
    ///
    /// # Errors
    /// `app_not_running` when nothing listens; the app's refusal otherwise.
    pub fn connect(&self) -> Result<Welcome, ControlError> {
        self.session().map(|s| s.welcome.clone())
    }
    /// The open connection, or a new one. A new one follows the topics the
    /// client already follows before it is used for anything else.
    fn session(&self) -> Result<Arc<Session>, ControlError> {
        let mut session = lock(&self.session);
        if let Some(open) = session.as_ref().filter(|s| !s.ended()) {
            return Ok(Arc::clone(open));
        }
        let open = Arc::new(self.open()?);
        *session = Some(Arc::clone(&open));
        drop(session);
        self.resume(&open);
        Ok(open)
    }
    /// Drops `session` if it is still the current one.
    fn forget(&self, session: &Arc<Session>) {
        let mut current = lock(&self.session);
        if current.as_ref().is_some_and(|c| Arc::ptr_eq(c, session)) {
            *current = None;
        }
    }
    /// Calls `Op` with `input` and decodes its output.
    ///
    /// # Errors
    /// The app's error, or `internal` when the output does not match `Op`.
    pub fn call<Op: Registered>(&self, input: Op::Input) -> Result<Op::Output, ControlError> {
        let value = self.send(None, Op::command(input))?;
        serde_json::from_value(value).map_err(|e| {
            ControlError::internal(format!("MonoCode answered {} unexpectedly: {e}", Op::NAME))
        })
    }
    /// Sends `command` and returns its raw output. A call that is safe to
    /// repeat (read-only, or carrying a request ID) is retried once on a
    /// fresh connection when the old one turns out to be dead.
    ///
    /// # Errors
    /// The app's error, `app_not_running`, or `invalid_input` for a bad
    /// request ID.
    pub fn send(
        &self,
        request_id: Option<String>,
        command: Command,
    ) -> Result<Value, ControlError> {
        if request_id
            .as_deref()
            .is_some_and(|id| !frame::valid_request_id(id))
        {
            return Err(ControlError::invalid_input(
                "requestId must be 1 to 128 letters, digits, underscores or hyphens",
            ));
        }
        let repeatable = command.hints().read_only || request_id.is_some();
        let id = self.next_id();
        let call = ClientFrame::Call(Call {
            id,
            request_id,
            command,
        });
        self.request(id, &call, repeatable)
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Sends `frame`, numbered `id`, and returns its reply; retried once on
    /// a fresh connection when the old one turns out to be dead.
    fn request(
        &self,
        id: u64,
        frame: &ClientFrame,
        repeatable: bool,
    ) -> Result<Value, ControlError> {
        let reused = lock(&self.session).is_some();
        match self.exchange(id, frame, repeatable) {
            Ok(value) => Ok(value),
            Err(Failure::Stale(_)) if reused => {
                self.exchange(id, frame, repeatable)
                    .map_err(|failure| match failure {
                        Failure::Stale(error) | Failure::Final(error) => error,
                    })
            }
            Err(Failure::Stale(error) | Failure::Final(error)) => Err(error),
        }
    }

    fn exchange(&self, id: u64, frame: &ClientFrame, repeatable: bool) -> Result<Value, Failure> {
        let session = self.session().map_err(Failure::Final)?;
        self.exchange_on(&session, id, frame, repeatable)
    }

    fn exchange_on(
        &self,
        session: &Arc<Session>,
        id: u64,
        frame: &ClientFrame,
        repeatable: bool,
    ) -> Result<Value, Failure> {
        let (deliver, delivered) = mpsc::channel();
        let Ok(reading) = session.register(id, deliver) else {
            // The connection died before this call was sent.
            self.forget(session);
            return Err(Failure::Stale(ControlError::internal(
                "MonoCode closed the control connection",
            )));
        };
        if let Err(error) = session.write(frame) {
            session.leave(id, reading);
            let error = ControlError::from(error);
            if error.code == ErrorCode::InvalidInput {
                return Err(Failure::Final(error));
            }
            self.forget(session);
            return Err(Failure::Stale(error));
        }
        let limit = match frame {
            ClientFrame::Call(call) if call.command.confirmed() => CONFIRMED_CALL_TIMEOUT,
            _ => CALL_TIMEOUT,
        };
        let outcome = Self::wait(session, id, reading, &delivered, limit);
        match outcome {
            Ok(outcome) => Result::from(outcome).map_err(Failure::Final),
            Err(Some(Ended::Closed)) => {
                self.forget(session);
                Err(if repeatable {
                    Failure::Stale(
                        ControlError::internal("MonoCode closed the control connection")
                            .retryable(true),
                    )
                } else {
                    // It may have acted before the connection went.
                    Failure::Final(ControlError::internal(UNKNOWN_OUTCOME))
                })
            }
            Err(Some(Ended::Failed(error))) => {
                self.forget(session);
                Err(Failure::Final(error))
            }
            Err(None) => Err(Failure::Final(timed_out())),
        }
    }

    /// Waits for call `id`'s reply, reading for everyone whenever it is this
    /// call's turn. `Err(None)` is a timeout.
    fn wait(
        session: &Session,
        id: u64,
        mut reading: bool,
        delivered: &Receiver<Delivery>,
        limit: Duration,
    ) -> Result<Outcome, Option<Ended>> {
        let deadline = Instant::now() + limit;
        loop {
            if reading {
                return match session.read_for(id, deadline) {
                    Some(result) => result.map_err(Some),
                    None => Err(None),
                };
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match delivered.recv_timeout(left) {
                Ok(Delivery::Reply(outcome)) => return Ok(outcome),
                Ok(Delivery::Ended(ended)) => return Err(Some(ended)),
                Ok(Delivery::Lead) => reading = true,
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    session.leave(id, false);
                    // A turn to read or a reply may have arrived meanwhile.
                    while let Ok(late) = delivered.try_recv() {
                        match late {
                            Delivery::Reply(outcome) => return Ok(outcome),
                            Delivery::Ended(ended) => return Err(Some(ended)),
                            Delivery::Lead => session.leave(id, true),
                        }
                    }
                    return Err(None);
                }
            }
        }
    }

    fn open(&self) -> Result<Session, ControlError> {
        let connection = Connection::connect(&self.endpoint).map_err(|error| {
            if nobody_listening(&error) {
                ControlError::app_not_running()
            } else {
                ControlError::internal(format!(
                    "Cannot reach MonoCode at {}: {error}",
                    self.endpoint.describe()
                ))
                .retryable(true)
            }
        })?;
        let io = |e: std::io::Error| ControlError::from(FrameError::Io(e));
        connection
            .set_write_timeout(Some(WRITE_TIMEOUT))
            .map_err(io)?;
        connection
            .set_read_timeout(Some(HELLO_TIMEOUT))
            .map_err(io)?;
        let connection = Arc::new(connection);
        let mut reader = Lines::new(Shared(Arc::clone(&connection)));
        let hello = ClientFrame::Hello(Hello {
            version: PROTOCOL_VERSION,
            client: self.info.clone(),
            session: self.credential.clone(),
        });
        frame::write_frame(&mut &*connection, &hello, MAX_REQUEST_BYTES)?;
        let welcome = match reader.next_frame::<ServerFrame>(MAX_RESPONSE_BYTES)? {
            Some(ServerFrame::Welcome(welcome)) => welcome,
            Some(ServerFrame::Error(error)) => return Err(error),
            Some(_) => {
                return Err(ControlError::internal(
                    "MonoCode answered hello unexpectedly",
                ));
            }
            None => {
                return Err(
                    ControlError::internal("MonoCode closed the control connection")
                        .retryable(true),
                );
            }
        };
        if welcome.version != PROTOCOL_VERSION {
            return Err(ControlError::invalid_input(format!(
                "MonoCode speaks control protocol {}; this relay speaks {PROTOCOL_VERSION}. Use the relay of the running app.",
                welcome.version
            )));
        }
        connection
            .set_read_timeout(Some(CONFIRMED_CALL_TIMEOUT))
            .map_err(io)?;
        Ok(Session {
            connection,
            reader: Mutex::new(reader),
            writing: Mutex::new(()),
            welcome,
            calls: Mutex::new(Calls::default()),
            push: Arc::clone(&self.push),
            pumping: AtomicBool::new(false),
            subscribed: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        })
    }
}

impl Drop for Client {
    /// Hangs up a connection that a pump thread still reads.
    fn drop(&mut self) {
        if let Some(session) = lock(&self.session).take() {
            session.close();
        }
    }
}
