//! Answering the question a chat waits on. The question card and external
//! control both answer through [`Conversation::answer_question`], so an
//! answer from either releases the card and settles the status the same way.
use super::*;
use monocode_core::agent::Question;

/// Why an answer was not delivered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AnswerRefused {
    /// No open question has this id.
    Unknown,
    /// The question is open, but this one comes first.
    NotFirst(String),
    /// The provider is no longer waiting; the chat shows the error.
    Gone,
}

impl Conversation {
    /// Shows the provider's question as a card over the composer. The card's
    /// reply comes back through [`Self::answer_question`].
    pub(super) fn ask_question(&mut self, request: Question, cx: &mut Context<Self>) {
        let card = cx.new(|cx| crate::questions::QuestionCard::new(request, cx));
        let events = cx.subscribe(&card, |this, _, reply: &crate::questions::Answered, cx| {
            let _ = this.answer_question(&reply.id, reply.answers.clone(), cx);
        });
        let keep_open = cx.subscribe(
            &card,
            |this, _, request: &crate::questions::KeptOpen, cx| {
                if let Some(controls) = &this.controls {
                    let _ = controls.send(Control {
                        id: request.id.clone(),
                        decision: Decision::KeepOpen,
                    });
                }
                cx.emit(SessionChanged);
                cx.notify();
            },
        );
        self.questions.push_back((card, vec![events, keep_open]));
        self.status = SessionStatus::AwaitingAnswer;
    }

    /// Answers the chat's first open question `id`: `Some` with one list of
    /// choices or typed text per item, `None` to decline it. The provider
    /// gets the decision, the card leaves, and the status moves on.
    pub(crate) fn answer_question(
        &mut self,
        id: &str,
        answers: Option<Vec<Vec<String>>>,
        cx: &mut Context<Self>,
    ) -> Result<(), AnswerRefused> {
        if self
            .questions
            .front()
            .is_none_or(|(card, _)| card.read(cx).id() != id)
        {
            return Err(
                match self
                    .questions
                    .iter()
                    .any(|(card, _)| card.read(cx).id() == id)
                {
                    true => AnswerRefused::NotFirst(
                        self.questions
                            .front()
                            .map(|(card, _)| card.read(cx).id().to_owned())
                            .unwrap_or_default(),
                    ),
                    false => AnswerRefused::Unknown,
                },
            );
        }
        let before = self.needs_approval();
        let decision = answers.map_or(Decision::Deny, Decision::Answer);
        if self.controls.as_ref().is_none_or(|controls| {
            controls
                .send(Control {
                    id: id.to_owned(),
                    decision,
                })
                .is_err()
        }) {
            self.status = SessionStatus::error("The provider is no longer waiting for this answer");
            cx.notify();
            return Err(AnswerRefused::Gone);
        }
        if let Some((card, _)) = self.questions.pop_front() {
            self.question_overlay.release(card, cx);
        }
        self.status = if !self.approvals.is_empty() {
            SessionStatus::AwaitingApproval
        } else if !self.questions.is_empty() {
            SessionStatus::AwaitingAnswer
        } else {
            SessionStatus::Working(None)
        };
        if before != self.needs_approval() {
            cx.emit(SessionChanged);
        }
        cx.notify();
        Ok(())
    }

    /// The open questions, the one to answer first first.
    pub(crate) fn open_questions(&self, cx: &gpui::App) -> Vec<Question> {
        self.questions
            .iter()
            .map(|(card, _)| card.read(cx).question().clone())
            .collect()
    }

    /// The tool approvals waiting for the person, oldest first.
    pub(crate) fn open_approvals(&self) -> impl Iterator<Item = &Approval> {
        self.approvals.iter()
    }
}

#[cfg(test)]
impl Conversation {
    /// Puts `question` in front of the chat as a running turn would, and
    /// returns what the provider would receive.
    pub(crate) fn test_ask(
        &mut self,
        question: Question,
        cx: &mut Context<Self>,
    ) -> std::sync::mpsc::Receiver<Control> {
        let (controls, received) = std::sync::mpsc::channel();
        self.controls = Some(controls);
        self.busy = true;
        self.ask_question(question, cx);
        received
    }
    /// Another question behind the first, as a turn would ask it.
    pub(crate) fn test_ask_more(&mut self, question: Question, cx: &mut Context<Self>) {
        self.ask_question(question, cx);
    }
    /// A tool approval waiting for the person.
    pub(crate) fn test_approval(&mut self, approval: Approval, cx: &mut Context<Self>) {
        self.approvals.push_back(approval);
        self.status = SessionStatus::AwaitingApproval;
        cx.notify();
    }
}

#[cfg(test)]
#[path = "answers_tests.rs"]
mod tests;
