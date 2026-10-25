//! What external control reads from a chat, and what it changes. Messages
//! from a control client go through `submit_external`, the path a paired
//! phone uses; `session_configure` changes only this session's settings.
use super::*;
use monocode_control::{
    ControlError, ErrorCode,
    ops::{
        dispatch::MAX_REPLY_CHARS,
        session::{SessionStatus as Status, Text},
    },
};

impl Conversation {
    /// The chat's status for control clients. Pending approvals and questions
    /// win over the status line, and a running turn is working whatever its
    /// status line says.
    pub(crate) fn control_status(&self) -> Status {
        if self.stopping {
            return Status::Stopping;
        }
        if !self.approvals.is_empty() {
            return Status::AwaitingApproval;
        }
        if !self.questions.is_empty() {
            return Status::AwaitingAnswer;
        }
        match &self.status {
            SessionStatus::Connecting(_) => Status::Connecting,
            SessionStatus::AwaitingApproval => Status::AwaitingApproval,
            SessionStatus::AwaitingAnswer => Status::AwaitingAnswer,
            _ if self.busy => Status::Working,
            _ if self.preparing_worktree => Status::Connecting,
            SessionStatus::Loading if self.loading => Status::Loading,
            SessionStatus::Working(_) => Status::Working,
            SessionStatus::Error { message } => Status::Error {
                message: message.clone(),
            },
            SessionStatus::Loading | SessionStatus::Idle | SessionStatus::Notice(_) => Status::Idle,
        }
    }
    /// The status line the chat shows.
    pub(crate) fn status_text(&self) -> String {
        self.status.to_string()
    }
    /// Whether a waiting client may return: the chat needs the person, or it
    /// is idle or failed with nothing about to run (`external_outcome`'s rule).
    pub(crate) fn control_settled(&self) -> bool {
        let status = self.control_status();
        matches!(status, Status::AwaitingApproval | Status::AwaitingAnswer)
            || (status.settled() && !self.external_submission_pending())
    }
    /// The agent's prose since the person's last message, cut to
    /// `MAX_REPLY_CHARS`, when there is any.
    pub(crate) fn control_reply(&self, cx: &gpui::App) -> Option<Text> {
        reply(self.transcript.read(cx).source_rows())
    }
    /// Messages queued after the current turn.
    pub(crate) fn queue_length(&self) -> usize {
        self.follow_ups.state.messages.len()
    }
    /// The context window use, when known.
    pub(crate) fn context_usage(&self) -> Option<monocode_core::context::Usage> {
        self.context
    }
    /// The model the next turn uses, when known.
    pub(crate) fn current_model(&self) -> Option<String> {
        self.settings
            .model
            .clone()
            .or_else(|| self.resolved_model.clone())
    }
    /// Starts compacting as `/compact` does, or says why it cannot start.
    pub(crate) fn control_compact(&mut self, cx: &mut Context<Self>) -> Result<(), ControlError> {
        use super::commands::CompactBlock;
        match self.compact_block(cx) {
            None => {
                self.compact_context(cx);
                Ok(())
            }
            Some(CompactBlock::Busy(message)) => {
                Err(ControlError::new(ErrorCode::SessionBusy, message))
            }
            Some(CompactBlock::Quiet) => Err(ControlError::new(
                ErrorCode::SessionNotIdle,
                "The session is waiting on the person or changing its settings; try again once it is idle.",
            )),
            Some(CompactBlock::Unsupported(message) | CompactBlock::Failed(message)) => {
                Err(ControlError::invalid_input(message))
            }
        }
    }
    /// `session_configure`: the model, its settings and the access mode of
    /// this session only. A pick in the window also makes it the default for
    /// new sessions; a client's never does (no recent model, effort or
    /// access is remembered).
    pub(crate) fn control_configure(
        &mut self,
        model: Option<(Kind, Option<String>)>,
        model_settings: &std::collections::HashMap<String, String>,
        access: Option<AccessMode>,
        cx: &mut Context<Self>,
    ) {
        if let Some((kind, model)) = model
            && (self.summary.provider != kind.id() || self.model != model)
        {
            self.apply_model_pick(&ModelPicked(kind, model), cx);
        }
        for (key, value) in model_settings {
            match key.as_str() {
                "fast" => self.settings.fast = value == "true",
                "thinking" => self.settings.thinking = Some(value == "true"),
                key if is_effort_key(key) => self.set_effort(value),
                _ => {
                    self.settings
                        .model_settings
                        .insert(key.clone(), value.clone());
                }
            }
        }
        if let Some(access) = access {
            self.settings.access = access;
        }
        self.save_settings(cx);
        cx.emit(SessionChanged);
        cx.notify();
    }
    /// The last response handed to `target`, as the response's Hand off
    /// action builds it, once history is loaded.
    pub(crate) fn control_handoff(
        &mut self,
        target: monocode_core::response::Target,
        access: monocode_core::agent::AccessMode,
        done: impl FnOnce(Result<Transfer, ControlError>) + 'static,
        cx: &mut Context<Self>,
    ) {
        if !self.history_ready() {
            self.defer_history_action(move |this, cx| done(this.handoff(target, access, cx)), cx);
            return;
        }
        done(self.handoff(target, access, cx));
    }
    fn handoff(
        &self,
        target: monocode_core::response::Target,
        access: monocode_core::agent::AccessMode,
        cx: &gpui::App,
    ) -> Result<Transfer, ControlError> {
        let transcript = self.transcript.read(cx);
        let turn = transcript
            .history_rows()
            .iter()
            .rposition(|row| !row.is_prompt())
            .ok_or_else(|| {
                ControlError::invalid_input(format!(
                    "Session {} has no response to hand off yet.",
                    self.summary.id
                ))
            })?;
        let from = transcript
            .turns
            .get(&turn)
            .map(|t| t.provider.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.summary.provider)
            .to_owned();
        let (rows, through) = transcript.response_rows(turn);
        let mut settings = self.settings.clone();
        settings.access = access;
        Ok(Transfer {
            kind: monocode_core::response::TransferKind::Handoff,
            target,
            from,
            turn: rows,
            through,
            cwd: self.summary.cwd.clone(),
            title: self.summary.title.clone(),
            settings,
        })
    }
}

/// The agent's prose after the last user message in `rows`.
pub(crate) fn reply(rows: &[monocode_core::transcript::Row]) -> Option<Text> {
    let start = rows.iter().rposition(|row| row.role == "user").unwrap_or(0);
    let text = monocode_core::response::prose(&rows[start..]);
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let truncated = text.chars().count() > MAX_REPLY_CHARS;
    Some(Text {
        text: text.chars().take(MAX_REPLY_CHARS).collect(),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use monocode_core::store::Store;
    use std::sync::Mutex;

    /// R1: automation outcomes read the typed status exactly as they read
    /// the status text, and control clients see the same state.
    #[gpui::test]
    fn outcomes_and_control_state_follow_the_typed_status(cx: &mut TestAppContext) {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store.create("codex", "/work/project").unwrap();
        let window = cx.update(|cx| {
            input::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view
                })
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                view.status = SessionStatus::Idle;
                assert_eq!(view.external_outcome(cx), (true, "Ready\n".into()));
                assert_eq!(view.control_status(), Status::Idle);
                assert!(view.control_settled());
                let failed = "Codex exited with status 1";
                view.status = SessionStatus::error(failed);
                assert_eq!(view.external_outcome(cx), (false, format!("{failed}\n")));
                assert_eq!(
                    view.control_status(),
                    Status::Error {
                        message: failed.into()
                    }
                );
                view.status = SessionStatus::notice("Worktree deleted");
                assert!(!view.external_outcome(cx).0, "only Ready ever succeeded");
                assert_eq!(view.control_status(), Status::Idle);
                view.busy = true;
                view.status = SessionStatus::working("Streaming…");
                assert_eq!(view.status_text(), "Streaming…");
                assert_eq!(view.control_status(), Status::Working);
                assert!(!view.control_settled());
                view.status = SessionStatus::Idle;
                assert_eq!(
                    view.control_status(),
                    Status::Working,
                    "a running turn works whatever its last status line"
                );
                view.status = SessionStatus::Connecting(Preparing::Handoff);
                assert_eq!(view.control_status(), Status::Connecting);
                view.status = SessionStatus::AwaitingAnswer;
                assert!(view.control_settled(), "a question needs the person");
            })
            .unwrap();
    }
}
