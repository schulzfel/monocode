//! The control socket: an accept thread, one thread per connection, and a
//! channel of requests the shell answers on the UI thread. A connection runs
//! up to [`MAX_CALLS`] calls at once, each on its own scoped thread, so a
//! long `session_wait` never holds up a read on the same connection. A
//! connection that follows topics gets one more thread, which writes the
//! events the shell queues for it.
use super::{caller::CallerKind, feed::Feed, hub};
use monocode_control::{
    ClientInfo, Command, ControlError, Endpoint, ErrorCode, Grant, PROTOCOL_VERSION,
    SessionCredential, Topic,
    frame::{
        self, ClientFrame, Event, FrameError, Hello, MAX_CONNECTIONS, MAX_REQUEST_BYTES,
        MAX_RESPONSE_BYTES, Outcome, Reply, ServerFrame, Welcome,
    },
    lock,
    transport::{BindError, Connection, Listener},
};
use serde_json::Value;
use std::{
    io::BufReader,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
    time::Duration,
};

/// How long a new connection may take to say hello.
const HELLO_WAIT: Duration = Duration::from_secs(10);
/// How long the shell may take to decide on a hello; the approval prompt
/// itself gives up after two minutes.
const DECISION_WAIT: Duration = Duration::from_secs(125);
/// How long the shell may take to answer a call: the longest `session_wait`
/// plus a margin, and less than the client's own deadline.
const CALL_WAIT: Duration = Duration::from_secs(90);
const _: () = assert!(
    CALL_WAIT.as_secs() >= monocode_control::ops::dispatch::MAX_WAIT_MS as u64 / 1000 + 20,
    "a session_wait must finish before the listener gives up"
);
/// A call the person confirms first may also wait for the prompt, which
/// gives up after `CONFIRM_TIMEOUT`.
const CONFIRMED_CALL_WAIT: Duration =
    Duration::from_secs(CALL_WAIT.as_secs() + monocode_control::CONFIRM_TIMEOUT.as_secs() + 5);
pub(super) const WRITE_WAIT: Duration = Duration::from_secs(10);
/// The most calls one connection may have running at once; more are
/// refused, retryably, until one finishes.
pub(super) const MAX_CALLS: usize = 8;

/// What the shell is asked to decide.
pub(super) enum Request {
    /// A client said hello: grant it, ask the person, or refuse. `session`
    /// is the credential of an agent session MonoCode runs, if it sent one.
    Hello {
        client: ClientInfo,
        session: Option<SessionCredential>,
        reply: SyncSender<Result<Grant, ControlError>>,
    },
    /// A welcomed client called an operation.
    Call {
        caller: CallerKind,
        request_id: Option<String>,
        command: Command,
        reply: SyncSender<Result<Value, ControlError>>,
    },
    /// A welcomed client follows topics; its events go into `events`.
    Subscribe {
        connection: u64,
        caller: CallerKind,
        topics: Vec<Topic>,
        events: hub::Events,
        reply: SyncSender<Result<Value, ControlError>>,
    },
    /// A client stops following topics.
    Unsubscribe {
        connection: u64,
        topics: Vec<Topic>,
        reply: SyncSender<Result<Value, ControlError>>,
    },
    /// A connection that followed topics closed.
    Closed { connection: u64 },
}

/// A running listener. Dropping it stops accepting and removes the socket.
pub(super) struct Server {
    endpoint: Endpoint,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl Server {
    /// Serves `listener`, bound on `endpoint`, and returns the requests to
    /// answer.
    #[cfg(test)]
    pub(super) fn start(
        listener: Listener,
        endpoint: &Endpoint,
        app_version: String,
    ) -> Result<(Self, async_channel::Receiver<Request>), BindError> {
        let (requests, received) = async_channel::unbounded();
        Self::start_with(listener, endpoint, app_version, requests).map(|s| (s, received))
    }

    /// Serves `listener`, bound on `endpoint`, sending the requests to
    /// answer into `requests`, which the app's HTTP server shares.
    pub(super) fn start_with(
        listener: Listener,
        endpoint: &Endpoint,
        app_version: String,
        requests: async_channel::Sender<Request>,
    ) -> Result<Self, BindError> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let accept = std::thread::Builder::new()
            .name("control-accept".into())
            .spawn(move || accept_loop(&listener, &requests, &stopping, &app_version))
            .map_err(BindError::Io)?;
        Ok(Self {
            endpoint: endpoint.clone(),
            stop,
            accept: Some(accept),
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocked accept; it sees the flag and drops the listener,
        // which removes the socket. Only wait when the wake-up got through.
        if Connection::connect(&self.endpoint).is_ok()
            && let Some(accept) = self.accept.take()
        {
            let _ = accept.join();
        }
    }
}

fn accept_loop(
    listener: &Listener,
    requests: &async_channel::Sender<Request>,
    stop: &Arc<AtomicBool>,
    app_version: &str,
) {
    let active = Arc::new(AtomicUsize::new(0));
    loop {
        let accepted = listener.accept();
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(connection) = accepted else {
            // A failing accept (descriptor exhaustion) must not spin.
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            let busy = ControlError::internal("MonoCode is serving too many control connections")
                .retryable(true);
            let _ = connection.set_write_timeout(Some(WRITE_WAIT));
            let _ = frame::write_frame(
                &mut &connection,
                &ServerFrame::Error(busy),
                MAX_RESPONSE_BYTES,
            );
            continue;
        }
        let requests = requests.clone();
        let stop = stop.clone();
        let active = active.clone();
        let app_version = app_version.to_owned();
        let spawned = std::thread::Builder::new()
            .name("control-connection".into())
            .spawn(move || {
                serve(&connection, &requests, &stop, &app_version);
                active.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            continue;
        }
    }
}

fn send_error(connection: &Connection, error: ControlError) {
    let _ = frame::write_frame(
        &mut &*connection,
        &ServerFrame::Error(error),
        MAX_RESPONSE_BYTES,
    );
}

/// Serves one connection: hello first, then calls until the client leaves.
fn serve(
    connection: &Connection,
    requests: &async_channel::Sender<Request>,
    stop: &AtomicBool,
    app_version: &str,
) {
    let _ = connection.set_write_timeout(Some(WRITE_WAIT));
    let _ = connection.set_read_timeout(Some(HELLO_WAIT));
    let mut reader = BufReader::new(connection);
    let Some(Hello {
        client, session, ..
    }) = hello(connection, &mut reader)
    else {
        return;
    };
    // A session's calls are its own, whatever name the client says.
    let caller = CallerKind::of_hello(&client, session.as_ref());
    let grant = match decide(requests, client, session) {
        Ok(grant) => grant,
        Err(error) => return send_error(connection, error),
    };
    let welcome = ServerFrame::Welcome(Welcome {
        version: PROTOCOL_VERSION,
        app_version: app_version.to_owned(),
        grant,
        push: true,
    });
    if frame::write_frame(&mut &*connection, &welcome, MAX_RESPONSE_BYTES).is_err() {
        return;
    }
    let _ = connection.set_read_timeout(None);
    let writer = Writer {
        connection,
        lock: Mutex::new(()),
    };
    let running = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let mut feed = Feed::new(requests.clone());
        loop {
            let line = match frame::read_line(&mut reader, MAX_REQUEST_BYTES) {
                Ok(Some(line)) => line,
                Ok(None) | Err(FrameError::Io(_) | FrameError::Malformed(_)) => return,
                Err(error @ FrameError::TooLarge(_)) => return writer.error(error.into()),
            };
            if stop.load(Ordering::SeqCst) {
                return writer.error(ControlError::control_disabled());
            }
            let call = match frame::decode_client(&line) {
                Ok(ClientFrame::Call(call)) => call,
                Ok(ClientFrame::Hello(_)) => {
                    return writer.error(ControlError::invalid_input("Hello was already said"));
                }
                Ok(ClientFrame::Subscribe(subscription)) => {
                    if feed.events.is_none() {
                        let (events, queued) = mpsc::sync_channel(hub::QUEUE);
                        let writer = &writer;
                        let spawned = std::thread::Builder::new()
                            .name("control-push".into())
                            .spawn_scoped(scope, move || write_events(writer, &queued));
                        if spawned.is_ok() {
                            feed.events = Some(events);
                        }
                    }
                    let id = subscription.id;
                    if writer.answer(id, feed.follow(&caller, subscription, true)) {
                        continue;
                    }
                    return;
                }
                Ok(ClientFrame::Unsubscribe(subscription)) => {
                    let id = subscription.id;
                    if writer.answer(id, feed.follow(&caller, subscription, false)) {
                        continue;
                    }
                    return;
                }
                Err((Some(id), error)) => {
                    if writer.answer(id, Err(error)) {
                        continue;
                    }
                    return;
                }
                Err((None, error)) => return writer.error(error),
            };
            if call
                .request_id
                .as_deref()
                .is_some_and(|id| !frame::valid_request_id(id))
            {
                let invalid = ControlError::invalid_input(
                    "requestId must be 1 to 128 letters, digits, underscores or hyphens",
                );
                if writer.answer(call.id, Err(invalid)) {
                    continue;
                }
                return;
            }
            if running.fetch_add(1, Ordering::SeqCst) >= MAX_CALLS {
                running.fetch_sub(1, Ordering::SeqCst);
                if writer.answer(call.id, Err(busy())) {
                    continue;
                }
                return;
            }
            let (writer, running, caller) = (&writer, &running, &caller);
            let id = call.id;
            let spawned = std::thread::Builder::new()
                .name("control-call".into())
                .spawn_scoped(scope, move || {
                    let result = ask(requests, caller, call.request_id, call.command);
                    writer.answer(id, result);
                    running.fetch_sub(1, Ordering::SeqCst);
                });
            if spawned.is_err() {
                running.fetch_sub(1, Ordering::SeqCst);
                if !writer.answer(id, Err(busy())) {
                    return;
                }
            }
        }
    });
}

/// Writes queued events until the connection ends or the queue says stop.
/// Told that the connection may no longer follow, it says why and closes
/// the connection, which ends the read loop too.
fn write_events(writer: &Writer<'_>, queued: &Receiver<hub::Push>) {
    while let Ok(push) = queued.recv() {
        match push {
            hub::Push::Event(event) => {
                if !writer.event(event) {
                    return;
                }
            }
            hub::Push::Lost(error) => {
                writer.error(error);
                let _ = writer.connection.shutdown();
                return;
            }
            hub::Push::End => return,
        }
    }
}

/// Refuses a call over [`MAX_CALLS`].
pub(super) fn busy() -> ControlError {
    ControlError::internal(format!(
        "MonoCode is already running {MAX_CALLS} calls from this client; retry when one finishes"
    ))
    .retryable(true)
}

/// Writes whole frames to a connection several call threads share.
struct Writer<'a> {
    connection: &'a Connection,
    lock: Mutex<()>,
}

impl Writer<'_> {
    fn error(&self, error: ControlError) {
        let _held = lock(&self.lock);
        send_error(self.connection, error);
    }
    fn answer(&self, id: u64, result: Result<Value, ControlError>) -> bool {
        let _held = lock(&self.lock);
        answer(self.connection, id, result)
    }
    fn event(&self, event: Event) -> bool {
        let _held = lock(&self.lock);
        let frame = ServerFrame::Event(event);
        frame::write_frame(&mut &*self.connection, &frame, MAX_RESPONSE_BYTES).is_ok()
    }
}

/// Reads the hello, answering anything else with an error.
fn hello(connection: &Connection, reader: &mut BufReader<&Connection>) -> Option<Hello> {
    let line = frame::read_line(reader, MAX_REQUEST_BYTES).ok()??;
    match frame::decode_client(&line) {
        Ok(ClientFrame::Hello(hello)) if hello.version == PROTOCOL_VERSION => Some(hello),
        Ok(ClientFrame::Hello(hello)) => {
            send_error(
                connection,
                ControlError::invalid_input(format!(
                    "This relay speaks control protocol {}; MonoCode speaks {PROTOCOL_VERSION}. Use the relay of the running app.",
                    hello.version
                )),
            );
            None
        }
        Ok(ClientFrame::Call(_) | ClientFrame::Subscribe(_) | ClientFrame::Unsubscribe(_))
        | Err(_) => {
            send_error(connection, ControlError::invalid_input("Say hello first"));
            None
        }
    }
}

/// Asks the shell to decide on a hello: the caller's grant, or why not.
pub(super) fn decide(
    requests: &async_channel::Sender<Request>,
    client: ClientInfo,
    session: Option<SessionCredential>,
) -> Result<Grant, ControlError> {
    let (reply, decision) = mpsc::sync_channel(1);
    let hello = Request::Hello {
        client,
        session,
        reply,
    };
    if requests.send_blocking(hello).is_err() {
        return Err(ControlError::control_disabled());
    }
    decision.recv_timeout(DECISION_WAIT).unwrap_or_else(|_| {
        Err(ControlError::new(
            ErrorCode::UserDeclined,
            "Nobody answered in MonoCode",
        ))
    })
}

/// Asks the shell to run a call, and waits for its answer.
pub(super) fn ask(
    requests: &async_channel::Sender<Request>,
    caller: &CallerKind,
    request_id: Option<String>,
    command: Command,
) -> Result<Value, ControlError> {
    let (reply, answer) = mpsc::sync_channel(1);
    let wait = if command.confirmed() {
        CONFIRMED_CALL_WAIT
    } else {
        CALL_WAIT
    };
    let request = Request::Call {
        caller: caller.clone(),
        request_id,
        command,
        reply,
    };
    if requests.send_blocking(request).is_err() {
        return Err(ControlError::control_disabled());
    }
    match answer.recv_timeout(wait) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(ControlError::new(
            ErrorCode::Timeout,
            "MonoCode did not finish the call in time",
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(ControlError::control_disabled()),
    }
}

/// Writes the reply; an oversized result becomes an error reply. Returns
/// whether the connection is still usable.
fn answer(connection: &Connection, id: u64, result: Result<Value, ControlError>) -> bool {
    let frame = ServerFrame::Reply(Reply {
        id,
        result: Outcome::from(result),
    });
    match frame::write_frame(&mut &*connection, &frame, MAX_RESPONSE_BYTES) {
        Ok(()) => true,
        Err(FrameError::TooLarge(limit)) => {
            let error = ServerFrame::Reply(Reply {
                id,
                result: Outcome::Error(ControlError::internal(format!(
                    "The result is larger than {limit} bytes; ask for a smaller page"
                ))),
            });
            frame::write_frame(&mut &*connection, &error, MAX_RESPONSE_BYTES).is_ok()
        }
        Err(_) => false,
    }
}
