//! `question_list` and `question_answer`. Answers go through
//! `Conversation::answer_question`, the question card's own path. Approvals
//! are listed only: no operation decides them.
use super::*;
use crate::conversation::AnswerRefused;
use monocode_control::ops::{
    self,
    question::{
        DEFAULT_QUESTIONS_PER_PAGE, ItemAnswer, MAX_APPROVAL_DETAIL_CHARS, MAX_APPROVALS,
        MAX_QUESTIONS_PER_PAGE, PendingApproval, PendingQuestion, QuestionAnswerInput,
        QuestionAnswerOutput, QuestionChoice, QuestionItem, QuestionListInput, QuestionListOutput,
    },
};
use monocode_core::agent::Question;

fn pending(session_id: i64, ix: usize, question: Question) -> PendingQuestion {
    PendingQuestion {
        session_id,
        question_id: question.id,
        title: question.title,
        items: question
            .items
            .into_iter()
            .map(|item| QuestionItem {
                id: item.id,
                prompt: item.prompt,
                multiple: item.multiple,
                allow_custom: item.allow_custom,
                options: item
                    .options
                    .into_iter()
                    .map(|o| QuestionChoice {
                        id: o.id,
                        label: o.label,
                        description: o.description,
                    })
                    .collect(),
            })
            .collect(),
        answerable: ix == 0,
        auto_resolve_at: question.auto_resolve_at,
    }
}

/// The card's reply for `given`: one list per item of picked choice labels
/// and typed text, a typed answer replacing a single choice. `None` when
/// nothing was answered, which declines the question.
pub(in crate::shell::control) fn card_answers(
    question: &Question,
    given: &[ItemAnswer],
) -> Result<Option<Vec<Vec<String>>>, ControlError> {
    let mut answers = vec![Vec::new(); question.items.len()];
    let mut seen = HashSet::new();
    for answer in given {
        let Some(ix) = question.items.iter().position(|i| i.id == answer.item_id) else {
            return Err(ControlError::invalid_input(format!(
                "Question {} has no item {}",
                question.id, answer.item_id
            )));
        };
        if !seen.insert(ix) {
            return Err(ControlError::invalid_input(format!(
                "Item {} is answered twice",
                answer.item_id
            )));
        }
        let item = &question.items[ix];
        let mut picked = Vec::new();
        for value in &answer.options {
            let choice = item
                .options
                .iter()
                .find(|o| o.id == *value || o.label == *value)
                .ok_or_else(|| {
                    ControlError::invalid_input(format!("Item {} has no choice {value}", item.id))
                })?;
            if !picked.contains(&choice.label) {
                picked.push(choice.label.clone());
            }
        }
        if !item.multiple && picked.len() > 1 {
            return Err(ControlError::invalid_input(format!(
                "Item {} takes one choice",
                item.id
            )));
        }
        if let Some(text) = answer
            .text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            if !item.allow_custom {
                return Err(ControlError::invalid_input(format!(
                    "Item {} takes only its choices, not typed text",
                    item.id
                )));
            }
            if !item.multiple {
                picked.clear();
            }
            picked.push(text.to_owned());
        }
        answers[ix] = picked;
    }
    Ok(answers.iter().any(|a| !a.is_empty()).then_some(answers))
}

impl Shell {
    pub(super) fn control_question_list(
        &self,
        input: &QuestionListInput,
        cx: &App,
    ) -> Result<QuestionListOutput, ControlError> {
        let limit = ops::limit(
            input.limit,
            DEFAULT_QUESTIONS_PER_PAGE,
            MAX_QUESTIONS_PER_PAGE,
        )?;
        let project = input
            .project
            .as_deref()
            .map(|p| self.control_project(Some(p)))
            .transpose()?;
        if let Some(id) = input.session_id {
            self.control_session(id, cx)?;
        }
        let mut ids: Vec<i64> = self.sessions.keys().copied().collect();
        ids.sort_unstable();
        let mut questions = Vec::new();
        let mut approvals = Vec::new();
        for id in ids {
            if input.session_id.is_some_and(|wanted| wanted != id)
                || self.control_session(id, cx).is_err()
                || project.as_ref().is_some_and(|p| {
                    monocode_core::files::path_key(&self.session_metadata(id, cx).summary.cwd)
                        != monocode_core::files::path_key(p)
                })
            {
                continue;
            }
            let view = self.sessions[&id].read(cx);
            for (ix, question) in view.open_questions(cx).into_iter().enumerate() {
                questions.push(pending(id, ix, question));
            }
            approvals.extend(view.open_approvals().map(|a| PendingApproval {
                session_id: id,
                approval_id: a.id.clone(),
                title: a.title.clone(),
                detail: a.detail.chars().take(MAX_APPROVAL_DETAIL_CHARS).collect(),
            }));
        }
        approvals.truncate(MAX_APPROVALS);
        let (questions, next_cursor) = ops::page(&questions, input.cursor.as_deref(), limit)?;
        Ok(QuestionListOutput {
            questions,
            next_cursor,
            approvals,
        })
    }

    pub(super) fn control_question_answer(
        &mut self,
        input: &QuestionAnswerInput,
        cx: &mut Context<Self>,
    ) -> Result<QuestionAnswerOutput, ControlError> {
        let id = input.session_id;
        let missing = || {
            ControlError::new(
                ErrorCode::NotFound,
                format!(
                    "Session {id} has no open question {}; see question_list.",
                    input.question_id
                ),
            )
        };
        let view = self.sessions.get(&id).cloned().ok_or_else(missing)?;
        let question = view
            .read(cx)
            .open_questions(cx)
            .into_iter()
            .find(|q| q.id == input.question_id)
            .ok_or_else(missing)?;
        let answers = card_answers(&question, &input.answers)?;
        let declined = answers.is_none();
        view.update(cx, |view, cx| {
            view.answer_question(&input.question_id, answers, cx)
        })
        .map_err(|refused| match refused {
            AnswerRefused::Unknown => missing(),
            AnswerRefused::NotFirst(first) => ControlError::invalid_input(format!(
                "Session {id} asks question {first} first; answer that one before {}.",
                input.question_id
            )),
            AnswerRefused::Gone => ControlError::new(
                ErrorCode::NotFound,
                "The provider is no longer waiting for this answer.",
            ),
        })?;
        Ok(QuestionAnswerOutput {
            session_id: id,
            question_id: input.question_id.clone(),
            declined,
            status: view.read(cx).control_status(),
        })
    }
}
