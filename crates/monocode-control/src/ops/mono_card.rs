//! Cards a Mono posts in its own chat: a pull request, a session, a few
//! replies to choose from, or a suggested habit. Only the Mono's chat and
//! habit runs call this; a habit run's cards arrive with its report.
use crate::{Hints, Operation, Scope, ops::mono_habit::HabitSchedule};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The longest note accepted, in characters.
pub const MAX_CARD_NOTE_CHARS: usize = 280;
/// The most replies a `choices` card offers.
pub const MAX_CARD_CHOICES: usize = 4;

/// A card, tagged by `type`. Each type takes only its own fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[schemars(transform = crate::schema::one_object)]
pub enum MonoCardPostInput {
    /// `pr`: a GitHub pull request, with its live title, state and size.
    Pr {
        /// For `pr`: the repository as `owner/name`; the first assigned
        /// project's repository when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repo: Option<String>,
        /// For `pr`: the pull request's number.
        number: u64,
        /// For `pr` and `session`: a note under the card (at most 280
        /// characters).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// `session`: a session in an assigned project, with its live state.
    Session {
        /// For `session`: the session's id.
        session_id: i64,
        /// For `pr` and `session`: a note under the card (at most 280
        /// characters).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// `choices`: replies the person can send with one click.
    Choices {
        /// For `choices`: 1 to 4 replies of 1 to 160 characters each.
        options: Vec<String>,
    },
    /// `habit`: a habit the person can add with one click.
    Habit {
        /// For `habit`: its name (at most 80 characters).
        name: String,
        /// For `habit`: what each run does (at most 4,000 characters).
        instructions: String,
        /// For `habit`: when it would run.
        schedule: HabitSchedule,
    },
}

/// A card's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CardType {
    /// A pull request.
    Pr,
    /// A session.
    Session,
    /// Replies to choose from.
    Choices,
    /// A suggested habit.
    Habit,
}

/// The posted card: the fields of its type, with what the app added.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonoCard {
    /// The card's id.
    pub id: String,
    /// Its type.
    #[serde(rename = "type")]
    pub card_type: CardType,
    /// `pr`: the repository, when one was named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// `pr`: the pull request's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// `session`: the session's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<i64>,
    /// `pr` and `session`: the title, from GitHub or the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `pr`: GitHub's state (`OPEN`, `MERGED`, `CLOSED`). `session`: where
    /// the session stands: `working`, `input`, `draft`, `failed`,
    /// `stopped` or `done`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// `pr`: its address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// `pr`: whether it is a draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_draft: Option<bool>,
    /// `pr`: lines added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u64>,
    /// `pr`: lines deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u64>,
    /// `pr` and `session`: the note, whitespace collapsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// `choices`: the replies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    /// `habit`: its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `habit`: its instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// `habit`: its schedule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<HabitSchedule>,
}

/// `mono_card_post`: a card in the Mono's chat.
#[derive(Debug)]
pub struct MonoCardPost;

impl Operation for MonoCardPost {
    const NAME: &'static str = "mono_card_post";
    const TITLE: &'static str = "Post card";
    const SUBJECT: &'static [&'static str] = &["name", "sessionId", "number", "options"];
    const SCOPE: Scope = Scope::Dispatch;
    const DESCRIPTION: &'static str = "Post a card in your chat, by `type`: `pr` (repo?, number, note?) shows a GitHub pull request with its live title, state and size; `session` (sessionId, note?) shows a session of an assigned project with its live state; `choices` (options) offers 1 to 4 replies the person sends with one click; `habit` (name, instructions, schedule) suggests a habit the person adds with one click. Each type takes only its own fields. A habit run's cards arrive with its report.";
    const HINTS: Hints = Hints::CHANGE;
    const MONO_ONLY: bool = true;
    type Input = MonoCardPostInput;
    type Output = MonoCard;

    fn summary(output: &MonoCard) -> String {
        let card = match output.card_type {
            CardType::Pr => "pull request",
            CardType::Session => "session",
            CardType::Choices => "choices",
            CardType::Habit => "habit",
        };
        format!("Posted a {card} card")
    }
}
