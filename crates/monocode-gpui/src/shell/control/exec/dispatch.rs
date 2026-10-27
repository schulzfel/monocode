//! `session_start`, `session_send`, `session_wait` and `session_stop`.
//! Starting reuses Operator's launch pipeline with the client as origin, so
//! launched sessions never get Operator, plan or control capability. An
//! agent session starts sessions as Operator's threads do: from its own
//! live settings, beside its own pane. A Mono starts them as its old app
//! actions did: from its main chat, recorded among its launches and kept
//! out of the sidebar, with a completion report unless it only drafts.
use super::session::{access_or_default, no_session, remote_unsupported};
use super::*;
use crate::shell::session_launch::{Launch, NewChat, Origin, launch_session};
use monocode_control::{
    AccessMode,
    ops::dispatch::{
        DEFAULT_WAIT_MS, Delivery, MAX_PROMPT_BYTES, MAX_WAIT_MS, Placement, SendMode,
        SessionSendInput, SessionSendOutput, SessionStartInput, SessionStartOutput,
        SessionStopOutput, SessionWaitInput, SessionWaitOutput, WorkspaceMode,
    },
};
use monocode_core::agent::Settings;
use monocode_core::app_access as app;
use serde_json::json;

/// The prompt, checked as Operator checks it.
pub(super) fn prompt(text: &str) -> Result<String, ControlError> {
    if text.len() > MAX_PROMPT_BYTES {
        return Err(ControlError::invalid_input(format!(
            "The prompt is {} bytes; at most {MAX_PROMPT_BYTES} are accepted.",
            text.len()
        )));
    }
    app::prompt(&json!({ "prompt": text })).map_err(ControlError::invalid_input)
}

/// Only a Mono's chat hears back when a session it started or messaged is
/// done.
const MONO_NOTIFY: &str = "Completion notifications are only available in a Mono's chat";
/// A draft is never sent, so it never completes.
const DRAFT_NOTIFY: &str = "An unsent draft cannot send a completion notification";
/// `session_send` with `ifIdle` into a session that is not ready, in
/// Operator's words.
const NOT_IDLE: &str = "Session is busy, loading, or has an unsent draft. Its draft was preserved.";

/// Where a launch's settings come from.
enum Start {
    /// An agent session's live settings.
    Inherited(Box<Settings>),
    /// A new chat's defaults, read from the store on the worker.
    New(NewChat),
}

fn launch_error(error: String) -> ControlError {
    if error.starts_with("Unknown model") {
        ControlError::new(ErrorCode::ModelUnavailable, error)
    } else {
        ControlError::invalid_input(error)
    }
}

/// Operator's launch options for `input`, with the session's access.
fn options(input: &SessionStartInput, prompt: &str, access: AccessMode) -> Value {
    let mut options = json!({ "prompt": prompt, "runtimeMode": access.id() });
    let mut set = |key: &str, value: Value| {
        options[key] = value;
    };
    if let Some(model) = &input.model {
        set("model", json!(model));
    }
    if let Some(effort) = &input.effort {
        set("effort", json!(effort));
    }
    if let Some(settings) = &input.model_settings {
        set("modelSettings", json!(settings));
    }
    if let Some(mode) = input.workspace_mode {
        let mode = match mode {
            WorkspaceMode::Current => "current",
            WorkspaceMode::Worktree => "worktree",
        };
        set("workspaceMode", json!(mode));
    }
    if let Some(cwd) = &input.worktree_cwd {
        set("worktreeCwd", json!(cwd));
    }
    if let Some(base) = &input.worktree_base {
        set("worktreeBase", json!(base));
    }
    if let Some(placement) = input.placement {
        let placement = match placement {
            Placement::Tab => "tab",
            Placement::Right => "right",
            Placement::Down => "down",
        };
        set("placement", json!(placement));
    }
    if let Some(id) = input.beside_session_id {
        set("besideSessionId", json!(id));
    }
    if let Some(title) = &input.title {
        set("title", json!(title));
    }
    options
}

impl Shell {
    pub(super) fn control_session_start(
        &mut self,
        input: &SessionStartInput,
        caller: &Caller,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let draft = input.draft.unwrap_or(false);
        let mono = match caller.caller {
            CallerKind::Mono { .. } => true,
            CallerKind::Session(_) | CallerKind::External(_) => false,
        };
        if input.notify_on_complete == Some(true) && (draft || !mono) {
            let refusal = if draft { DRAFT_NOTIFY } else { MONO_NOTIFY };
            return Err(ControlError::invalid_input(refusal));
        }
        // A Mono hears back from what it starts unless it only drafts (MO-21).
        let notify = input.notify_on_complete.unwrap_or(!draft);
        let prompt = prompt(&input.prompt)?;
        let project =
            self.control_local_project_for(caller.caller, input.project.as_deref(), cx)?;
        let harness = match &input.harness {
            Some(id) => Some(Kind::parse(id).ok_or_else(|| {
                ControlError::invalid_input(format!(
                    "There is no harness {id}; see model_list for the providers."
                ))
            })?),
            None => None,
        };
        // An agent session's launch inherits its settings and access and goes
        // beside its pane; a Mono's inherits its main chat's (or its habit
        // run's), beside its chat, which is never a pane; a client's starts
        // as a new chat, beside the focus.
        let (kind, source_provider, start, access, beside) = match caller.caller {
            CallerKind::Session(id) => {
                let source = self
                    .session_source(*id, harness, &project, cx)
                    .ok_or_else(|| no_session(*id))?;
                let access = input
                    .access
                    .unwrap_or_else(|| source.settings.access.into());
                let start = Start::Inherited(Box::new(source.settings));
                (source.kind, source.provider, start, access, Some(*id))
            }
            CallerKind::Mono { session, mono, .. } => {
                let source = self.mono_source(*session, mono, harness, &project, cx);
                let access = input
                    .access
                    .unwrap_or_else(|| source.settings.access.into());
                let start = Start::Inherited(Box::new(source.settings));
                (source.kind, source.provider, start, access, Some(*session))
            }
            CallerKind::External(_) => {
                let kind = harness.unwrap_or_else(|| self.default_kind_for(&project));
                let mut new_chat = self.new_chat(kind, &project, cx);
                new_chat.access = access_or_default(input.access, cx);
                let access = new_chat.access.into();
                let focused = self.control_focused_pane(&project, None, cx);
                (
                    kind,
                    kind.id().into(),
                    Start::New(new_chat),
                    access,
                    focused,
                )
            }
        };
        let options = options(input, &prompt, access);
        let placement = self.control_placement(
            input.placement,
            input.beside_session_id,
            beside,
            &project,
            cx,
        )?;
        let request = caller.request_key();
        let origin = caller.caller.origin();
        let reveal = input.reveal.unwrap_or(false);
        let monos = self.monos.clone();
        let store = self.store.clone();
        let launch = {
            let origin = origin.clone();
            let project = project.clone();
            move |settings| Launch {
                fingerprint: app::fingerprint("session_start", &options),
                project,
                kind,
                source_provider,
                settings,
                input: options,
                prompt,
                origin,
                request,
            }
        };
        let work = cx.background_executor().spawn(async move {
            let settings = match start {
                Start::Inherited(settings) => *settings,
                Start::New(new_chat) => new_chat.settings(&*locked(&store)?),
            };
            launch_session(&store, launch(settings))
        });
        Ok(Exec::Later(cx.spawn(async move |this, cx| {
            let launched = work.await.map_err(launch_error)?;
            let settings = launched.settings.clone();
            let out = SessionStartOutput {
                session_id: launched.summary.id,
                project,
                harness: kind.id().into(),
                model: settings.model.clone(),
                effort: settings.effort.clone(),
                access: settings.access.into(),
                worktree_cwd: settings.worktree_cwd.clone(),
                submitted: !draft,
                draft,
            };
            let closed = |_| ControlError::internal("The window closed");
            match (launched.fresh, origin, monos) {
                // A repeat finds the session its first call installed.
                (false, ..) => {}
                // The Mono records and installs it, updating the window itself.
                (true, Origin::Mono { caller, mono }, Some(monos)) => {
                    let launched = crate::monos::control::Launched {
                        mono,
                        caller,
                        summary: launched.summary,
                        settings: launched.settings,
                        draft,
                        notify,
                        reveal,
                        placement,
                    };
                    let handle = this
                        .update_in(cx, |_, window, _| window.window_handle())
                        .map_err(closed)?;
                    handle
                        .update(cx, |_, window, cx| {
                            monos.update(cx, |m, cx| m.control_launched(launched, window, cx));
                        })
                        .map_err(closed)?;
                }
                (true, origin, _) => {
                    this.update_in(cx, |this, window, cx| {
                        this.install_launch(
                            launched,
                            origin,
                            (draft, reveal),
                            placement,
                            window,
                            cx,
                        );
                    })
                    .map_err(closed)?;
                }
            }
            output(&out)
        })))
    }

    pub(super) fn control_session_send(
        &mut self,
        input: SessionSendInput,
        caller: &CallerKind,
        cx: &mut Context<Self>,
    ) -> Result<SessionSendOutput, ControlError> {
        let notify = input.notify_on_complete == Some(true);
        let mono = match caller {
            CallerKind::Mono { session, mono, .. } => Some((*session, mono.clone())),
            CallerKind::Session(_) | CallerKind::External(_) => None,
        };
        if notify && mono.is_none() {
            return Err(ControlError::invalid_input(MONO_NOTIFY));
        }
        let id = input.session_id;
        if input.if_idle == Some(true) {
            // The old send: only into an idle chat with an empty composer,
            // which a chat that is not loaded yet gets once it has loaded
            // (`submit_external`). A session's also needs the saved draft
            // empty, so the person's draft stays theirs; a Mono's never
            // looked at it.
            let busy = self.session_metadata(id, cx).busy;
            let view = self.control_view(id, cx)?;
            let view = view.read(cx);
            if busy || !view.idle_for_external(cx) || (mono.is_none() && view.has_saved_draft()) {
                return Err(ControlError::new(ErrorCode::SessionBusy, NOT_IDLE));
            }
        }
        let prompt = prompt(&input.prompt)?;
        let steer = input.mode == Some(SendMode::Steer);
        let kind = Kind::from_id(&self.session_metadata(id, cx).summary.provider);
        if steer && !kind.can_steer() {
            return Err(ControlError::invalid_input(format!(
                "{} cannot take a message into a running turn; send with mode queue.",
                kind.label()
            )));
        }
        let view = self.control_view(id, cx)?;
        let (delivery, queue_length) = view.update(cx, |view, cx| {
            let queued = view.external_queues(cx);
            view.submit_external(prompt, Vec::new(), Some(steer), cx);
            let delivery = match (queued, steer) {
                (false, _) => Delivery::Started,
                (true, false) => Delivery::Queued,
                (true, true) => Delivery::Steered,
            };
            (delivery, view.queue_length())
        });
        // A Mono that asks to hear back gets a report once the session is
        // done, as for a session it started.
        if let (Some((caller, mono)), true, Some(monos)) = (mono, notify, self.monos.clone()) {
            let view = view.read(cx);
            let messaged = crate::monos::control::Messaged {
                mono,
                caller,
                session: id,
                cwd: view.summary.cwd.clone(),
                title: view.summary.title.clone(),
                harness: view.summary.provider.clone(),
                model: view.mono_model(),
            };
            monos.update(cx, |m, cx| m.control_rearm(messaged, cx));
        }
        Ok(SessionSendOutput {
            session_id: id,
            delivery,
            queue_length: u32::try_from(queue_length).unwrap_or(u32::MAX),
        })
    }

    pub(super) fn control_session_stop(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Result<SessionStopOutput, ControlError> {
        let was_busy = self.session_metadata(id, cx).busy;
        // As the old stop did, a chat that is not loaded is loaded to stop
        // it, so its saved queue is paused before it would run.
        let view = self.control_view(id, cx)?;
        view.update(cx, |view, cx| view.stop_app_session(cx));
        Ok(SessionStopOutput {
            session_id: id,
            was_busy,
            status: self.control_status_of(id, cx),
        })
    }

    pub(super) fn control_session_wait(
        &mut self,
        input: &SessionWaitInput,
        caller: &CallerKind,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let id = input.session_id;
        let timeout = input.timeout_ms.unwrap_or(DEFAULT_WAIT_MS);
        if timeout > MAX_WAIT_MS {
            return Err(ControlError::invalid_input(format!(
                "timeoutMs must be at most {MAX_WAIT_MS}; call again to keep waiting."
            )));
        }
        self.control_readable_for(caller, id, cx)?;
        let Some(view) = self.sessions.get(&id).cloned() else {
            return self.control_wait_unloaded(id, cx);
        };
        if timeout == 0 || view.read(cx).control_settled() {
            return ready(self.control_waited(id, true, caller, cx));
        }
        // Whichever comes first: the chat settling, or the time running out.
        let (done, finished) = async_channel::bounded::<bool>(2);
        let settled = done.clone();
        let watch = cx.observe(&view, move |_, view, cx| {
            if view.read(cx).control_settled() {
                let _ = settled.try_send(false);
            }
        });
        let timer = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(u64::from(timeout)));
        let clock = cx.background_executor().spawn(async move {
            timer.await;
            let _ = done.try_send(true);
        });
        let caller = caller.clone();
        Ok(Exec::Later(cx.spawn(async move |this, cx| {
            let timed_out = finished.recv().await.unwrap_or(true);
            drop((watch, clock));
            this.update(cx, |this, cx| {
                this.control_waited(id, timed_out, &caller, cx)
            })
            .map_err(|_| ControlError::internal("The window closed"))?
            .and_then(|o| output(&o))
        })))
    }

    /// How a wait on a loaded chat ended. A chat that settled as the time ran
    /// out did not time out.
    fn control_waited(
        &self,
        id: i64,
        timed_out: bool,
        caller: &CallerKind,
        cx: &App,
    ) -> Result<SessionWaitOutput, ControlError> {
        self.control_readable_for(caller, id, cx)?;
        let view = self.sessions.get(&id).map(|v| v.read(cx));
        Ok(SessionWaitOutput {
            session_id: id,
            status: self.control_status_of(id, cx),
            status_text: view.map_or_else(|| "Ready".into(), Conversation::status_text),
            timed_out: timed_out && !view.is_some_and(Conversation::control_settled),
            reply: view.and_then(|v| v.control_reply(cx)),
        })
    }

    /// A chat that is not loaded runs nothing here: answer at once, with its
    /// last reply, read from the store's rows since its last prompt.
    fn control_wait_unloaded(&self, id: i64, cx: &mut Context<Self>) -> Result<Exec, ControlError> {
        if self.session_metadata(id, cx).busy {
            // A remote host's turn is not observable here.
            return Err(remote_unsupported());
        }
        let store = self.store.clone();
        let status = self.control_status_of(id, cx);
        Ok(Exec::Later(cx.background_executor().spawn(async move {
            let rows = locked(&store)
                .and_then(|s| s.last_turn_rows(id))
                .map_err(ControlError::internal)?;
            output(&SessionWaitOutput {
                session_id: id,
                status,
                status_text: "Ready".into(),
                timed_out: false,
                reply: crate::conversation::control_reply(&rows),
            })
        })))
    }
}
