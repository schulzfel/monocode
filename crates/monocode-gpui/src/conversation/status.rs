//! What a chat is doing, as one value instead of free text. The header's
//! status line, automation results and external control all read it; its
//! display text is exactly what the status line showed before it was typed.
use std::fmt;

/// The step before a turn's provider answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Preparing {
    /// Starting the provider: "Connecting…".
    Connecting,
    /// Making the turn's new worktree first: "Creating worktree…".
    CreatingWorktree,
    /// Handing the chat to another provider first: "Preparing handoff…".
    Handoff,
}

/// A chat's status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SessionStatus {
    /// The history is loading: "Loading conversation…".
    Loading,
    /// Nothing runs and the last step succeeded: "Ready".
    Idle,
    /// A turn is starting.
    Connecting(Preparing),
    /// Work is under way: "Working…", or what the provider or app reports
    /// ("Streaming…", "Compacting context…").
    Working(Option<String>),
    /// The provider waits on a tool approval: "Waiting for approval".
    AwaitingApproval,
    /// The provider waits on an answer to its question: "Waiting for your answer".
    AwaitingAnswer,
    /// Nothing runs and the person has a note that is not a failure, such as
    /// "Worktree deleted".
    Notice(String),
    /// Something failed or was refused; the message says what.
    Error { message: String },
}

impl SessionStatus {
    /// A failure or refusal.
    pub(crate) fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }
    /// A note that is not a failure.
    pub(crate) fn notice(message: impl Into<String>) -> Self {
        Self::Notice(message.into())
    }
    /// Work under way, described by `activity`.
    pub(crate) fn working(activity: impl Into<String>) -> Self {
        Self::Working(Some(activity.into()))
    }
    /// Whether the last step succeeded and nothing runs or waits.
    pub(crate) fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
    /// This status with a failure note appended, as "Ready · Saving failed: …".
    #[must_use]
    pub(crate) fn with_failure(self, note: &str) -> Self {
        Self::error(format!("{self} · {note}"))
    }
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Loading => "Loading conversation…",
            Self::Idle => "Ready",
            Self::Connecting(Preparing::Connecting) => "Connecting…",
            Self::Connecting(Preparing::CreatingWorktree) => "Creating worktree…",
            Self::Connecting(Preparing::Handoff) => "Preparing handoff…",
            Self::Working(None) => "Working…",
            Self::Working(Some(text)) | Self::Notice(text) | Self::Error { message: text } => text,
            Self::AwaitingApproval => "Waiting for approval",
            Self::AwaitingAnswer => "Waiting for your answer",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The texts the status line showed when the status was a string.
    #[test]
    fn display_text_is_the_former_status_line() {
        let shown = [
            (SessionStatus::Loading, "Loading conversation…"),
            (SessionStatus::Idle, "Ready"),
            (
                SessionStatus::Connecting(Preparing::Connecting),
                "Connecting…",
            ),
            (
                SessionStatus::Connecting(Preparing::CreatingWorktree),
                "Creating worktree…",
            ),
            (
                SessionStatus::Connecting(Preparing::Handoff),
                "Preparing handoff…",
            ),
            (SessionStatus::Working(None), "Working…"),
            (SessionStatus::working("Streaming…"), "Streaming…"),
            (SessionStatus::AwaitingApproval, "Waiting for approval"),
            (SessionStatus::AwaitingAnswer, "Waiting for your answer"),
            (
                SessionStatus::notice("Worktree deleted"),
                "Worktree deleted",
            ),
            (
                SessionStatus::error("Saving failed: disk"),
                "Saving failed: disk",
            ),
        ];
        for (status, text) in shown {
            assert_eq!(status.to_string(), text);
        }
    }

    #[test]
    fn only_ready_is_idle_and_a_failure_note_keeps_the_former_text() {
        assert!(SessionStatus::Idle.is_idle());
        assert!(!SessionStatus::notice("Worktree deleted").is_idle());
        assert!(!SessionStatus::error("Ready").is_idle());
        let noted = SessionStatus::Idle.with_failure("Saving handoff failed: x");
        assert_eq!(noted.to_string(), "Ready · Saving handoff failed: x");
        assert!(!noted.is_idle(), "a failure after Ready is not Ready");
    }
}
