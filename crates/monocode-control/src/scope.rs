//! Scopes a client may hold, and the access modes sessions work with.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What a client is allowed to do. Every operation needs one scope; a few
/// calls need a second one too (`Operation::ALSO`).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Lists, reads and status.
    Read,
    /// Select projects, open, focus and close tabs, panels and layout.
    Navigate,
    /// Start, send to, stop and wait for sessions.
    Dispatch,
    /// Add, archive and rename projects; organise sessions.
    Manage,
    /// The allowlisted preferences.
    Settings,
    /// Source control in local projects: stage, unstage, commit; discarding
    /// and pushing are also confirmed in the app.
    Vcs,
    /// Deletions and discarding queued messages; each call is also
    /// confirmed in the app.
    Destructive,
}

impl Scope {
    /// Every scope, in order of increasing reach.
    pub const ALL: [Self; 7] = [
        Self::Read,
        Self::Navigate,
        Self::Dispatch,
        Self::Manage,
        Self::Settings,
        Self::Vcs,
        Self::Destructive,
    ];
    /// What a newly approved client gets: `settings`, `vcs` and
    /// `destructive` stay off until the person turns them on in Settings.
    pub const DEFAULT_GRANT: [Self; 4] = [Self::Read, Self::Navigate, Self::Dispatch, Self::Manage];

    /// The wire and display identifier, as serialized.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Navigate => "navigate",
            Self::Dispatch => "dispatch",
            Self::Manage => "manage",
            Self::Settings => "settings",
            Self::Vcs => "vcs",
            Self::Destructive => "destructive",
        }
    }
    /// A short label for Settings.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "Read",
            Self::Navigate => "Navigate",
            Self::Dispatch => "Dispatch",
            Self::Manage => "Manage",
            Self::Settings => "Settings",
            Self::Vcs => "Source control",
            Self::Destructive => "Destructive",
        }
    }
}

/// A session's access mode, with the app's own identifiers, from least to
/// most access.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum AccessMode {
    /// Inspect without changing files.
    ReadOnly,
    /// Ask before changes that need approval.
    #[default]
    Supervised,
    /// Allow file edits; ask for other actions.
    AutoAcceptEdits,
    /// Review approvals automatically.
    Auto,
    /// Allow everything.
    FullAccess,
}

impl AccessMode {
    /// Every mode, from least to most access.
    pub const ALL: [Self; 5] = [
        Self::ReadOnly,
        Self::Supervised,
        Self::AutoAcceptEdits,
        Self::Auto,
        Self::FullAccess,
    ];

    /// The app's identifier, as serialized.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Supervised => "supervised",
            Self::AutoAcceptEdits => "auto-accept-edits",
            Self::Auto => "auto",
            Self::FullAccess => "full-access",
        }
    }
    /// The mode `id` names, if any.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.id() == id)
    }
    /// The app's label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read-only",
            Self::Supervised => "Supervised",
            Self::AutoAcceptEdits => "Auto-accept edits",
            Self::Auto => "Auto",
            Self::FullAccess => "Full access",
        }
    }
}
