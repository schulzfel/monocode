//! `session_start`, `session_send`, `session_wait` and `session_stop`.
//! Starting reuses Operator's launch pipeline with the client as origin, so
//! launched sessions never get Operator, plan or control capability.
use super::session::{access_or_default, remote_unsupported};
use super::*;
use crate::shell::session_launch::{Launch, launch_session};
use monocode_control::{
    AccessMode,
    ops::dispatch::{
        DEFAULT_WAIT_MS, Delivery, MAX_PROMPT_BYTES, MAX_WAIT_MS, Placement, SendMode,
        SessionSendInput, SessionSendOutput, SessionStartInput, SessionStartOutput,
        SessionStopOutput, SessionWaitInput, SessionWaitOutput, WorkspaceMode,
    },
};
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
    options
}

impl Shell {
    pub(super) fn control_session_start(
        &mut self,
        input: &SessionStartInput,
        caller: &Caller,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let prompt = prompt(&input.prompt)?;
        let project = self.control_local_project(input.project.as_deref())?;
        let kind = match &input.harness {
            Some(id) => Kind::parse(id).ok_or_else(|| {
                ControlError::invalid_input(format!(
                    "There is no harness {id}; see model_list for the providers."
                ))
            })?,
            None => self.default_kind_for(&project),
        };
        let mut new_chat = self.new_chat(kind, &project, cx);
        new_chat.access = access_or_default(input.access, cx);
        let options = options(input, &prompt, new_chat.access.into());
        let focused = self
            .workspace
            .active
            .filter(|id| self.session_summaries.contains_key(id))
            .filter(|id| self.session_metadata(*id, cx).summary.cwd == project);
        let placement = self
            .app_placement(&options, focused, &project, cx)
            .map_err(ControlError::invalid_input)?;
        let request = caller.request_key();
        let origin = caller.caller.origin();
        let draft = input.draft.unwrap_or(false);
        let reveal = input.reveal.unwrap_or(false);
        let store = self.store.clone();
        let launch = {
            let origin = origin.clone();
            let project = project.clone();
            move |settings| Launch {
                fingerprint: app::fingerprint("session_start", &options),
                project,
                kind,
                source_provider: kind.id().into(),
                settings,
                input: options,
                prompt,
                origin,
                request,
            }
        };
        let work = cx.background_executor().spawn(async move {
            let settings = new_chat.settings(&*locked(&store)?);
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
            if launched.fresh {
                this.update_in(cx, |this, window, cx| {
                    this.install_launch(launched, origin, (draft, reveal), placement, window, cx);
                })
                .map_err(|_| ControlError::internal("The window closed"))?;
            }
            output(&out)
        })))
    }

    pub(super) fn control_session_send(
        &mut self,
        input: SessionSendInput,
        cx: &mut Context<Self>,
    ) -> Result<SessionSendOutput, ControlError> {
        let prompt = prompt(&input.prompt)?;
        let id = input.session_id;
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
        if let Some(view) = self.sessions.get(&id).cloned() {
            view.update(cx, |view, cx| view.stop_app_session(cx));
        }
        Ok(SessionStopOutput {
            session_id: id,
            was_busy,
            status: self.control_status_of(id, cx),
        })
    }

    pub(super) fn control_session_wait(
        &mut self,
        input: &SessionWaitInput,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let id = input.session_id;
        let timeout = input.timeout_ms.unwrap_or(DEFAULT_WAIT_MS);
        if timeout > MAX_WAIT_MS {
            return Err(ControlError::invalid_input(format!(
                "timeoutMs must be at most {MAX_WAIT_MS}; call again to keep waiting."
            )));
        }
        self.control_session(id, cx)?;
        let Some(view) = self.sessions.get(&id).cloned() else {
            return self.control_wait_unloaded(id, cx);
        };
        if timeout == 0 || view.read(cx).control_settled() {
            return ready(self.control_waited(id, true, cx));
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
        Ok(Exec::Later(cx.spawn(async move |this, cx| {
            let timed_out = finished.recv().await.unwrap_or(true);
            drop((watch, clock));
            this.update(cx, |this, cx| this.control_waited(id, timed_out, cx))
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
        cx: &App,
    ) -> Result<SessionWaitOutput, ControlError> {
        self.control_session(id, cx)?;
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
