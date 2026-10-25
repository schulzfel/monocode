//! The server's way into the app: its session's [`Backend`], whose blocking
//! calls run on blocking threads so a long `session_wait` holds up nothing.
use crate::{backend::Backend, tools::Request};
use monocode_control::{ClientInfo, ControlError, Topic};
use serde_json::Value;
use std::sync::Arc;

pub(crate) struct Link {
    backend: Arc<dyn Backend>,
}

impl Link {
    pub(crate) fn new(backend: Box<dyn Backend>) -> Self {
        Self {
            backend: Arc::from(backend),
        }
    }

    /// Forwards `request` for `client`.
    pub(crate) async fn call(
        &self,
        request: Request,
        client: ClientInfo,
    ) -> Result<Value, ControlError> {
        self.blocking(move |backend| backend.call(&client, request.request_id, request.command))
            .await
    }

    /// Follows `topics` on the app.
    pub(crate) async fn subscribe(
        &self,
        topics: Vec<Topic>,
        client: ClientInfo,
    ) -> Result<(), ControlError> {
        self.blocking(move |backend| backend.subscribe(&client, &topics))
            .await
    }

    /// Stops following `topics`.
    pub(crate) async fn unsubscribe(&self, topics: Vec<Topic>) -> Result<(), ControlError> {
        self.blocking(move |backend| backend.unsubscribe(&topics))
            .await
    }

    /// Follows the same topics again, if any.
    pub(crate) async fn resubscribe(&self) -> Result<(), ControlError> {
        self.blocking(|backend| backend.resubscribe()).await
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(&dyn Backend) -> Result<T, ControlError> + Send + 'static,
    ) -> Result<T, ControlError> {
        let backend = Arc::clone(&self.backend);
        tokio::task::spawn_blocking(move || work(backend.as_ref()))
            .await
            .map_err(|e| ControlError::internal(format!("The MCP server's worker stopped: {e}")))?
    }
}
