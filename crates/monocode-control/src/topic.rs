//! What a client can follow: each [`Topic`] stands for one read operation's
//! result, and an event about it says only that it changed, so the client
//! reads it again. No change payload travels twice.
use crate::Scope;
use serde::{Deserialize, Serialize};

/// The most topics one connection may follow at once.
pub const MAX_TOPICS: usize = 64;

/// Something whose changes a client can follow.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Topic {
    /// What the window shows, as `workspace_read` returns it.
    Workspace,
    /// The projects, as `project_list` returns them.
    Projects,
    /// One project's sessions, as `session_list` returns them.
    ProjectSessions {
        /// The project's path or key, as `session_list` takes it.
        project: String,
    },
    /// What one session is doing, as `session_status` returns it.
    Session {
        /// The session's id.
        id: i64,
    },
    /// One session's exchanges, as `session_read` returns them.
    SessionTranscript {
        /// The session's id.
        id: i64,
    },
}

impl Topic {
    /// The scope following any topic needs, checked again before every event.
    pub const SCOPE: Scope = Scope::Read;

    /// The read operation that shows what changed.
    #[must_use]
    pub fn operation(&self) -> &'static str {
        match self {
            Self::Workspace => "workspace_read",
            Self::Projects => "project_list",
            Self::ProjectSessions { .. } => "session_list",
            Self::Session { .. } => "session_status",
            Self::SessionTranscript { .. } => "session_read",
        }
    }

    /// The session this topic is about, if any.
    #[must_use]
    pub fn session(&self) -> Option<i64> {
        match self {
            Self::Session { id } | Self::SessionTranscript { id } => Some(*id),
            _ => None,
        }
    }
}
