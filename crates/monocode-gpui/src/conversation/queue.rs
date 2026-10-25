//! The follow-up queue's actions: remove, steer, resume and edit a queued
//! message. The queue card's buttons and external control both act through
//! these, so a change from either saves, pumps and redraws the same way.
use super::*;
use monocode_core::follow_up::Message;

/// Why a queue action was not taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum QueueRefused {
    /// No queued message has this id.
    Unknown,
    /// It is being sent now.
    Sending,
    /// The person is editing it.
    Editing,
    /// It runs in a mode that cannot join the running turn.
    CannotSteer,
    /// The new text would leave it empty.
    Empty,
}

impl Conversation {
    /// Checks that queued message `id` exists and is free to change.
    fn queued_for_change(&self, id: &str) -> Result<(), QueueRefused> {
        if self.follow_ups.sending.as_deref() == Some(id) {
            return Err(QueueRefused::Sending);
        }
        if self
            .follow_ups
            .edit
            .as_ref()
            .is_some_and(|edit| edit.id == id)
        {
            return Err(QueueRefused::Editing);
        }
        if !self.follow_ups.state.messages.iter().any(|m| m.id == id) {
            return Err(QueueRefused::Unknown);
        }
        Ok(())
    }

    /// Discards queued message `id` unsent.
    pub(crate) fn queue_remove(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), QueueRefused> {
        self.queued_for_change(id)?;
        self.follow_ups.state.messages.retain(|m| m.id != id);
        self.follow_ups.continuations.retain(|c| c != id);
        if self.follow_ups.requested.as_deref() == Some(id) {
            self.follow_ups.requested = self.follow_ups.continuations.front().cloned();
        }
        self.save_follow_ups(cx);
        Ok(())
    }

    /// Marks queued message `id` to go into the running turn now.
    pub(crate) fn queue_steer(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), QueueRefused> {
        self.queued_for_change(id)?;
        let busy = self.busy;
        let Some(message) = self
            .follow_ups
            .state
            .messages
            .iter_mut()
            .find(|m| m.id == id)
        else {
            return Err(QueueRefused::Unknown);
        };
        if !message.can_steer() && busy {
            return Err(QueueRefused::CannotSteer);
        }
        message.steer = true;
        message.error = None;
        self.follow_ups.requested = Some(id.to_owned());
        self.save_follow_ups(cx);
        Ok(())
    }

    /// Releases a paused queue: the interrupted turn continues first, then
    /// the queued messages follow, as App.tsx's "resuming" status did.
    pub(crate) fn queue_resume(&mut self, cx: &mut Context<Self>) {
        if self.stopping {
            self.defer_history_action(|this, cx| this.queue_resume(cx), cx);
            return;
        }
        if self.busy || !self.follow_ups.state.paused || self.follow_ups.state.messages.is_empty() {
            return;
        }
        self.follow_ups.resuming = true;
        self.launch_prompt("Continue from where you left off.".into(), cx);
        if !self.busy {
            self.follow_ups.resuming = false;
        }
        cx.notify();
    }

    /// Replaces queued message `id`'s text, keeping its MCP context while
    /// its tokens stay. False when that would leave it empty.
    pub(super) fn replace_queued_text(&mut self, id: &str, text: String, cx: &gpui::App) -> bool {
        let prepared = self.completion.read(cx).prepare_prompt(&text);
        if let Some(message) = self
            .follow_ups
            .state
            .messages
            .iter_mut()
            .find(|m| m.id == id)
        {
            if text.trim().is_empty() && message.attachments.is_empty() {
                return false;
            }
            message.replace_text(text, prepared);
            message.error = None;
        }
        true
    }

    /// Changes queued message `id`'s text and saves the queue.
    pub(crate) fn queue_edit(
        &mut self,
        id: &str,
        text: String,
        cx: &mut Context<Self>,
    ) -> Result<(), QueueRefused> {
        self.queued_for_change(id)?;
        if !self.replace_queued_text(id, text, cx) {
            return Err(QueueRefused::Empty);
        }
        self.save_follow_ups(cx);
        Ok(())
    }

    /// The queued messages in sending order, whether the queue is paused,
    /// and the one being sent.
    pub(crate) fn queued(&self) -> (&[Message], bool, Option<&str>) {
        (
            &self.follow_ups.state.messages,
            self.follow_ups.state.paused && !self.follow_ups.resuming,
            self.follow_ups.sending.as_deref(),
        )
    }
}

#[cfg(test)]
impl Conversation {
    /// Pauses the queue as stopping a turn does. The continuation a resume
    /// starts waits for history and never reaches a provider.
    pub(crate) fn test_pause_queue(&mut self, cx: &mut Context<Self>) {
        self.busy = false;
        self.follow_ups.state.paused = true;
        self.history_unloaded = true;
        self.account_switching = true;
        cx.notify();
    }
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
