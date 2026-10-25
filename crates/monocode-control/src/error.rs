//! The closed set of control errors every operation and transport returns.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Why a control call failed. This set is closed: callers may match on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// No app is listening for this data folder.
    AppNotRunning,
    /// External control is turned off in Settings.
    ControlDisabled,
    /// This client was not granted the operation's scope.
    ScopeDenied,
    /// The person refused, or did not answer in time.
    UserDeclined,
    /// The target does not exist.
    NotFound,
    /// The project is not open in the app.
    ProjectNotOpen,
    /// The session is working.
    SessionBusy,
    /// The session is not idle.
    SessionNotIdle,
    /// The provider is not installed or not signed in.
    ProviderUnavailable,
    /// The model is not offered by the provider.
    ModelUnavailable,
    /// Remote projects are not supported.
    RemoteUnsupported,
    /// The input is malformed or out of range.
    InvalidInput,
    /// The request ID was used before with different input, or is still running.
    RequestConflict,
    /// The app did not answer in time.
    Timeout,
    /// Anything else.
    Internal,
}

impl ErrorCode {
    /// Every code, in declaration order.
    pub const ALL: [Self; 15] = [
        Self::AppNotRunning,
        Self::ControlDisabled,
        Self::ScopeDenied,
        Self::UserDeclined,
        Self::NotFound,
        Self::ProjectNotOpen,
        Self::SessionBusy,
        Self::SessionNotIdle,
        Self::ProviderUnavailable,
        Self::ModelUnavailable,
        Self::RemoteUnsupported,
        Self::InvalidInput,
        Self::RequestConflict,
        Self::Timeout,
        Self::Internal,
    ];

    /// The wire identifier, as serialized.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::AppNotRunning => "app_not_running",
            Self::ControlDisabled => "control_disabled",
            Self::ScopeDenied => "scope_denied",
            Self::UserDeclined => "user_declined",
            Self::NotFound => "not_found",
            Self::ProjectNotOpen => "project_not_open",
            Self::SessionBusy => "session_busy",
            Self::SessionNotIdle => "session_not_idle",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ModelUnavailable => "model_unavailable",
            Self::RemoteUnsupported => "remote_unsupported",
            Self::InvalidInput => "invalid_input",
            Self::RequestConflict => "request_conflict",
            Self::Timeout => "timeout",
            Self::Internal => "internal",
        }
    }
    /// Whether the same call can succeed later without changing it.
    #[must_use]
    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::AppNotRunning | Self::SessionBusy | Self::SessionNotIdle | Self::Timeout
        )
    }
}

/// A failed control call: a code to match on, a message for people, and
/// whether retrying the same call may succeed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlError {
    /// What went wrong.
    pub code: ErrorCode,
    /// A sentence for the person reading the tool result.
    pub message: String,
    /// Whether the same call can succeed later.
    pub retryable: bool,
}

impl ControlError {
    /// An error with the code's default retry advice.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: code.retryable(),
        }
    }
    /// The same error with explicit retry advice.
    #[must_use]
    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }
    /// Malformed or out-of-range input.
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }
    /// An unexpected failure.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
    /// No app answers on the control endpoint.
    #[must_use]
    pub fn app_not_running() -> Self {
        Self::new(
            ErrorCode::AppNotRunning,
            "MonoCode is not running for this data folder. Open the app and try again.",
        )
    }
    /// External control is off.
    #[must_use]
    pub fn control_disabled() -> Self {
        Self::new(
            ErrorCode::ControlDisabled,
            "MonoCode's MCP server is turned off. Turn it on in MonoCode under Settings → General → MCP server.",
        )
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.id(), self.message)
    }
}

impl std::error::Error for ControlError {}
