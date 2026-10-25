//! The stdio relay's backend: one control connection, opened on the first
//! tool call. Tool calls share it concurrently: a `session_wait` never holds
//! up a read. The same connection carries the relay's subscriptions; their
//! notices go to the sink the relay set up.
use crate::{Options, backend::Backend};
use monocode_control::{
    Client, ClientInfo, Command, ControlError, ErrorCode, NoticeSink, Topic, lock,
    session_caller_name,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub(crate) struct Socket {
    options: Options,
    client: Mutex<Option<Arc<Client>>>,
    notices: NoticeSink,
}

impl Socket {
    pub(crate) fn new(options: Options, notices: NoticeSink) -> Self {
        Self {
            options,
            client: Mutex::new(None),
            notices,
        }
    }

    /// Runs `work` with the client. A client that follows nothing is
    /// dropped when the app is gone, so the next call starts afresh; one
    /// that follows topics stays to follow them again.
    fn with<T>(
        &self,
        peer: Option<&ClientInfo>,
        work: impl FnOnce(&Client) -> Result<T, ControlError>,
    ) -> Result<T, ControlError> {
        let client = self.client(peer)?;
        match work(&client) {
            Err(error) if error.code == ErrorCode::AppNotRunning => {
                let mut current = lock(&self.client);
                if current
                    .as_ref()
                    .is_some_and(|c| Arc::ptr_eq(c, &client) && c.topics().is_empty())
                {
                    *current = None;
                }
                Err((self.options.unavailable)())
            }
            result => result,
        }
    }

    fn client(&self, peer: Option<&ClientInfo>) -> Result<Arc<Client>, ControlError> {
        let endpoint = self.options.endpoint.clone()?;
        let mut current = lock(&self.client);
        Ok(Arc::clone(current.get_or_insert_with(|| {
            let mut client = Client::new(endpoint, self.identity(peer));
            if let Some(credential) = self.options.session.clone() {
                client = client.with_session(credential);
            }
            client.on_notice(Arc::clone(&self.notices));
            Arc::new(client)
        })))
    }

    /// The session's name, else `--client`, else the MCP client's own.
    fn identity(&self, peer: Option<&ClientInfo>) -> ClientInfo {
        let session = self.options.session.as_ref();
        let name = session
            .map(|credential| session_caller_name(credential.id))
            .or_else(|| self.options.client_name.clone())
            .filter(|name| !name.trim().is_empty());
        let peer = peer.cloned().unwrap_or_else(crate::identity::unnamed);
        ClientInfo {
            name: name.unwrap_or(peer.name),
            version: peer.version,
        }
    }
}

impl Backend for Socket {
    fn call(
        &self,
        client: &ClientInfo,
        request_id: Option<String>,
        command: Command,
    ) -> Result<Value, ControlError> {
        self.with(Some(client), move |c| c.send(request_id, command))
    }

    fn subscribe(&self, client: &ClientInfo, topics: &[Topic]) -> Result<(), ControlError> {
        self.with(Some(client), |c| c.subscribe(topics))
    }

    fn unsubscribe(&self, topics: &[Topic]) -> Result<(), ControlError> {
        self.with(None, |c| c.unsubscribe(topics))
    }

    fn resubscribe(&self) -> Result<(), ControlError> {
        self.with(None, Client::resubscribe)
    }
}
