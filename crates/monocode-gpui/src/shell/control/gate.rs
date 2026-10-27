//! `control_grant`, the one place that decides whether a caller may make a
//! call. An agent session's calls get Operator's rules first: it must still
//! exist and be local, it may not act on itself where the operation forbids
//! that, it may not change an orchestration's workers, and a read-only turn
//! (plan, orchestrator lead, read-only access, or a chat that is not loaded)
//! may only read. A Mono's calls get the same rules on the chat it calls
//! from, and keep to the operations a Mono had and to its assigned projects;
//! its habit runs may not schedule more runs. Operations only a Mono may call
//! are refused to everyone else. Then every caller needs the call's scopes.
//! A refusal is logged and keeps no receipt.
use super::*;
use monocode_control::{
    Command, Operation,
    ops::{
        mono_artifact::{MonoArtifactList, MonoArtifactRead, MonoArtifactWrite},
        mono_card::MonoCardPost,
        mono_habit::{MonoHabitAdd, MonoHabitList, MonoHabitRemove, MonoHabitRun, MonoHabitUpdate},
        mono_memory::{
            MonoMemoryAdd, MonoMemoryRead, MonoMemoryRemove, MonoMemoryReplace, MonoMemorySearch,
        },
        mono_soul::{MonoSoulRead, MonoSoulUpdate},
    },
};
use monocode_core::{agent::AccessMode, files::path_key, mono::Mono, remote::is_remote_path};

/// What a read-only thread is told when it tries to change something, in
/// Operator's words.
pub(super) const READ_ONLY: &str = "This thread's access policy allows read-only app actions.";

/// What a Mono may call: the equivalents of its old app actions, reads on
/// its assigned projects, and the operations only a Mono may call
/// (`Operation::MONO_ONLY`) on its own memory, soul, habits, artifacts and
/// chat (§7.3). Everything else is refused.
pub(crate) const MONO_OPS: [&str; 37] = [
    "model_list",
    "session_list",
    "session_read",
    "session_start",
    "session_send",
    "session_draft",
    "session_stop",
    "session_archive",
    "session_delete",
    "folder_list",
    "session_move",
    "worktree_list",
    "worktree_create",
    "notes_list",
    "notes_read",
    "notes_write",
    "session_status",
    "session_wait",
    "changes_read",
    "search_project",
    "queue_list",
    MonoMemoryRead::NAME,
    MonoMemorySearch::NAME,
    MonoMemoryAdd::NAME,
    MonoMemoryReplace::NAME,
    MonoMemoryRemove::NAME,
    MonoSoulRead::NAME,
    MonoSoulUpdate::NAME,
    MonoHabitList::NAME,
    MonoHabitAdd::NAME,
    MonoHabitUpdate::NAME,
    MonoHabitRun::NAME,
    MonoHabitRemove::NAME,
    MonoArtifactList::NAME,
    MonoArtifactRead::NAME,
    MonoArtifactWrite::NAME,
    MonoCardPost::NAME,
];

/// The operations that change a Mono's habits, which its habit runs may not
/// call (MO-8).
pub(super) const HABIT_MUTATIONS: [&str; 4] = [
    MonoHabitAdd::NAME,
    MonoHabitUpdate::NAME,
    MonoHabitRun::NAME,
    MonoHabitRemove::NAME,
];

/// A session outside a Mono's assigned projects, in the bridge's words.
pub(super) const OUTSIDE: &str = "Session is outside the assigned project";

fn scope_denied(message: impl Into<String>) -> ControlError {
    ControlError::new(ErrorCode::ScopeDenied, message)
}

/// What a caller that is not a Mono is told for `operation`, which only a
/// Mono may call.
pub(super) fn mono_only_denied(operation: &str) -> ControlError {
    scope_denied(format!("{operation} is only available in a Mono's chat"))
}

/// Whether a caller whose live chat is `view` may only read: its running
/// turn is a plan or orchestrator turn or read-only (`app_read_only`), its
/// composer is in plan mode or its access is read-only right now, or its
/// chat is not loaded, so nothing says otherwise.
fn read_only(view: Option<&Conversation>) -> bool {
    view.is_none_or(|v| {
        v.app_read_only || v.settings.plan || v.settings.access == AccessMode::ReadOnly
    })
}

/// What an app caller is told when its call would change orchestration
/// worker `id`.
fn worker_protected(id: i64) -> ControlError {
    ControlError::invalid_input(format!(
        "Session {id} belongs to an orchestration; only its lead manages it"
    ))
}

/// The project a Mono's call names itself, for the operations a Mono may
/// call that take one.
fn named_project(command: &Command) -> Option<&str> {
    match command {
        Command::SessionList(input) => input.project.as_deref(),
        Command::SessionStart(input) => input.project.as_deref(),
        Command::FolderList(input) | Command::WorktreeList(input) => input.project.as_deref(),
        Command::WorktreeCreate(input) => input.project.as_deref(),
        Command::NotesList(input) => input.project.as_deref(),
        Command::NotesWrite(input) => input.project.as_deref(),
        Command::SearchProject(input) => input.project.as_deref(),
        Command::ChangesRead(input) => input.project.as_deref(),
        _ => None,
    }
}

impl Shell {
    /// The caller's current grant, if it may make the call: a session
    /// caller passes [`Self::session_call_allowed`] first, a Mono
    /// [`Self::mono_call_allowed`]; then every caller needs the scopes the
    /// call needs (`Command::scopes`).
    pub(super) fn control_grant(
        &self,
        caller: &CallerKind,
        command: &Command,
        cx: &App,
    ) -> Result<Grant, ControlError> {
        match caller {
            CallerKind::Session(me) => self.session_call_allowed(*me, command, cx)?,
            CallerKind::Mono { session, mono, run } => {
                self.mono_call_allowed(*session, mono, *run, command, cx)?;
            }
            CallerKind::External(_) => Self::not_mono_only(command)?,
        }
        let scopes = command.scopes();
        let grant = self.control_scope(caller, scopes[0], command.name(), cx)?;
        for scope in &scopes[1..] {
            self.control_scope(caller, *scope, command.name(), cx)?;
        }
        Ok(grant)
    }

    /// Refuses an operation only a Mono may call to every other caller.
    fn not_mono_only(command: &Command) -> Result<(), ControlError> {
        if command.mono_only() {
            return Err(mono_only_denied(command.name()));
        }
        Ok(())
    }

    /// Operator's rules for session `me`'s call, in order: it exists, is
    /// local, calls no Mono's operation, does not target itself where the
    /// operation forbids that (`Operation::NOT_ON_SELF`), changes no
    /// orchestration worker, and only reads while its turn is read-only.
    fn session_call_allowed(
        &self,
        me: i64,
        command: &Command,
        cx: &App,
    ) -> Result<(), ControlError> {
        self.session_exists(me)?;
        Self::local_caller(&self.session_metadata(me, cx).summary.cwd)?;
        Self::not_mono_only(command)?;
        self.target_allowed(me, command)?;
        self.project_delete_allowed(me, command)?;
        if !command.hints().read_only && self.session_read_only(me, cx) {
            return Err(scope_denied(READ_ONLY));
        }
        Ok(())
    }

    /// A session's `project_delete`, which nobody confirms for it: not of
    /// the project it is in, nor of one with an orchestration's workers,
    /// which only their lead manages. (Monos cannot call it: `MONO_OPS`.)
    fn project_delete_allowed(&self, me: i64, command: &Command) -> Result<(), ControlError> {
        let Command::ProjectDelete(input) = command else {
            return Ok(());
        };
        // An unknown project is refused when the call runs.
        let path = self
            .control_project(Some(&input.project))
            .unwrap_or_else(|_| input.project.clone());
        let key = path_key(&path);
        let sessions: Vec<i64> = self
            .session_summaries
            .values()
            .filter(|summary| path_key(&summary.cwd) == key)
            .map(|summary| summary.id)
            .collect();
        if sessions.contains(&me) {
            return Err(ControlError::invalid_input(
                "A session cannot delete its own project.",
            ));
        }
        match sessions
            .into_iter()
            .find(|id| self.orchestration_worker(*id))
        {
            Some(worker) => Err(worker_protected(worker)),
            None => Ok(()),
        }
    }

    /// The rules for Mono `mono`'s call from session `me` (its main chat,
    /// or with `run` a running habit), in order: the Mono still calls from
    /// it, it is local, the operation is one a Mono may call and, from a
    /// habit run, changes no habit; then Operator's target rules and
    /// read-only gate on the chat it calls from; then the call stays in the
    /// Mono's assigned projects.
    fn mono_call_allowed(
        &self,
        me: i64,
        mono: &str,
        run: bool,
        command: &Command,
        cx: &App,
    ) -> Result<(), ControlError> {
        self.mono_caller_exists(me, mono, run, cx)?;
        let monos = self.monos.as_ref().map(|m| m.read(cx));
        let view = monos.and_then(|m| m.caller_view(me, mono, run, cx));
        let view = view.as_ref().map(|v| v.read(cx));
        if let Some(view) = view {
            Self::local_caller(&view.summary.cwd)?;
        }
        let name = command.name();
        if !MONO_OPS.contains(&name) {
            return Err(scope_denied(format!("Monos cannot use {name}")));
        }
        if run && HABIT_MUTATIONS.contains(&name) {
            return Err(scope_denied("A habit run cannot schedule more runs"));
        }
        self.target_allowed(me, command)?;
        if !command.hints().read_only && read_only(view) {
            return Err(scope_denied(READ_ONLY));
        }
        match monos.and_then(|m| m.roster.get(mono)) {
            Some(mono) => self.in_mono_projects(mono, command, cx),
            None => Err(ControlError::new(ErrorCode::NotFound, caller::MONO_GONE)),
        }
    }

    /// The checks a call's target gets before it runs, as its operation
    /// declares them: a remote session is read-only to clients
    /// (`Operation::LOCAL_ONLY`).
    pub(super) fn control_target(
        &self,
        caller: &CallerKind,
        command: &monocode_control::Command,
        cx: &App,
    ) -> Result<(), ControlError> {
        match command.target_session() {
            Some(id) if command.local_only() => self.control_local_session_for(caller, id, cx),
            _ => Ok(()),
        }
    }

    /// Refuses an app caller whose own session is remote.
    fn local_caller(cwd: &str) -> Result<(), ControlError> {
        if is_remote_path(cwd) {
            return Err(ControlError::new(
                ErrorCode::RemoteUnsupported,
                "MonoCode tools are not available on remote projects",
            ));
        }
        Ok(())
    }

    /// The target rules of an app caller's call from session `me`: not on
    /// itself where the operation forbids that, and no change to an
    /// orchestration worker, which only its lead manages.
    fn target_allowed(&self, me: i64, command: &Command) -> Result<(), ControlError> {
        let target = command.target_session();
        if command.not_on_self() && target == Some(me) {
            return Err(ControlError::invalid_input(format!(
                "{} cannot target session {me}: that is the session making this call. A session may not act on itself this way (stop, archive, delete, reconfigure, wait on, message or leave a draft); name another session.",
                command.name()
            )));
        }
        let reads = command.hints().read_only;
        if let Some(target) = target.filter(|id| !reads && self.orchestration_worker(*id)) {
            return Err(worker_protected(target));
        }
        Ok(())
    }

    /// Whether a Mono's call stays in its assigned projects: a project it
    /// names resolves among them by path or unique name (`Mono::project`,
    /// MO-15), and a session it acts on is in one of them.
    fn in_mono_projects(
        &self,
        mono: &Mono,
        command: &Command,
        cx: &App,
    ) -> Result<(), ControlError> {
        if let Some(project) = named_project(command) {
            mono.project(Some(project)).map_err(scope_denied)?;
        }
        let known =
            |id: &i64| self.sessions.contains_key(id) || self.session_summaries.contains_key(id);
        if let Some(target) = command.target_session().filter(known)
            && !mono.works_on(&self.session_metadata(target, cx).summary.cwd)
        {
            return Err(scope_denied(OUTSIDE));
        }
        Ok(())
    }

    /// Whether session `id`'s calls may only read ([`read_only`] on its
    /// chat).
    pub(super) fn session_read_only(&self, id: i64, cx: &App) -> bool {
        read_only(self.sessions.get(&id).map(|v| v.read(cx)))
    }
}
