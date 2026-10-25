//! MCP resource subscriptions on both protocol versions, on top of control
//! topics: `resources/subscribe` (2025-11-25) and `subscriptions/listen`
//! (2026-07-28). Each subscribed URI maps to one topic; the app's events
//! become `notifications/resources/updated` for every URI whose topic
//! changed. When the control connection drops, the relay connects again
//! with backoff, follows the same topics, and then tells every subscriber
//! that its resources may have changed. When the app refuses the relay (the
//! person denied it, took a scope away or turned control off) it stops
//! trying; the next tool call connects again and follows the topics anew.
use crate::{link::Link, log, resources};
use monocode_control::{ClientInfo, ControlError, ErrorCode, Notice, NoticeSink, Topic, lock};
use rmcp::{Peer, RoleServer, model::ResourceUpdatedNotificationParam, service::SubscriptionSink};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

/// The first wait before connecting again, doubled up to [`LAST_RETRY`].
const FIRST_RETRY: Duration = Duration::from_millis(250);
const LAST_RETRY: Duration = Duration::from_secs(5);

/// Whether the app said no to this client, so asking again soon would only
/// bother the person.
fn refused(error: &ControlError) -> bool {
    matches!(
        error.code,
        ErrorCode::UserDeclined | ErrorCode::ScopeDenied | ErrorCode::ControlDisabled
    )
}

/// Notices from the control client, coalesced until the forwarder runs.
#[derive(Default)]
pub(crate) struct Inbox {
    pending: Mutex<Pending>,
    wake: Notify,
    /// How often the connection was lost, so a reconnect sees a newer loss.
    losses: AtomicU64,
}

#[derive(Default)]
struct Pending {
    changed: BTreeSet<Topic>,
    lost: bool,
    resumed: bool,
}

impl Inbox {
    /// The sink the control client calls, on its own threads; never blocks.
    pub(crate) fn sink(self: &Arc<Self>) -> NoticeSink {
        let inbox = Arc::clone(self);
        Arc::new(move |notice| {
            {
                let mut pending = lock(&inbox.pending);
                match notice {
                    Notice::Changed(event) => {
                        pending.changed.insert(event.topic);
                    }
                    Notice::Lost => {
                        inbox.losses.fetch_add(1, Ordering::SeqCst);
                        pending.lost = true;
                    }
                    Notice::Resumed => pending.resumed = true,
                }
            }
            inbox.wake.notify_one();
        })
    }
}

/// Where an MCP client hears about one subscribed URI.
enum Target {
    Legacy(Peer<RoleServer>),
    Listen(SubscriptionSink),
}

#[derive(Default)]
struct State {
    /// URIs subscribed with `resources/subscribe`, and whom to tell.
    legacy: BTreeSet<String>,
    peer: Option<Peer<RoleServer>>,
    /// Open `subscriptions/listen` requests: each sink and its URIs.
    listens: HashMap<u64, (SubscriptionSink, Vec<String>)>,
    next: u64,
    /// The topics the app is asked to follow.
    followed: BTreeSet<Topic>,
    forwarding: bool,
    /// The task connecting again, while it runs.
    reconnecting: Option<tokio::task::JoinHandle<()>>,
}

impl State {
    fn wanted(&self) -> BTreeSet<Topic> {
        self.legacy
            .iter()
            .chain(self.listens.values().flat_map(|(_, uris)| uris))
            .filter_map(|uri| resources::topic(uri).ok())
            .collect()
    }
}

pub(crate) struct Watch {
    link: Arc<Link>,
    inbox: Arc<Inbox>,
    state: Mutex<State>,
    /// Serializes changes to the followed topics.
    changing: tokio::sync::Mutex<()>,
}

impl Watch {
    pub(crate) fn new(link: Arc<Link>, inbox: Arc<Inbox>) -> Self {
        Self {
            link,
            inbox,
            state: Mutex::default(),
            changing: tokio::sync::Mutex::new(()),
        }
    }

    /// `resources/subscribe`: follows `uri` for `peer`. The subscriber is
    /// known before the app is asked: the app may push a change before its
    /// reply arrives, and the forwarder only tells the subscribers it knows.
    pub(crate) async fn subscribe(
        self: &Arc<Self>,
        uri: String,
        peer: Peer<RoleServer>,
        client: ClientInfo,
    ) -> Result<(), ControlError> {
        let topic = resources::topic(&uri)?;
        let _changing = self.changing.lock().await;
        let added = {
            let mut state = lock(&self.state);
            state.peer = Some(peer);
            state.legacy.insert(uri.clone())
        };
        let followed = self.follow(&[topic], client).await;
        if followed.is_err() && added {
            lock(&self.state).legacy.remove(&uri);
        }
        followed
    }

    /// `resources/unsubscribe`.
    pub(crate) async fn unsubscribe(self: &Arc<Self>, uri: &str) {
        let _changing = self.changing.lock().await;
        lock(&self.state).legacy.remove(uri);
        self.release().await;
    }

    /// An established `subscriptions/listen`: follows `uris` until
    /// [`Self::stop`] with the returned id. Like [`Self::subscribe`], the
    /// sink is known before the app is asked.
    pub(crate) async fn listen(
        self: &Arc<Self>,
        uris: Vec<String>,
        sink: SubscriptionSink,
        client: ClientInfo,
    ) -> Result<u64, ControlError> {
        let topics = uris
            .iter()
            .map(|uri| resources::topic(uri))
            .collect::<Result<Vec<_>, _>>()?;
        let _changing = self.changing.lock().await;
        let id = {
            let mut state = lock(&self.state);
            let id = state.next;
            state.next += 1;
            state.listens.insert(id, (sink, uris));
            id
        };
        if let Err(error) = self.follow(&topics, client).await {
            lock(&self.state).listens.remove(&id);
            return Err(error);
        }
        Ok(id)
    }

    /// A `subscriptions/listen` ended.
    pub(crate) async fn stop(self: &Arc<Self>, id: u64) {
        let _changing = self.changing.lock().await;
        lock(&self.state).listens.remove(&id);
        self.release().await;
    }

    async fn follow(
        self: &Arc<Self>,
        topics: &[Topic],
        client: ClientInfo,
    ) -> Result<(), ControlError> {
        self.start_forwarding();
        let missing: Vec<Topic> = {
            let state = lock(&self.state);
            topics
                .iter()
                .filter(|t| !state.followed.contains(*t))
                .cloned()
                .collect()
        };
        if !missing.is_empty() {
            self.link.subscribe(missing.clone(), client).await?;
            lock(&self.state).followed.extend(missing);
        }
        Ok(())
    }

    /// Stops following topics no subscription needs any more.
    async fn release(&self) {
        let unused: Vec<Topic> = {
            let mut state = lock(&self.state);
            let wanted = state.wanted();
            let unused = state.followed.difference(&wanted).cloned().collect();
            state.followed = wanted.intersection(&state.followed).cloned().collect();
            unused
        };
        if !unused.is_empty() {
            let _ = self.link.unsubscribe(unused).await;
        }
    }

    fn start_forwarding(self: &Arc<Self>) {
        let mut state = lock(&self.state);
        if !std::mem::replace(&mut state.forwarding, true) {
            tokio::spawn(Self::forward(Arc::downgrade(self), Arc::clone(&self.inbox)));
        }
    }

    /// Turns notices into MCP notifications for as long as the session's
    /// server lives: a session of the app's HTTP server ends long before
    /// the app does.
    async fn forward(watch: Weak<Self>, inbox: Arc<Inbox>) {
        loop {
            inbox.wake.notified().await;
            let Some(watch) = watch.upgrade() else {
                return;
            };
            let pending = std::mem::take(&mut *lock(&inbox.pending));
            if pending.lost {
                watch.start_reconnecting();
            }
            for (target, uri) in watch.targets(&pending) {
                // A client that went away is no reason to stop the others.
                let _ = match target {
                    Target::Legacy(peer) => peer
                        .notify_resource_updated(ResourceUpdatedNotificationParam::new(uri))
                        .await
                        .map_err(|e| e.to_string()),
                    Target::Listen(sink) => sink
                        .notify_resource_updated(uri)
                        .await
                        .map_err(|e| e.to_string()),
                };
            }
        }
    }

    /// Every subscribed URI the notices touch: all of them after a resume.
    fn targets(&self, pending: &Pending) -> Vec<(Target, String)> {
        let touched = |uri: &str| {
            pending.resumed || resources::topic(uri).is_ok_and(|t| pending.changed.contains(&t))
        };
        let state = lock(&self.state);
        let mut targets = Vec::new();
        if let Some(peer) = &state.peer {
            for uri in state.legacy.iter().filter(|uri| touched(uri)) {
                targets.push((Target::Legacy(peer.clone()), uri.clone()));
            }
        }
        for (sink, uris) in state.listens.values() {
            for uri in uris.iter().filter(|uri| touched(uri)) {
                targets.push((Target::Listen(sink.clone()), uri.clone()));
            }
        }
        targets
    }

    /// Connects again with backoff until the app follows the topics again,
    /// nothing is followed any more, or the app refuses this client.
    fn start_reconnecting(self: &Arc<Self>) {
        let mut state = lock(&self.state);
        if state
            .reconnecting
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return;
        }
        let watch = Arc::clone(self);
        state.reconnecting = Some(tokio::spawn(async move {
            let mut delay = FIRST_RETRY;
            loop {
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(LAST_RETRY);
                if lock(&watch.state).followed.is_empty() {
                    break;
                }
                let losses = watch.inbox.losses.load(Ordering::SeqCst);
                match watch.link.resubscribe().await {
                    // A loss while connecting means the new connection went too.
                    Ok(()) if watch.inbox.losses.load(Ordering::SeqCst) == losses => break,
                    Err(error) if refused(&error) => {
                        log::line(&format!(
                            "stopped following resources until the next tool call: {error}"
                        ));
                        break;
                    }
                    _ => {}
                }
            }
        }));
    }
}

impl Drop for Watch {
    /// Wakes the forwarder, which finds the watch gone and ends.
    fn drop(&mut self) {
        self.inbox.wake.notify_one();
    }
}

#[cfg(test)]
mod tests;
