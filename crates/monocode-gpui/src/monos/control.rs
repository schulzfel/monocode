//! What the control pipeline asks of the Monos: which Mono a calling session
//! belongs to and the chat it calls from, and the bookkeeping a Mono's
//! launches, follow-ups and stops get, as the old bridge kept it: launch
//! records and placement here, folders and completion reports in `host`.
use super::*;
use crate::pane_layout::Edge;
use monocode_core::mono::Launch;

/// A session a Mono started, once it exists, for [`Monos::control_launched`].
pub(crate) struct Launched {
    /// The Mono that started it.
    pub(crate) mono: String,
    /// The session the Mono called from: its main chat or a habit run.
    pub(crate) caller: i64,
    pub(crate) summary: Session,
    pub(crate) settings: Settings,
    /// Whether the prompt was saved as a draft instead of sent.
    pub(crate) draft: bool,
    /// Whether the Mono hears back once it is done.
    pub(crate) notify: bool,
    /// Whether it opens, when no placement says where.
    pub(crate) reveal: bool,
    /// The pane it goes beside, if any.
    pub(crate) placement: Option<(i64, Edge)>,
}

/// A session a Mono sent a message to and wants to hear back from, for
/// [`Monos::control_rearm`].
pub(crate) struct Messaged {
    pub(crate) mono: String,
    /// The session the Mono called from.
    pub(crate) caller: i64,
    pub(crate) session: i64,
    pub(crate) cwd: String,
    pub(crate) title: String,
    pub(crate) harness: String,
    pub(crate) model: Option<String>,
}

impl Monos {
    /// The Mono whose main chat (`false`) or running habit (`true`) session
    /// `id` is, as the bridge looked it up (MO-7).
    pub(crate) fn caller_of(&self, id: i64) -> Option<(String, bool)> {
        if let Some(mono) = self.roster.of_session(id) {
            return Some((mono.id.clone(), false));
        }
        self.running
            .get(&id)
            .filter(|run| self.roster.get(&run.mono).is_some())
            .map(|run| (run.mono.clone(), true))
    }

    /// The live chat a Mono calls from: its main chat, or its habit run's.
    /// `None` while that chat is not loaded.
    pub(crate) fn caller_view(
        &self,
        session: i64,
        mono: &str,
        run: bool,
        cx: &App,
    ) -> Option<Entity<Conversation>> {
        if run {
            return self.running.get(&session).map(|run| run.view.clone());
        }
        self.chats
            .get(mono)
            .filter(|chat| chat.read(cx).summary.id == session)
            .cloned()
    }

    /// What a Mono's launches and follow-ups start from: its main chat,
    /// else the habit run calling (MO-16).
    pub(crate) fn source_view(&self, session: i64, mono: &str) -> Option<Entity<Conversation>> {
        self.chats
            .get(mono)
            .or_else(|| self.running.get(&session).map(|run| &run.view))
            .cloned()
    }

    /// What the activity log calls a Mono: its name, or for a habit run the
    /// run's title, "{Mono} · {habit}".
    pub(crate) fn caller_label(
        &self,
        session: i64,
        mono: &str,
        run: bool,
        cx: &App,
    ) -> Option<String> {
        if run {
            return self
                .running
                .get(&session)
                .map(|run| run.view.read(cx).summary.title.clone());
        }
        self.roster.get(mono).map(Mono::label)
    }

    /// Records, installs and shows a session a Mono started, in the bridge's
    /// order (MO-21): its launch record with the calling chat's turn as it
    /// is now, the hidden install, the roster save, the Mono's folder, then
    /// the placement, which always reveals, or the reveal.
    pub(crate) fn control_launched(
        &mut self,
        launched: Launched,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Launched {
            mono,
            caller,
            summary,
            settings,
            draft,
            notify,
            reveal,
            placement,
        } = launched;
        let id = summary.id;
        let origin = self
            .chats
            .values()
            .chain(self.running.values().map(|r| &r.view))
            .find(|v| v.read(cx).summary.id == caller)
            .map_or(0, |v| v.read(cx).mono_turn);
        if let Some(record) = self.roster.get_mut(&mono) {
            record.launches.push(Launch {
                session_id: id,
                cwd: summary.cwd.clone(),
                title: summary.title.clone(),
                harness: summary.provider.clone(),
                model: settings.model.clone(),
                origin_turn: ((caller as u64) << 32) | origin,
                notify_on_complete: notify,
                completed: false,
                dismissed: false,
            });
        }
        let _ = self.owner.update(cx, |owner, cx| {
            owner.mono_install_app(summary, settings, draft, cx)
        });
        self.save(cx);
        self.group_launch(&mono, id, cx);
        if placement.is_some() {
            let _ = self.owner.update(cx, |owner, cx| {
                owner.app_place_launch(id, placement, true, window, cx)
            });
        } else if reveal {
            self.open_workspace(id, window, cx);
        }
    }

    /// Asks for a completion report on a session a Mono just messaged, as
    /// the bridge did (MO-25): its launch record is re-armed, or recorded
    /// when it has none, under the turn of the Mono's main chat.
    pub(crate) fn control_rearm(&mut self, messaged: Messaged, cx: &mut Context<Self>) {
        let origin = self
            .source_view(messaged.caller, &messaged.mono)
            .map_or(0, |v| v.read(cx).mono_turn);
        let origin_turn = ((messaged.caller as u64) << 32) | origin;
        if let Some(mono) = self.roster.get_mut(&messaged.mono) {
            if let Some(launch) = mono
                .launches
                .iter_mut()
                .find(|l| l.session_id == messaged.session)
            {
                launch.completed = false;
                launch.dismissed = false;
                launch.notify_on_complete = true;
                launch.origin_turn = origin_turn;
            } else {
                mono.launches.push(Launch {
                    session_id: messaged.session,
                    cwd: messaged.cwd,
                    title: messaged.title,
                    harness: messaged.harness,
                    model: messaged.model,
                    origin_turn,
                    notify_on_complete: true,
                    completed: false,
                    dismissed: false,
                });
            }
        }
        self.save(cx);
    }

    /// No Mono reports on session `id` any more: an app caller stopped,
    /// archived or deleted it (MO-26).
    pub(crate) fn dismiss_launches(&mut self, id: i64, cx: &mut Context<Self>) {
        let mut dismissed = false;
        for launch in self
            .roster
            .monos
            .iter_mut()
            .flat_map(|m| m.launches.iter_mut())
            .filter(|l| l.session_id == id)
        {
            launch.dismissed = true;
            dismissed = true;
        }
        if dismissed {
            self.save(cx);
        }
    }

    /// Mono `mono`'s files changed under a call: its panels read them again
    /// (MO-18).
    pub(crate) fn files_written(&mut self, mono: &str, cx: &mut Context<Self>) {
        self.reload_files(mono, cx);
        self.changed(cx);
    }
}
