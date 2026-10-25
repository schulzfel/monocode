//! Who follows which topics, and which events each connection is owed. Pure
//! state on the UI thread: the shell marks topics as changed, and a flush
//! once per [`INTERVAL`] turns the changed ones into events. Delivery never
//! waits: each connection has a bounded queue, and an event that does not
//! fit stays owed, with its newest revision, until the next flush. A
//! follower that may no longer follow is told why and its connection is
//! closed, so its client notices and can follow again later.
use super::caller::CallerKind;
use monocode_control::{ControlError, MAX_TOPICS, Topic, frame::Event};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::mpsc::{SyncSender, TrySendError},
    time::Duration,
};

/// How long changes gather before they become events.
pub(super) const INTERVAL: Duration = Duration::from_millis(100);
/// How many events may wait for one connection's writer.
pub(super) const QUEUE: usize = MAX_TOPICS;

/// What a connection's event writer is handed.
#[derive(Debug)]
pub(super) enum Push {
    /// A followed topic changed.
    Event(Event),
    /// The connection may no longer follow: say why, then close it.
    Lost(ControlError),
    /// Sent only by the connection itself, which is ending: stop writing.
    End,
}

/// A connection's event queue.
pub(super) type Events = SyncSender<Push>;

struct Follower {
    caller: CallerKind,
    events: Events,
    topics: BTreeSet<Topic>,
    /// Events that did not fit the queue yet, newest revision per topic.
    owed: BTreeMap<Topic, u64>,
}

#[derive(Default)]
pub(super) struct Hub {
    followers: HashMap<u64, Follower>,
    /// Every followed topic, with how many connections follow it.
    followed: BTreeMap<Topic, usize>,
    /// Counts flushes that changed something; every event carries it.
    revision: u64,
    dirty: BTreeSet<Topic>,
    /// What each followed topic showed when it was last checked.
    shown: HashMap<Topic, u64>,
    /// Whether a flush is already on its way.
    pub(super) scheduled: bool,
}

impl Hub {
    /// Whether nobody follows anything: then marks cost nothing.
    pub(super) fn is_empty(&self) -> bool {
        self.followers.is_empty()
    }
    /// How many connections follow something.
    pub(super) fn followers(&self) -> usize {
        self.followers.len()
    }
    /// Every followed topic, once.
    pub(super) fn topics(&self) -> impl Iterator<Item = &Topic> {
        self.followed.keys()
    }
    /// Counts `topic` as followed by one more connection (`true`) or one
    /// fewer; a topic nobody follows any more is dropped.
    fn count(followed: &mut BTreeMap<Topic, usize>, topic: &Topic, more: bool) {
        if more {
            *followed.entry(topic.clone()).or_default() += 1;
        } else if let Some(count) = followed.get_mut(topic) {
            *count -= 1;
            if *count == 0 {
                followed.remove(topic);
            }
        }
    }
    /// Removes `connection`'s follower and everything it followed.
    fn remove(&mut self, connection: u64) -> Option<Follower> {
        let follower = self.followers.remove(&connection)?;
        for topic in &follower.topics {
            Self::count(&mut self.followed, topic, false);
        }
        Some(follower)
    }
    /// Adds `topics` for `connection`; returns those nobody followed before.
    pub(super) fn subscribe(
        &mut self,
        connection: u64,
        caller: CallerKind,
        events: Events,
        topics: &[Topic],
    ) -> Result<Vec<Topic>, ControlError> {
        let held = self.followers.get(&connection).map(|f| &f.topics);
        let fresh: BTreeSet<&Topic> = topics
            .iter()
            .filter(|t| !held.is_some_and(|held| held.contains(*t)))
            .collect();
        if held.map_or(0, BTreeSet::len) + fresh.len() > MAX_TOPICS {
            return Err(ControlError::invalid_input(format!(
                "A connection follows at most {MAX_TOPICS} topics at once"
            )));
        }
        let new: Vec<Topic> = fresh
            .iter()
            .filter(|t| !self.followed.contains_key(**t))
            .map(|t| (*t).clone())
            .collect();
        for topic in &fresh {
            Self::count(&mut self.followed, topic, true);
        }
        let follower = self
            .followers
            .entry(connection)
            .or_insert_with(|| Follower {
                caller,
                events,
                topics: BTreeSet::new(),
                owed: BTreeMap::new(),
            });
        follower.topics.extend(fresh.into_iter().cloned());
        Ok(new)
    }
    pub(super) fn unsubscribe(&mut self, connection: u64, topics: &[Topic]) {
        if let Some(follower) = self.followers.get_mut(&connection) {
            for topic in topics {
                if follower.topics.remove(topic) {
                    Self::count(&mut self.followed, topic, false);
                }
                follower.owed.remove(topic);
            }
        }
        self.forget_unfollowed();
    }
    /// Forgets everything `connection` followed.
    pub(super) fn close(&mut self, connection: u64) {
        self.remove(connection);
        self.forget_unfollowed();
    }
    /// Marks the followed topics `which` accepts as changed. Returns whether
    /// a flush is now needed and not yet scheduled.
    pub(super) fn mark(&mut self, which: impl Fn(&Topic) -> bool) -> bool {
        let mut any = false;
        for topic in self.followed.keys().filter(|t| which(t)) {
            any = true;
            if !self.dirty.contains(topic) {
                self.dirty.insert(topic.clone());
            }
        }
        any && !self.scheduled
    }
    pub(super) fn take_dirty(&mut self) -> BTreeSet<Topic> {
        std::mem::take(&mut self.dirty)
    }
    /// Records what `topic` shows now. `None` means it cannot be compared,
    /// so it counts as changed. Returns whether it changed.
    pub(super) fn changed(&mut self, topic: &Topic, shows: Option<u64>) -> bool {
        let Some(shows) = shows else {
            return true;
        };
        self.shown.insert(topic.clone(), shows) != Some(shows)
    }
    /// Tells each follower `allowed` refuses (its client was revoked or lost
    /// the read scope, or control is off) why, which closes its connection,
    /// and forgets it.
    pub(super) fn refuse(&mut self, allowed: impl Fn(&CallerKind) -> Result<(), ControlError>) {
        let refused: Vec<(u64, ControlError)> = self
            .followers
            .iter()
            .filter_map(|(id, follower)| allowed(&follower.caller).err().map(|e| (*id, e)))
            .collect();
        for (id, error) in refused {
            if let Some(follower) = self.remove(id) {
                // A full queue still holds the writer's attention; a writer
                // that is gone has nothing to close.
                let _ = follower.events.try_send(Push::Lost(error));
            }
        }
        self.forget_unfollowed();
    }
    /// Delivers `changed` to whoever follows it and whatever is still owed,
    /// after refusing followers `allowed` refuses. A connection that closed
    /// is forgotten. Returns whether some events are still owed, so another
    /// flush is needed.
    pub(super) fn deliver(
        &mut self,
        changed: &[Topic],
        allowed: impl Fn(&CallerKind) -> Result<(), ControlError>,
    ) -> bool {
        self.refuse(allowed);
        if !changed.is_empty() {
            self.revision += 1;
        }
        let revision = self.revision;
        let mut gone = Vec::new();
        for (id, follower) in &mut self.followers {
            for topic in changed.iter().filter(|t| follower.topics.contains(*t)) {
                follower.owed.insert(topic.clone(), revision);
            }
            while let Some((topic, revision)) = follower.owed.pop_first() {
                let event = Event {
                    topic: topic.clone(),
                    revision,
                };
                match follower.events.try_send(Push::Event(event)) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        follower.owed.insert(topic, revision);
                        break;
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        gone.push(*id);
                        break;
                    }
                }
            }
        }
        for id in &gone {
            self.remove(*id);
        }
        if !gone.is_empty() {
            self.forget_unfollowed();
        }
        self.followers.values().any(|f| !f.owed.is_empty())
    }
    fn forget_unfollowed(&mut self) {
        let followed = &self.followed;
        self.shown.retain(|topic, _| followed.contains_key(topic));
        self.dirty.retain(|topic| followed.contains_key(topic));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Receiver};

    fn queue(capacity: usize) -> (Events, Receiver<Push>) {
        mpsc::sync_channel(capacity)
    }

    fn drain(events: &Receiver<Push>) -> Vec<(Topic, u64)> {
        events
            .try_iter()
            .filter_map(|push| match push {
                Push::Event(e) => Some((e.topic, e.revision)),
                Push::Lost(_) | Push::End => None,
            })
            .collect()
    }

    fn allow_all(_: &CallerKind) -> Result<(), ControlError> {
        Ok(())
    }

    #[test]
    fn many_marks_in_one_interval_are_one_event_per_topic() {
        let mut hub = Hub::default();
        let (events, heard) = queue(QUEUE);
        let new = hub
            .subscribe(1, "c".into(), events, &[Topic::Workspace, Topic::Projects])
            .unwrap();
        assert_eq!(new.len(), 2);
        assert!(
            hub.mark(|t| *t == Topic::Workspace),
            "the first mark asks for a flush"
        );
        hub.scheduled = true;
        for _ in 0..5 {
            assert!(
                !hub.mark(|t| *t == Topic::Workspace),
                "later ones wait for it"
            );
        }
        assert!(
            !hub.mark(|t| matches!(t, Topic::Session { .. })),
            "nobody follows it"
        );
        let dirty: Vec<Topic> = hub.take_dirty().into_iter().collect();
        assert_eq!(dirty, [Topic::Workspace]);
        assert!(!hub.deliver(&dirty, allow_all));
        assert_eq!(drain(&heard), [(Topic::Workspace, 1)]);
        assert!(hub.changed(&Topic::Projects, Some(9)));
        assert!(
            !hub.changed(&Topic::Projects, Some(9)),
            "the same view is no change"
        );
        assert!(
            hub.changed(&Topic::Projects, None),
            "a view that cannot be compared is"
        );
    }

    #[test]
    fn a_full_queue_never_blocks_and_the_owed_event_carries_the_newest_revision() {
        let mut hub = Hub::default();
        let (events, heard) = queue(1);
        let topics = [Topic::Workspace, Topic::Projects, Topic::Session { id: 3 }];
        hub.subscribe(1, "c".into(), events, &topics).unwrap();
        // Nobody drains the queue: delivery returns at once, owing two.
        assert!(hub.deliver(&topics, allow_all), "events are still owed");
        assert_eq!(drain(&heard), [(Topic::Workspace, 1)]);
        assert!(hub.deliver(&[Topic::Session { id: 3 }], allow_all));
        assert_eq!(drain(&heard), [(Topic::Projects, 1)]);
        assert!(!hub.deliver(&[], allow_all));
        assert_eq!(
            drain(&heard),
            [(Topic::Session { id: 3 }, 2)],
            "the stale event was replaced by the newer change"
        );
    }

    #[test]
    fn a_closed_or_refused_follower_is_forgotten() {
        let mut hub = Hub::default();
        let (events, heard) = queue(QUEUE);
        hub.subscribe(1, "gone".into(), events, &[Topic::Workspace])
            .unwrap();
        drop(heard);
        let (events, _kept) = queue(QUEUE);
        hub.subscribe(2, "revoked".into(), events, &[Topic::Projects])
            .unwrap();
        hub.changed(&Topic::Projects, Some(1));
        let (events, told) = queue(QUEUE);
        hub.subscribe(4, "revoked".into(), events, &[Topic::Workspace])
            .unwrap();
        hub.deliver(&[Topic::Workspace, Topic::Projects], |c| {
            if *c == CallerKind::External("revoked".into()) {
                Err(ControlError::invalid_input("revoked"))
            } else {
                Ok(())
            }
        });
        assert!(hub.is_empty(), "both are forgotten");
        assert!(
            matches!(told.try_recv(), Ok(Push::Lost(_))),
            "a refused follower is told why, so its connection closes"
        );
        assert!(
            hub.changed(&Topic::Projects, Some(1)),
            "and so is what they saw"
        );
        let (events, _heard) = queue(QUEUE);
        let many: Vec<Topic> = (0..=i64::try_from(MAX_TOPICS).unwrap())
            .map(|id| Topic::Session { id })
            .collect();
        assert!(hub.subscribe(3, "c".into(), events.clone(), &many).is_err());
        assert!(hub.subscribe(3, "c".into(), events, &many[1..]).is_ok());
        hub.unsubscribe(3, &many);
        assert!(hub.topics().next().is_none());
        hub.close(3);
        assert!(hub.is_empty());
    }
}
