//! Questions agents ask the person, and the approvals they wait on.
//! Questions can be answered here; tool-permission approvals cannot, by
//! design: the person decides those in the app.
use crate::{Hints, Operation, Scope, ops::session::SessionStatus};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Questions per page unless `limit` says otherwise.
pub const DEFAULT_QUESTIONS_PER_PAGE: u32 = 20;
/// The largest `limit` accepted.
pub const MAX_QUESTIONS_PER_PAGE: u32 = 100;
/// The most approvals listed.
pub const MAX_APPROVALS: usize = 100;
/// The most characters of an approval's detail listed.
pub const MAX_APPROVAL_DETAIL_CHARS: usize = 2_000;

/// `question_list`: questions and approvals waiting for the person.
#[derive(Debug)]
pub struct QuestionList;

/// Which sessions, and paging. All open sessions when neither is given.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionListInput {
    /// Only this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<i64>,
    /// Only sessions of this project (path or key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// `nextCursor` from the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Questions per page, 1 to 100 (default 20).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One choice of a question item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuestionChoice {
    /// Its id, for `question_answer`.
    pub id: String,
    /// What the person sees.
    pub label: String,
    /// More about it, when given.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// One part of a question.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuestionItem {
    /// Its id, for `question_answer`.
    pub id: String,
    /// What is asked.
    pub prompt: String,
    /// Whether several choices may be picked.
    pub multiple: bool,
    /// Whether a typed answer is accepted.
    pub allow_custom: bool,
    /// The choices; none for a free-text item.
    pub options: Vec<QuestionChoice>,
}

/// A question waiting for an answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PendingQuestion {
    /// The session asking.
    pub session_id: i64,
    /// The question's id, for `question_answer`.
    pub question_id: String,
    /// Its title.
    pub title: String,
    /// Its items, answered together.
    pub items: Vec<QuestionItem>,
    /// Whether it is the session's first open question: only that one can
    /// be answered now.
    pub answerable: bool,
    /// When the provider stops waiting, in Unix milliseconds, if it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_resolve_at: Option<u64>,
}

/// A tool-permission approval waiting for the person in the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PendingApproval {
    /// The session waiting.
    pub session_id: i64,
    /// The approval's id.
    pub approval_id: String,
    /// What the agent wants to do.
    pub title: String,
    /// Details, at most 2,000 characters.
    pub detail: String,
}

/// What waits for the person.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuestionListOutput {
    /// One page of open questions.
    pub questions: Vec<PendingQuestion>,
    /// The cursor for the next page of questions, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Approvals blocking sessions. They cannot be answered here.
    pub approvals: Vec<PendingApproval>,
}

impl Operation for QuestionList {
    const NAME: &'static str = "question_list";
    const TITLE: &'static str = "List questions";
    const SUBJECT: &'static [&'static str] = &["sessionId", "project"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "List questions agents asked and are waiting to have answered, with their items and choices for question_answer, and the tool-permission approvals blocking sessions. Approvals are listed so you can see a session is blocked; only the person can decide them, in the app.";
    const HINTS: Hints = Hints::READ;
    type Input = QuestionListInput;
    type Output = QuestionListOutput;

    fn target_session(input: &QuestionListInput) -> Option<i64> {
        input.session_id
    }

    fn summary(output: &QuestionListOutput) -> String {
        format!(
            "{} questions{}, {} approvals waiting for the person",
            output.questions.len(),
            if output.next_cursor.is_some() {
                " (more)"
            } else {
                ""
            },
            output.approvals.len()
        )
    }
}

/// `question_answer`: answer an agent's question.
#[derive(Debug)]
pub struct QuestionAnswer;

/// The answer to one item.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemAnswer {
    /// The item's id.
    pub item_id: String,
    /// Picked choices by id or label; at most one unless the item is `multiple`.
    #[serde(default)]
    pub options: Vec<String>,
    /// A typed answer, for items that allow one. For a single-choice item
    /// it replaces the picked choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Which question, and the answers. Answering nothing declines it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionAnswerInput {
    /// The session asking.
    pub session_id: i64,
    /// The question's id from `question_list`.
    pub question_id: String,
    /// Answers by item; items left out are skipped.
    #[serde(default)]
    pub answers: Vec<ItemAnswer>,
}

/// The answered question.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuestionAnswerOutput {
    /// The session.
    pub session_id: i64,
    /// The question.
    pub question_id: String,
    /// Whether it was declined because nothing was answered.
    pub declined: bool,
    /// What the session is doing now.
    pub status: SessionStatus,
}

impl Operation for QuestionAnswer {
    const NAME: &'static str = "question_answer";
    const TITLE: &'static str = "Answer question";
    const SUBJECT: &'static [&'static str] = &["sessionId"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Answer the question a session is waiting on, as the question card in the app would: pick choices by id or label and type answers where allowed. Answering no item declines the question. Only a session's first open question can be answered. Tool-permission approvals cannot be answered over MCP.";
    const HINTS: Hints = Hints::CHANGE;
    const LOCAL_ONLY: bool = true;
    type Input = QuestionAnswerInput;
    type Output = QuestionAnswerOutput;

    fn target_session(input: &QuestionAnswerInput) -> Option<i64> {
        Some(input.session_id)
    }

    fn summary(output: &QuestionAnswerOutput) -> String {
        format!(
            "Question {} of session {} {}; session {}",
            output.question_id,
            output.session_id,
            if output.declined {
                "declined"
            } else {
                "answered"
            },
            output.status.id()
        )
    }
}
