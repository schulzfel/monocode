//! Pushed changes: connections follow topics, the window's own change
//! signals mark them, and a flush once per [`hub::INTERVAL`] sends an event
//! for each followed topic whose read now shows something else. Nothing
//! here runs while nobody follows anything, and chat views are watched only
//! while someone follows their session.
use super::*;
use hub::Events;
use monocode_control::{Scope, Topic};
use monocode_core::files::path_key;
use std::collections::BTreeSet;

/// A followed session's chat, watched for changes.
pub(super) struct Watched {
    view: gpui::EntityId,
    _observers: Vec<Subscription>,
}

impl Shell {
    /// Whether `caller` may still receive events: control is on and the
    /// client still holds the scope, or the session still exists, or the
    /// Mono still calls from its session. Checked before every delivery,
    /// whenever Settings changes the policy and when a session is deleted;
    /// the error is what the connection is told.
    fn may_follow(&self, caller: &CallerKind, cx: &App) -> Result<(), ControlError> {
        self.control_scope(caller, Topic::SCOPE, "Following resources", cx)
            .map(drop)
    }

    /// Runs `work` on the hub, with [`may_follow`](Self::may_follow) at hand.
    fn with_hub<T>(&mut self, work: impl FnOnce(&mut hub::Hub, &Self) -> T) -> T {
        let mut hub = std::mem::take(&mut self.control.hub);
        let result = work(&mut hub, self);
        self.control.hub = hub;
        result
    }

    /// Follows `topics` for `connection` and answers with them.
    pub(super) fn control_subscribe(
        &mut self,
        connection: u64,
        caller: CallerKind,
        topics: Vec<Topic>,
        events: Events,
        reply: &SyncSender<Result<Value, ControlError>>,
        cx: &mut Context<Self>,
    ) {
        let result = self.follow_topics(connection, caller, &topics, events, cx);
        let _ = reply.send(result.map(|()| serde_json::json!({ "topics": topics })));
    }

    fn follow_topics(
        &mut self,
        connection: u64,
        caller: CallerKind,
        topics: &[Topic],
        events: Events,
        cx: &mut Context<Self>,
    ) -> Result<(), ControlError> {
        // A session's credential may stand for a Mono; who it is now decides.
        let caller = self.resolve_caller(&caller, cx)?;
        self.control_scope(&caller, Scope::Read, "Following resources", cx)?;
        for topic in topics {
            self.control_topic_target(topic, cx)?;
            self.mono_topic(&caller, topic, cx)?;
        }
        let new = self
            .control
            .hub
            .subscribe(connection, caller, events, topics)?;
        // What a new topic shows now is its baseline: only later changes count.
        for topic in &new {
            let shows = self.control_fingerprint(topic, cx);
            self.control.hub.changed(topic, shows);
        }
        self.watch_control_sessions(cx);
        Ok(())
    }

    pub(super) fn control_unsubscribe(
        &mut self,
        connection: u64,
        topics: &[Topic],
        reply: &SyncSender<Result<Value, ControlError>>,
        cx: &mut Context<Self>,
    ) {
        self.control.hub.unsubscribe(connection, topics);
        self.watch_control_sessions(cx);
        let _ = reply.send(Ok(serde_json::json!({ "topics": [] })));
    }

    /// After Settings changed the policy or a session was deleted: tells
    /// each follower that may no longer follow why, closing its connection,
    /// and forgets it.
    pub(in crate::shell) fn control_refuse_followers(&mut self, cx: &mut Context<Self>) {
        let app: &App = cx;
        self.with_hub(|hub, shell| hub.refuse(|caller| shell.may_follow(caller, app)));
        self.watch_control_sessions(cx);
    }

    /// Whether `caller` may follow `topic`: a Mono only follows its assigned
    /// projects and their sessions (§5 step 9); anyone else any topic.
    fn mono_topic(&self, caller: &CallerKind, topic: &Topic, cx: &App) -> Result<(), ControlError> {
        let mono = match caller {
            CallerKind::Mono { mono, .. } => self.caller_mono(mono, cx),
            CallerKind::Session(_) | CallerKind::External(_) => return Ok(()),
        };
        let project = match topic {
            Topic::ProjectSessions { project } => Some(project.clone()),
            other => other
                .session()
                .filter(|id| self.session_summaries.contains_key(id))
                .map(|id| self.session_metadata(id, cx).summary.cwd.clone()),
        };
        match (mono, project) {
            (Some(mono), Some(project)) if !mono.works_on(&project) => Err(ControlError::new(
                ErrorCode::ScopeDenied,
                format!("{project} is not one of this Mono's assigned projects"),
            )),
            (Some(_), _) => Ok(()),
            (None, _) => Err(ControlError::new(ErrorCode::NotFound, caller::MONO_GONE)),
        }
    }

    /// Forgets what a closed connection followed.
    pub(super) fn control_closed(&mut self, connection: u64, cx: &mut Context<Self>) {
        self.control.hub.close(connection);
        self.watch_control_sessions(cx);
    }

    /// A session's chat changed (`SessionChanged`): its status, transcript
    /// and its project's session list may show something new.
    pub(in crate::shell) fn control_session_changed(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.control.hub.is_empty() {
            return;
        }
        // Its view may have loaded since the topic was followed.
        self.watch_control_sessions(cx);
        let project = path_key(&self.session_metadata(id, cx).summary.cwd);
        self.mark_control(
            |t| match t {
                Topic::ProjectSessions { project: listed } => path_key(listed) == project,
                other => other.session() == Some(id),
            },
            cx,
        );
    }

    /// A chat view loaded: watch it if its session is followed.
    pub(in crate::shell) fn control_session_loaded(&mut self, cx: &mut Context<Self>) {
        if self.control.hub.is_empty() {
            return;
        }
        self.watch_control_sessions(cx);
        self.mark_control(|t| matches!(t, Topic::ProjectSessions { .. }), cx);
    }

    /// The workspace was saved: tabs, projects or sessions may have changed.
    pub(in crate::shell) fn control_workspace_changed(&mut self, cx: &mut Context<Self>) {
        if self.control.hub.is_empty() {
            return;
        }
        self.mark_control(
            |t| {
                matches!(
                    t,
                    Topic::Workspace | Topic::Projects | Topic::ProjectSessions { .. }
                )
            },
            cx,
        );
    }

    fn mark_control(&mut self, which: impl Fn(&Topic) -> bool, cx: &mut Context<Self>) {
        if self.control.hub.mark(which) {
            self.schedule_control_flush(cx);
        }
    }

    fn schedule_control_flush(&mut self, cx: &mut Context<Self>) {
        self.control.hub.scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(hub::INTERVAL).await;
            let _ = this.update(cx, |this, cx| this.flush_control(cx));
        })
        .detach();
    }

    /// Sends an event for each marked topic that now reads differently.
    fn flush_control(&mut self, cx: &mut Context<Self>) {
        self.control.hub.scheduled = false;
        let mut changed = Vec::new();
        for topic in self.control.hub.take_dirty() {
            let shows = self.control_fingerprint(&topic, cx);
            if self.control.hub.changed(&topic, shows) {
                changed.push(topic);
            }
        }
        let followers = self.control.hub.followers();
        let app: &App = cx;
        let owed = self
            .with_hub(|hub, shell| hub.deliver(&changed, |caller| shell.may_follow(caller, app)));
        let left = self.control.hub.followers() != followers;
        if owed {
            self.schedule_control_flush(cx);
        }
        // Only a follower that left changes which chats are watched.
        if left {
            self.watch_control_sessions(cx);
        }
    }

    /// Watches the loaded chats of followed sessions, and only those.
    fn watch_control_sessions(&mut self, cx: &mut Context<Self>) {
        let wanted: BTreeSet<i64> = self
            .control
            .hub
            .topics()
            .filter_map(Topic::session)
            .collect();
        self.control.watched.retain(|id, _| wanted.contains(id));
        for id in wanted {
            let Some(view) = self.sessions.get(&id).cloned() else {
                continue;
            };
            let watched = self.control.watched.get(&id);
            if watched.is_some_and(|w| w.view == view.entity_id()) {
                continue;
            }
            let transcript = view.read(cx).transcript.clone();
            let observers = vec![
                cx.observe(&view, move |this, _, cx| {
                    this.mark_control(|t| *t == Topic::Session { id }, cx);
                }),
                cx.observe(&transcript, move |this, _, cx| {
                    this.mark_control(|t| *t == Topic::SessionTranscript { id }, cx);
                }),
            ];
            self.control.watched.insert(
                id,
                Watched {
                    view: view.entity_id(),
                    _observers: observers,
                },
            );
        }
    }
}
