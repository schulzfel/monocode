//! External control (MCP): the app's side of `monocode-control`.
//!
//! A listener serves the data folder's control socket while the app runs:
//! the agent sessions MonoCode runs always reach it (`session_caller`), and
//! external clients only while the stored policy (`external-control.v1`)
//! has control on. While it is on, the app also serves MCP over HTTP on
//! localhost (`mcp_http`), whose sessions call in-process (`in_app`).
//! Connection threads and HTTP sessions hand
//! each hello and call to the shell on the UI thread, which checks the
//! client's grant, replays or records idempotency receipts, runs the
//! operation through the same state the window shows, and logs the call.
//! Connections may also follow topics; the hub turns the window's change
//! signals into pushed events (`push`). Only Settings changes the policy;
//! no operation can.
mod activity;
mod auth;
mod caller;
mod confirm;
mod defaults;
mod exec;
mod feed;
mod gate;
mod hub;
mod in_app;
mod listener;
mod mcp_http;
mod prompts;
mod push;
mod receipts;
mod session_caller;
mod settings_page;
#[cfg(test)]
mod test_hooks;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_automation;
#[cfg(test)]
mod tests_capacity;
#[cfg(test)]
mod tests_cli;
#[cfg(test)]
mod tests_confirm;
#[cfg(test)]
mod tests_denied;
#[cfg(test)]
mod tests_discard;
#[cfg(test)]
mod tests_dispatch;
#[cfg(test)]
mod tests_exec;
#[cfg(test)]
mod tests_gate;
#[cfg(test)]
mod tests_gate_mono;
#[cfg(test)]
mod tests_http;
#[cfg(test)]
mod tests_lifecycle;
#[cfg(test)]
mod tests_listener;
#[cfg(test)]
mod tests_lost;
#[cfg(test)]
mod tests_manage;
#[cfg(test)]
mod tests_mono;
#[cfg(test)]
mod tests_operator;
#[cfg(test)]
mod tests_operator_ops;
#[cfg(test)]
mod tests_ops;
#[cfg(test)]
mod tests_parity;
#[cfg(test)]
mod tests_push;
#[cfg(test)]
mod tests_questions;
#[cfg(test)]
mod tests_receipts;
#[cfg(test)]
mod tests_scoped;
#[cfg(test)]
mod tests_session;
#[cfg(test)]
mod tests_thread;
#[cfg(test)]
mod tests_thread_ops;
#[cfg(test)]
mod tests_vcs;
#[cfg(test)]
mod tests_window;

use super::*;
use crate::settings::{ExternalControlEdit, ExternalControlStatus};
use caller::CallerKind;
pub(crate) use gate::MONO_OPS;
use monocode_control::{
    ACTIVITY_KEY, ControlError, Endpoint, ErrorCode, Grant, POLICY_KEY, Policy, Scope,
    transport::{BindError, Listener},
};
use monocode_core::usage::now_ms;
use serde_json::Value;
use std::{sync::mpsc::SyncSender, time::Duration};

use listener::Request;

/// How long the person's prompts wait, and how long a denial is
/// remembered; tests shorten them.
pub(super) struct Timeouts {
    /// How long an approval prompt waits for an answer.
    approval: Duration,
    /// How long a client the person denied is refused without asking.
    denial_memory: Duration,
    /// How long a confirmation prompt waits for an answer.
    confirm: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            approval: auth::APPROVAL_TIMEOUT,
            denial_memory: auth::DENIAL_MEMORY,
            confirm: monocode_control::CONFIRM_TIMEOUT,
        }
    }
}

/// The start of the banner that says the sessions' MonoCode tools are
/// unavailable.
const UNAVAILABLE: &str = "MonoCode tools are unavailable: ";

/// What the shell keeps for external control.
pub(super) struct Control {
    policy: Policy,
    /// The data folder's endpoint; `None` for an in-memory store.
    endpoint: Option<Result<Endpoint, ControlError>>,
    server: Option<listener::Server>,
    /// Whether the person was told that the socket cannot be served, since
    /// it last was.
    told_unavailable: bool,
    /// The MCP server on localhost, while it runs.
    http: Option<mcp_http::Serving>,
    /// Why the MCP server is on but not serving.
    http_error: Option<String>,
    /// The MCP server's port when the policy names none.
    default_port: u16,
    /// Where the socket's connections and the HTTP sessions send their
    /// hellos and calls, and where the shell reads them.
    requests: async_channel::Sender<Request>,
    received: async_channel::Receiver<Request>,
    /// Hellos waiting on the approval prompt, by client name.
    approvals: HashMap<String, auth::Approval>,
    next_approval: u64,
    timeouts: Timeouts,
    /// Clients the person denied recently, with the denial's number.
    denied: HashMap<String, u64>,
    /// Calls waiting on the person's confirmation, one per client.
    confirmations: HashMap<CallerKind, confirm::Waiting>,
    /// Calls with a request ID that are still running, by caller.
    pending: HashSet<(CallerKind, String)>,
    /// When the last receipt was kept, so each is newer than the one before.
    last_receipt: i64,
    activity: activity::Activity,
    /// Where `model_list` reads providers from; tests swap in a fixture.
    models: exec::model::Source,
    /// Sessions clients started, and who: `mcp:<client>`. Shown as their
    /// `startedBy` while this window runs.
    started: HashMap<i64, String>,
    /// Who follows which topics, and the events they are owed.
    hub: hub::Hub,
    /// Chats watched because someone follows their session.
    watched: HashMap<i64, push::Watched>,
}

impl Default for Control {
    fn default() -> Self {
        let (requests, received) = async_channel::unbounded();
        Self {
            policy: Policy::default(),
            endpoint: None,
            server: None,
            told_unavailable: false,
            http: None,
            http_error: None,
            // Any free port until the shell knows its data folder.
            default_port: 0,
            requests,
            received,
            approvals: HashMap::new(),
            next_approval: 0,
            timeouts: Timeouts::default(),
            denied: HashMap::new(),
            confirmations: HashMap::new(),
            pending: HashSet::new(),
            last_receipt: 0,
            activity: activity::Activity::default(),
            models: monocode_core::app_access::models_catalog,
            started: HashMap::new(),
            hub: hub::Hub::default(),
            watched: HashMap::new(),
        }
    }
}

/// One call on its way through the shell.
struct Pending {
    caller: CallerKind,
    request_id: Option<String>,
    command: monocode_control::Command,
    reply: SyncSender<Result<Value, ControlError>>,
}

impl Shell {
    /// Reads the policy and activity and binds the socket on a worker,
    /// then listens.
    pub(super) fn init_control(&mut self, cx: &mut Context<Self>) {
        // Tests serve on any free port, never on the real one.
        if !cx.is_test() {
            self.control.default_port = mcp_http::default_port(crate::data_location::mode());
            self.drain_control(cx);
        }
        let store = self.store.clone();
        let load = cx.background_executor().spawn(async move {
            let store = locked(&store)?;
            let endpoint = store.data_dir().map(Endpoint::for_data_dir);
            let policy = store.setting(POLICY_KEY)?;
            let activity = store.setting(ACTIVITY_KEY)?;
            drop(store);
            let bound = match &endpoint {
                Some(Ok(endpoint)) => Some(Listener::bind(endpoint)),
                _ => None,
            };
            Ok::<_, String>((endpoint, policy, activity, bound))
        });
        cx.on_app_quit(|this, cx| this.flush_activity(cx)).detach();
        cx.spawn(async move |this, cx| {
            let loaded = load.await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok((endpoint, policy, activity, bound)) => {
                        this.control.endpoint = endpoint;
                        this.control.policy = Policy::parse(policy.as_deref());
                        this.control.activity = activity::Activity::parse(activity.as_deref());
                        if let (Some(Ok(endpoint)), Some(bound)) =
                            (this.control.endpoint.clone(), bound)
                        {
                            this.start_control(&endpoint, bound, cx);
                        }
                    }
                    Err(error) => eprintln!("Cannot read the external control policy: {error}"),
                }
                this.apply_control(cx);
            });
        })
        .detach();
    }

    /// Listens, whatever the policy says: the app's own sessions call in
    /// either way. With control off, external hellos and calls are refused
    /// (`control_disabled`) and their pending prompts are dropped. A socket
    /// that could not be bound is tried again here. The MCP server on
    /// localhost runs only while control is on.
    fn apply_control(&mut self, cx: &mut Context<Self>) {
        if !self.control.policy.enabled {
            for (_, approval) in self.control.approvals.drain() {
                approval.refuse(&ControlError::control_disabled());
            }
            self.refuse_confirmations(cx);
        }
        if self.control.server.is_none() {
            match self.control.endpoint.clone() {
                Some(Ok(endpoint)) => {
                    let bound = Listener::bind(&endpoint);
                    self.start_control(&endpoint, bound, cx);
                }
                Some(Err(error)) => {
                    eprintln!("Cannot serve the control socket: {error}");
                    self.control_unavailable(&error.message, cx);
                }
                // An in-memory store has no data folder to put a socket in.
                None => {}
            }
        }
        self.apply_mcp_http();
        self.sync_settings(cx);
    }

    fn start_control(
        &mut self,
        endpoint: &Endpoint,
        bound: Result<Listener, BindError>,
        cx: &mut Context<Self>,
    ) {
        let version = crate::notifications::updater::version().to_owned();
        let requests = self.control.requests.clone();
        match bound
            .and_then(|bound| listener::Server::start_with(bound, endpoint, version, requests))
        {
            Ok(server) => {
                self.control.server = Some(server);
                // The banner of an earlier failure is out of date now; any
                // other message the window shows stays.
                if self
                    .error
                    .as_deref()
                    .is_some_and(|shown| shown.starts_with(UNAVAILABLE))
                {
                    self.error = None;
                    cx.notify();
                }
                self.control.told_unavailable = false;
            }
            Err(error) => {
                eprintln!("Cannot serve the control socket: {error}");
                self.control_unavailable(&error.to_string(), cx);
            }
        }
    }

    /// Tells the person, once, that the sessions' MonoCode tools are
    /// unavailable: the socket cannot be served, for `reason`.
    fn control_unavailable(&mut self, reason: &str, cx: &mut Context<Self>) {
        if !std::mem::replace(&mut self.control.told_unavailable, true) {
            self.error = Some(format!("{UNAVAILABLE}{reason}"));
            cx.notify();
        }
    }

    /// Answers the socket's and the HTTP sessions' requests on the UI
    /// thread. Not in tests: the test scheduler forbids wake-ups from
    /// other threads, so tests call `handle_control` directly.
    fn drain_control(&self, cx: &mut Context<Self>) {
        let requests = self.control.received.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        this.handle_control(request, window, cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    /// Answers one request from a connection thread.
    fn handle_control(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        match request {
            Request::Hello {
                client,
                session,
                reply,
            } => self.control_hello(client, session, reply, window, cx),
            Request::Call {
                caller,
                request_id,
                command,
                reply,
            } => self.control_call(
                Pending {
                    caller,
                    request_id,
                    command: *command,
                    reply,
                },
                window,
                cx,
            ),
            Request::Subscribe {
                connection,
                caller,
                topics,
                events,
                reply,
            } => self.control_subscribe(connection, caller, topics, events, &reply, cx),
            Request::Unsubscribe {
                connection,
                topics,
                reply,
            } => self.control_unsubscribe(connection, &topics, &reply, cx),
            Request::Closed { connection } => self.control_closed(connection, cx),
        }
    }

    fn control_call(&mut self, mut call: Pending, window: &mut Window, cx: &mut Context<Self>) {
        // A session's credential may stand for a Mono; who it is now decides.
        let checked = self.resolve_caller(&call.caller, cx).and_then(|caller| {
            call.caller = caller;
            let grant = self.control_grant(&call.caller, &call.command, cx)?;
            // Before the receipt, so it records the call as it runs.
            self.control_defaults(&mut call, cx)?;
            Ok(grant)
        });
        let grant = match checked {
            Ok(grant) => grant,
            Err(error) => return self.refuse_control(call, error, cx),
        };
        if call.request_id.is_some() {
            self.control_with_receipt(call, grant, window, cx);
        } else {
            self.run_control(call, &grant, window, cx);
        }
    }

    /// Runs the operation once its target checks out, and answers when it
    /// is done; one that deletes, discards or publishes waits for the
    /// person's confirmation first, unless a session calls: its provider's
    /// permission mode decides.
    fn run_control(
        &mut self,
        call: Pending,
        grant: &Grant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.control_target(&call.caller, &call.command, cx) {
            return self.finish_control(call, Err(error), cx);
        }
        match (call.command.confirmed(), call.caller.confirms()) {
            (true, true) => self.confirm_control(call, cx),
            (true, false) => self.run_unconfirmed(call, grant.clone(), cx),
            (false, _) => self.execute_control(call, grant, None, window, cx),
        }
    }

    /// Runs the call; `pin` is what the person confirmed, if they were asked.
    fn execute_control(
        &mut self,
        call: Pending,
        grant: &Grant,
        pin: Option<&exec::vcs::Pin>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let caller = exec::Caller {
            caller: &call.caller,
            request_id: call.request_id.as_deref(),
            pin,
        };
        let exec = self.exec_control(grant, &caller, call.command.clone(), window, cx);
        match exec {
            exec::Exec::Ready(result) => self.finish_control(call, result, cx),
            exec::Exec::Later(task) => {
                cx.spawn(async move |this, cx| {
                    let result = task.await;
                    let _ = this.update(cx, |this, cx| this.finish_control(call, result, cx));
                })
                .detach();
            }
        }
    }

    /// Records the call, keeps its receipt when it has a request ID, then
    /// answers. A reply with a receipt is sent only once the receipt is stored.
    /// A refusal keeps none (the person declined, another prompt was open,
    /// or control or the grant went away meanwhile), and neither does a
    /// failure worth retrying (`retryable`), so a retry runs again.
    fn finish_control(
        &mut self,
        call: Pending,
        result: Result<Value, ControlError>,
        cx: &mut Context<Self>,
    ) {
        let wire = activity::wire(&call.command, call.request_id.as_deref());
        self.record_control(&call.caller, &call.command, wire.as_ref(), &result, cx);
        let refused = matches!(&result, Err(e) if e.retryable || matches!(
            e.code,
            ErrorCode::UserDeclined
                | ErrorCode::RequestConflict
                | ErrorCode::ControlDisabled
                | ErrorCode::ScopeDenied
        ));
        let Some(request_id) = call.request_id.clone().filter(|_| !refused) else {
            if let Some(request_id) = &call.request_id {
                self.control
                    .pending
                    .remove(&(call.caller.clone(), request_id.clone()));
            }
            let _ = call.reply.send(result);
            return;
        };
        self.keep_receipt(call, request_id, wire.unwrap_or_default(), result, cx);
    }
}
