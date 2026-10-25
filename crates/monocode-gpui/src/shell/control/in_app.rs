//! The way the app's own MCP server reaches the control pipeline: each HTTP
//! session (2025-11-25), or each stateless request (2026-07-28), is one
//! in-process connection. Its first call says hello with the MCP client's
//! name, which the person approves as they would a socket client's; then
//! its calls and subscriptions go on the channel the socket's connection
//! threads use, so grants, denials, confirmations, receipts and the activity
//! log apply unchanged. When the shell takes its topics away (the client was
//! revoked or lost a scope), the next call or resubscribe says hello again.
use super::{
    caller::CallerKind,
    feed::Feed,
    hub::{self, Push},
    listener::{self, Request},
};
use monocode_control::{
    ClientInfo, Command, ControlError, MAX_TOPICS, Notice, NoticeSink, Topic, frame::Subscription,
    lock,
};
use monocode_mcp::Backend;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

pub(super) struct InApp {
    requests: async_channel::Sender<Request>,
    notices: NoticeSink,
    /// Set when the shell took this connection's topics away.
    lost: Arc<AtomicBool>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Whom the hello named, to say it again.
    client: Option<ClientInfo>,
    /// Who calls, once the hello was granted.
    caller: Option<CallerKind>,
    /// This connection's event queue, once it follows something.
    feed: Option<Feed>,
    /// The topics followed, kept to follow again.
    topics: BTreeSet<Topic>,
    /// Whether the shell holds `topics` for this connection now.
    subscribed: bool,
}

impl InApp {
    pub(super) fn new(requests: async_channel::Sender<Request>, notices: NoticeSink) -> Self {
        Self {
            requests,
            notices,
            lost: Arc::default(),
            state: Mutex::default(),
        }
    }

    /// The caller, saying hello first unless the hello was granted and
    /// nothing was taken away since.
    fn welcome(
        &self,
        state: &mut State,
        client: Option<&ClientInfo>,
    ) -> Result<CallerKind, ControlError> {
        if self.lost.swap(false, Ordering::SeqCst) {
            state.caller = None;
            state.subscribed = false;
        }
        if let Some(caller) = &state.caller {
            return Ok(caller.clone());
        }
        let client = client
            .or(state.client.as_ref())
            .cloned()
            .ok_or_else(|| ControlError::internal("This MCP session never named its client"))?;
        // Never a session credential: the app's own sessions use the socket.
        let caller = CallerKind::of_hello(&client, None);
        listener::decide(&self.requests, client.clone(), None)?;
        state.client = Some(client);
        state.caller = Some(caller.clone());
        Ok(caller)
    }

    /// Follows every kept topic again, then says anything may have changed.
    fn resume(&self, state: &mut State, caller: &CallerKind) -> Result<(), ControlError> {
        let topics = state.topics.clone();
        self.follow(state, caller, topics)?;
        state.subscribed = true;
        (self.notices)(Notice::Resumed);
        Ok(())
    }

    /// Asks the shell to follow `topics` for this connection, setting up
    /// its event queue the first time.
    fn follow(
        &self,
        state: &mut State,
        caller: &CallerKind,
        topics: BTreeSet<Topic>,
    ) -> Result<(), ControlError> {
        let feed = state
            .feed
            .get_or_insert_with(|| self.feed(Feed::new(self.requests.clone())));
        let subscription = Subscription {
            id: 0,
            topics: topics.into_iter().collect(),
        };
        feed.follow(caller, subscription, true).map(drop)
    }

    /// Gives `feed` its event queue and a thread that turns the events into
    /// notices; without the thread the feed refuses to follow, retryably.
    fn feed(&self, mut feed: Feed) -> Feed {
        let (events, queued) = mpsc::sync_channel(hub::QUEUE);
        let (notices, lost) = (Arc::clone(&self.notices), Arc::clone(&self.lost));
        let started = std::thread::Builder::new()
            .name("mcp-http-events".into())
            .spawn(move || {
                while let Ok(push) = queued.recv() {
                    match push {
                        Push::Event(event) => notices(Notice::Changed(event)),
                        Push::Lost(_) => {
                            lost.store(true, Ordering::SeqCst);
                            notices(Notice::Lost);
                        }
                        Push::End => return,
                    }
                }
            });
        if started.is_ok() {
            feed.events = Some(events);
        }
        feed
    }
}

impl Backend for InApp {
    fn call(
        &self,
        client: &ClientInfo,
        request_id: Option<String>,
        command: Command,
    ) -> Result<Value, ControlError> {
        let caller = {
            let mut state = lock(&self.state);
            let caller = self.welcome(&mut state, Some(client))?;
            // A call after the topics were taken away follows them again,
            // as a socket client does when it connects anew.
            if !(state.subscribed || state.topics.is_empty())
                && self.resume(&mut state, &caller).is_err()
            {
                (self.notices)(Notice::Lost);
            }
            caller
        };
        listener::ask(&self.requests, &caller, request_id, command)
    }

    fn subscribe(&self, client: &ClientInfo, topics: &[Topic]) -> Result<(), ControlError> {
        let mut state = lock(&self.state);
        let caller = self.welcome(&mut state, Some(client))?;
        let fresh: BTreeSet<Topic> = topics
            .iter()
            .filter(|t| !state.topics.contains(t))
            .cloned()
            .collect();
        if fresh.is_empty() && state.subscribed {
            return Ok(());
        }
        if state.topics.len() + fresh.len() > MAX_TOPICS {
            return Err(ControlError::invalid_input(format!(
                "A client follows at most {MAX_TOPICS} resources at once"
            )));
        }
        // A connection that lost its earlier topics gets them back too.
        let send = if state.subscribed {
            fresh.clone()
        } else {
            state.topics.union(&fresh).cloned().collect()
        };
        self.follow(&mut state, &caller, send)?;
        state.topics.extend(fresh);
        state.subscribed = true;
        Ok(())
    }

    fn unsubscribe(&self, topics: &[Topic]) -> Result<(), ControlError> {
        let mut state = lock(&self.state);
        let removed: Vec<Topic> = topics
            .iter()
            .filter(|t| state.topics.remove(t))
            .cloned()
            .collect();
        let (Some(feed), Some(caller)) = (&state.feed, &state.caller) else {
            return Ok(());
        };
        if removed.is_empty() || !state.subscribed {
            return Ok(());
        }
        let subscription = Subscription {
            id: 0,
            topics: removed,
        };
        feed.follow(caller, subscription, false).map(drop)
    }

    fn resubscribe(&self) -> Result<(), ControlError> {
        let mut state = lock(&self.state);
        if state.topics.is_empty() {
            return Ok(());
        }
        let caller = self.welcome(&mut state, None)?;
        if state.subscribed {
            return Ok(());
        }
        self.resume(&mut state, &caller)
    }
}
