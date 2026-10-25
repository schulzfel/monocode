//! `queue_list`, `queue_remove`, `queue_steer`, `queue_resume` and
//! `queue_edit`, through the queue card's own actions on the chat.
use super::dispatch::prompt;
use super::*;
use crate::conversation::QueueRefused;
use monocode_control::ops::queue::{
    MAX_QUEUED_TEXT_CHARS, QueueEditInput, QueueItemInput, QueueListOutput, QueueOutput,
    QueuedMessage,
};

fn refused(id: i64, message: &str, refused: &QueueRefused) -> ControlError {
    match refused {
        QueueRefused::Unknown => ControlError::new(
            ErrorCode::NotFound,
            format!("Session {id} has no queued message {message}; see queue_list."),
        ),
        QueueRefused::Sending => ControlError::new(
            ErrorCode::SessionBusy,
            format!("Message {message} is being sent now."),
        ),
        QueueRefused::Editing => ControlError::new(
            ErrorCode::SessionBusy,
            format!("The person is editing message {message} in MonoCode."),
        ),
        QueueRefused::CannotSteer => ControlError::invalid_input(format!(
            "Message {message} runs in its own mode and cannot join the running turn."
        )),
        QueueRefused::Empty => {
            ControlError::invalid_input("A queued message without attachments needs text.")
        }
    }
}

impl Shell {
    fn control_queue_state(&self, id: i64, view: &Entity<Conversation>, cx: &App) -> QueueOutput {
        let (messages, paused, _) = view.read(cx).queued();
        QueueOutput {
            session_id: id,
            queue_length: u32::try_from(messages.len()).unwrap_or(u32::MAX),
            paused,
        }
    }

    pub(super) fn control_queue_list(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Result<QueueListOutput, ControlError> {
        self.control_session(id, cx)?;
        let Some(view) = self.sessions.get(&id) else {
            // A chat that is not open sends nothing; its saved queue waits.
            return Ok(QueueListOutput {
                session_id: id,
                paused: false,
                messages: Vec::new(),
            });
        };
        let (messages, paused, sending) = view.read(cx).queued();
        Ok(QueueListOutput {
            session_id: id,
            paused,
            messages: messages
                .iter()
                .map(|m| {
                    let label = m.label();
                    QueuedMessage {
                        id: m.id.clone(),
                        truncated: label.chars().count() > MAX_QUEUED_TEXT_CHARS,
                        text: label.chars().take(MAX_QUEUED_TEXT_CHARS).collect(),
                        attachments: u32::try_from(m.attachments.len()).unwrap_or(u32::MAX),
                        mode: m.mode.clone(),
                        steer: m.steer,
                        sending: sending == Some(m.id.as_str()),
                        error: m.error.clone(),
                    }
                })
                .collect(),
        })
    }

    pub(super) fn control_queue_remove(
        &mut self,
        input: &QueueItemInput,
        cx: &mut Context<Self>,
    ) -> Result<QueueOutput, ControlError> {
        let id = input.session_id;
        let view = self.control_view(id, cx)?;
        view.update(cx, |v, cx| v.queue_remove(&input.message_id, cx))
            .map_err(|r| refused(id, &input.message_id, &r))?;
        Ok(self.control_queue_state(id, &view, cx))
    }

    pub(super) fn control_queue_steer(
        &mut self,
        input: &QueueItemInput,
        cx: &mut Context<Self>,
    ) -> Result<QueueOutput, ControlError> {
        let id = input.session_id;
        let kind = Kind::from_id(&self.session_metadata(id, cx).summary.provider);
        if !kind.can_steer() {
            return Err(ControlError::invalid_input(format!(
                "{} cannot take a message into a running turn.",
                kind.label()
            )));
        }
        let view = self.control_view(id, cx)?;
        view.update(cx, |v, cx| v.queue_steer(&input.message_id, cx))
            .map_err(|r| refused(id, &input.message_id, &r))?;
        Ok(self.control_queue_state(id, &view, cx))
    }

    pub(super) fn control_queue_resume(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Result<QueueOutput, ControlError> {
        let view = self.control_view(id, cx)?;
        view.update(cx, |v, cx| v.queue_resume(cx));
        Ok(self.control_queue_state(id, &view, cx))
    }

    pub(super) fn control_queue_edit(
        &mut self,
        input: QueueEditInput,
        cx: &mut Context<Self>,
    ) -> Result<QueueOutput, ControlError> {
        let id = input.session_id;
        let text = if input.text.is_empty() {
            input.text
        } else {
            prompt(&input.text)?
        };
        let view = self.control_view(id, cx)?;
        view.update(cx, |v, cx| v.queue_edit(&input.message_id, text, cx))
            .map_err(|r| refused(id, &input.message_id, &r))?;
        Ok(self.control_queue_state(id, &view, cx))
    }
}
