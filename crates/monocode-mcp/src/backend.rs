//! Where the MCP server's calls go. The stdio relay reaches the running app
//! over its control socket; the app's own HTTP server hands them straight to
//! its control pipeline. Both answer on the calling thread, so the server
//! runs them on blocking threads.
use monocode_control::{ClientInfo, Command, ControlError, NoticeSink, Topic};
use serde_json::Value;
use std::sync::Arc;

/// One MCP session's way into the app. The client it speaks for is named
/// on its first call; later calls of the same session keep that identity.
pub trait Backend: Send + Sync {
    /// Runs `command` for `client`. `request_id` makes a changing call
    /// idempotent.
    ///
    /// # Errors
    /// The app's error, or why the app could not be reached.
    fn call(
        &self,
        client: &ClientInfo,
        request_id: Option<String>,
        command: Command,
    ) -> Result<Value, ControlError>;

    /// Follows `topics` as well, for `client`; their changes go to the sink
    /// the backend was made with.
    ///
    /// # Errors
    /// The app's refusal, or why the app could not be reached.
    fn subscribe(&self, client: &ClientInfo, topics: &[Topic]) -> Result<(), ControlError>;

    /// Stops following `topics`.
    ///
    /// # Errors
    /// Why the app could not be reached; the topics are forgotten anyway.
    fn unsubscribe(&self, topics: &[Topic]) -> Result<(), ControlError>;

    /// Follows the same topics again after the sink heard
    /// [`Notice::Lost`](monocode_control::Notice::Lost). Ok when nothing is
    /// followed.
    ///
    /// # Errors
    /// The app's refusal, or why the app could not be reached.
    fn resubscribe(&self) -> Result<(), ControlError>;
}

/// Makes the backend of one MCP session, telling `notices` what the topics
/// it follows do.
pub type Connect = Arc<dyn Fn(NoticeSink) -> Box<dyn Backend> + Send + Sync>;
