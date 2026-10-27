//! `session_list`, `session_read`, `session_status`, and how operations
//! find the projects and sessions they name.
use super::*;
use crate::notifications::approvals::InputKind;
use monocode_control::{
    AccessMode,
    ops::{
        self,
        session::{
            ArchivedFilter, ContextUsage, DEFAULT_SESSIONS_PER_PAGE, MAX_SESSIONS_PER_PAGE,
            SessionListInput, SessionListOutput, SessionReadInput, SessionReadOutput,
            SessionStatus as Status, SessionStatusOutput, SessionSummary, Text, Turn,
        },
    },
};
use monocode_core::files::path_key;

/// Remote projects and their sessions are read-only to control clients.
pub(in crate::shell::control) fn remote_unsupported() -> ControlError {
    ControlError::new(
        ErrorCode::RemoteUnsupported,
        "Remote projects cannot be changed over external control yet.",
    )
}

/// The access mode a session a client starts or hands off gets: the one it
/// asks for, else the app's default for new chats.
pub(in crate::shell::control) fn access_or_default(
    requested: Option<AccessMode>,
    cx: &App,
) -> monocode_core::agent::AccessMode {
    requested.map_or_else(|| crate::conversation::default_access(cx), Into::into)
}

/// There is no session `id`.
pub(in crate::shell::control) fn no_session(id: i64) -> ControlError {
    ControlError::new(
        ErrorCode::NotFound,
        format!("There is no session {id}; see session_list."),
    )
}

impl Shell {
    /// The project `project` names by path or key, or the current one.
    pub(in crate::shell::control) fn control_project(
        &self,
        project: Option<&str>,
    ) -> Result<String, ControlError> {
        let Some(wanted) = project else {
            return Ok(self.workspace.project.clone());
        };
        let key = path_key(wanted);
        self.workspace
            .projects
            .iter()
            .find(|path| path.as_str() == wanted || path_key(path) == key)
            .cloned()
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::ProjectNotOpen,
                    format!(
                        "{wanted} is not a project in MonoCode; see project_list or project_add."
                    ),
                )
            })
    }
    /// The project `caller`'s call acts on, as `project` names it, or by
    /// default: for a Mono the assigned project it resolves to
    /// (`Mono::project`), whether or not the window lists it, as the
    /// bridge's calls did; for anyone else a project of the window
    /// ([`Self::control_project`]).
    pub(in crate::shell::control) fn control_project_for(
        &self,
        caller: &CallerKind,
        project: Option<&str>,
        cx: &App,
    ) -> Result<String, ControlError> {
        match caller {
            CallerKind::Mono { mono, .. } => {
                let mono = self
                    .caller_mono(mono, cx)
                    .ok_or_else(|| ControlError::new(ErrorCode::NotFound, caller::MONO_GONE))?;
                defaults::mono_project(mono, project)
            }
            CallerKind::Session(_) | CallerKind::External(_) => self.control_project(project),
        }
    }
    /// As [`Self::control_project_for`], refusing remote projects.
    pub(in crate::shell::control) fn control_local_project_for(
        &self,
        caller: &CallerKind,
        project: Option<&str>,
        cx: &App,
    ) -> Result<String, ControlError> {
        let path = self.control_project_for(caller, project, cx)?;
        if is_remote_path(&path) {
            return Err(remote_unsupported());
        }
        Ok(path)
    }
    /// As [`Self::control_project`], refusing remote projects.
    pub(in crate::shell::control) fn control_local_project(
        &self,
        project: Option<&str>,
    ) -> Result<String, ControlError> {
        let path = self.control_project(project)?;
        if is_remote_path(&path) {
            return Err(remote_unsupported());
        }
        Ok(path)
    }
    /// Checks that session `id` exists and is a project session, not a Mono's
    /// own chat or habit run, nor a launch a Mono keeps out of the sidebar.
    pub(in crate::shell::control) fn control_session(
        &self,
        id: i64,
        cx: &App,
    ) -> Result<(), ControlError> {
        self.control_visible(id, false, cx)
    }
    /// As [`Self::control_session`] for `caller`'s call: a Mono also reaches
    /// the launches Monos keep out of the sidebar, as its old app actions
    /// did (MO-22); nobody reaches a Mono's own chat or habit run.
    pub(in crate::shell::control) fn control_session_for(
        &self,
        caller: &CallerKind,
        id: i64,
        cx: &App,
    ) -> Result<(), ControlError> {
        let launches = match caller {
            CallerKind::Mono { .. } => true,
            CallerKind::Session(_) | CallerKind::External(_) => false,
        };
        self.control_visible(id, launches, cx)
    }
    /// As [`Self::control_session_for`] for a call that only reads: a
    /// session's read also reaches the launches Monos keep out of the
    /// sidebar, as Operator's reads did (OP-79). Changing one stays the
    /// Mono's.
    pub(in crate::shell::control) fn control_readable_for(
        &self,
        caller: &CallerKind,
        id: i64,
        cx: &App,
    ) -> Result<(), ControlError> {
        let launches = match caller {
            CallerKind::Mono { .. } | CallerKind::Session(_) => true,
            CallerKind::External(_) => false,
        };
        self.control_visible(id, launches, cx)
    }
    /// Checks that session `id` is a project session, showing hidden
    /// launches when `launches` says so.
    fn control_visible(&self, id: i64, launches: bool, cx: &App) -> Result<(), ControlError> {
        let mono = self.monos.as_ref().is_some_and(|m| {
            let m = m.read(cx);
            m.is_mono_session(id) || (!launches && m.hidden_session(id))
        });
        if !self.session_summaries.contains_key(&id) || self.deleting.contains(&id) || mono {
            return Err(no_session(id));
        }
        Ok(())
    }
    /// As [`Self::control_session`], refusing remote sessions.
    pub(in crate::shell::control) fn control_local_session(
        &self,
        id: i64,
        cx: &App,
    ) -> Result<(), ControlError> {
        self.control_session(id, cx)?;
        self.control_local_cwd(id, cx)
    }
    /// As [`Self::control_session_for`], refusing remote sessions.
    pub(in crate::shell::control) fn control_local_session_for(
        &self,
        caller: &CallerKind,
        id: i64,
        cx: &App,
    ) -> Result<(), ControlError> {
        self.control_session_for(caller, id, cx)?;
        self.control_local_cwd(id, cx)
    }
    /// Refuses session `id` when it is remote.
    pub(in crate::shell::control) fn control_local_cwd(
        &self,
        id: i64,
        cx: &App,
    ) -> Result<(), ControlError> {
        if is_remote_path(&self.session_metadata(id, cx).summary.cwd) {
            return Err(remote_unsupported());
        }
        Ok(())
    }
    /// The chat of session `id`, loaded if it was not.
    pub(in crate::shell::control) fn control_view(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Result<Entity<Conversation>, ControlError> {
        self.ensure_session(id, cx).ok_or_else(|| no_session(id))
    }
    /// What session `id` is doing; a chat that is not loaded is idle unless
    /// its remote host reports a turn.
    pub(in crate::shell::control) fn control_status_of(&self, id: i64, cx: &App) -> Status {
        match self.sessions.get(&id) {
            Some(view) => view.read(cx).control_status(),
            None if self.session_metadata(id, cx).busy => Status::Working,
            None => Status::Idle,
        }
    }

    fn control_summary(&self, id: i64, cx: &App) -> SessionSummary {
        let metadata = self.session_metadata(id, cx);
        let live = self.sessions.get(&id).map(|v| v.read(cx));
        SessionSummary {
            id,
            title: metadata.display_title(),
            project: metadata.summary.cwd.clone(),
            harness: metadata.summary.provider.clone(),
            model: live
                .and_then(Conversation::current_model)
                .or_else(|| self.session_details.get(&id).and_then(|m| m.model.clone())),
            status: self.control_status_of(id, cx),
            busy: metadata.busy,
            archived: metadata.summary.archived,
            has_draft: metadata.has_saved_draft()
                || live.is_some_and(|v| !v.input.read(cx).content.is_empty()),
            started_by: self
                .control
                .started
                .get(&id)
                .cloned()
                .or_else(|| self.started_by.get(&id).map(|c| format!("session:{c}"))),
            updated_at: i64::try_from(metadata.summary.updated_at).unwrap_or(i64::MAX),
        }
    }

    /// The sessions of `input`'s project, as a client sees them.
    pub(in crate::shell::control) fn control_session_list(
        &self,
        input: &SessionListInput,
        cx: &App,
    ) -> Result<SessionListOutput, ControlError> {
        let project = self.control_project(input.project.as_deref())?;
        self.control_sessions_of(input, project, false, cx)
    }

    /// The sessions of `input`'s project, as `caller` sees them. A Mono also
    /// lists the launches Monos keep out of the sidebar, wherever the window
    /// keeps them, and orchestration workers, as its old app actions did
    /// (MO-22).
    pub(super) fn control_session_list_for(
        &self,
        input: &SessionListInput,
        caller: &CallerKind,
        cx: &App,
    ) -> Result<SessionListOutput, ControlError> {
        let project = self.control_project_for(caller, input.project.as_deref(), cx)?;
        let mono = match caller {
            CallerKind::Mono { .. } => true,
            CallerKind::Session(_) | CallerKind::External(_) => false,
        };
        self.control_sessions_of(input, project, mono, cx)
    }

    fn control_sessions_of(
        &self,
        input: &SessionListInput,
        project: String,
        mono: bool,
        cx: &App,
    ) -> Result<SessionListOutput, ControlError> {
        let limit = ops::limit(
            input.limit,
            DEFAULT_SESSIONS_PER_PAGE,
            MAX_SESSIONS_PER_PAGE,
        )?;
        let key = path_key(&project);
        let archived = input.archived.unwrap_or_default();
        let listed: HashSet<i64> = self.order.iter().copied().collect();
        let mut unlisted: Vec<i64> = self
            .session_summaries
            .keys()
            .copied()
            .filter(|id| mono && !listed.contains(id))
            .collect();
        unlisted.sort_unstable();
        let mut all: Vec<_> = self
            .order
            .iter()
            .copied()
            .chain(unlisted)
            .filter(|id| {
                self.session_summaries.contains_key(id)
                    && self.control_visible(*id, mono, cx).is_ok()
                    && (mono || !self.orchestration_worker(*id))
            })
            .map(|id| self.control_summary(id, cx))
            .filter(|s| path_key(&s.project) == key)
            .filter(|s| match archived {
                ArchivedFilter::Exclude => !s.archived,
                ArchivedFilter::Include => true,
                ArchivedFilter::Only => s.archived,
            })
            .collect();
        all.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        let (sessions, next_cursor) = ops::page(&all, input.cursor.as_deref(), limit)?;
        Ok(SessionListOutput {
            project,
            sessions,
            total: u32::try_from(all.len()).unwrap_or(u32::MAX),
            next_cursor,
        })
    }

    pub(super) fn control_session_read(
        &mut self,
        input: SessionReadInput,
        caller: &CallerKind,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        let id = input.session_id;
        if input.before.is_some() && input.before_index.is_some() {
            return Err(ControlError::invalid_input(
                "Pass before or beforeIndex, not both.",
            ));
        }
        self.control_readable_for(caller, id, cx)?;
        let metadata = self.session_metadata(id, cx);
        let busy = metadata.busy;
        let draft = metadata.has_saved_draft();
        let mut page = serde_json::json!({
            "limit": input.limit.unwrap_or(3),
            "maxChars": input.max_chars.unwrap_or(1200),
        });
        if let Some(before) = input.before {
            page["before"] = before.into();
        }
        // A number is Operator's turn index, clamped to the turns there are.
        if let Some(index) = input.before_index {
            page["before"] = index.into();
        }
        let store = self.store.clone();
        Ok(Exec::Later(cx.background_executor().spawn(async move {
            let page = locked(&store)
                .map_err(ControlError::internal)?
                .app_conversation(id, &page, busy, draft)
                .map_err(ControlError::invalid_input)?;
            output(&read_output(id, &page))
        })))
    }

    pub(super) fn control_session_status(
        &mut self,
        id: i64,
        caller: &CallerKind,
        cx: &mut Context<Self>,
    ) -> Result<Exec, ControlError> {
        self.control_readable_for(caller, id, cx)?;
        let (mut status, loaded) = self.control_status_now(id, cx);
        if loaded {
            return ready(Ok(status));
        }
        // Not loaded: its queue is in the store.
        let store = self.store.clone();
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Ok(Exec::Later(cx.background_executor().spawn(async move {
            let queued = locked(&store)
                .and_then(|s| s.follow_ups(id))
                .map_err(ControlError::internal)?;
            status.queue_length = count(queued.messages.len());
            output(&status)
        })))
    }

    /// What `session_status` shows from the window's own state, and whether
    /// the chat is loaded; an unloaded chat's queue length is left at 0.
    pub(in crate::shell::control) fn control_status_now(
        &self,
        id: i64,
        cx: &App,
    ) -> (SessionStatusOutput, bool) {
        let metadata = self.session_metadata(id, cx);
        let mut status = SessionStatusOutput {
            session_id: id,
            project: metadata.summary.cwd.clone(),
            status: self.control_status_of(id, cx),
            status_text: "Ready".into(),
            busy: metadata.busy,
            pending_approvals: 0,
            pending_questions: 0,
            queue_length: 0,
            context: None,
            harness: metadata.summary.provider.clone(),
            model: self.session_details.get(&id).and_then(|m| m.model.clone()),
            access: self
                .session_details
                .get(&id)
                .map(|m| m.access)
                .unwrap_or_default()
                .into(),
        };
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        if let Some(view) = self.sessions.get(&id).map(|v| v.read(cx)) {
            let pending = view.pending_notification_inputs(cx);
            let of = |kind| pending.iter().filter(|p| p.kind == kind).count();
            let (approvals, questions) = (of(InputKind::Approval), of(InputKind::Question));
            status.status_text = view.status_text();
            status.pending_approvals = count(approvals);
            status.pending_questions = count(questions);
            status.queue_length = count(view.queue_length());
            status.context = view.context_usage().map(|usage| ContextUsage {
                used: usage.used,
                window: usage.window,
            });
            status.model = view.current_model().or(status.model);
            status.access = view.settings.access.into();
            return (status, true);
        }
        (status, false)
    }
}

/// `app_conversation`'s page as `session_read`'s output.
fn read_output(id: i64, page: &Value) -> SessionReadOutput {
    let text = |value: &Value| Text {
        text: value["text"].as_str().unwrap_or_default().to_owned(),
        truncated: value["truncated"].as_bool().unwrap_or(false),
    };
    let id_text = |value: &Value| {
        value
            .as_str()
            .map_or_else(|| value.to_string(), str::to_owned)
    };
    SessionReadOutput {
        session_id: id,
        title: page["title"].as_str().map(str::to_owned),
        busy: page["busy"].as_bool().unwrap_or(false),
        has_draft: page["hasDraft"].as_bool().unwrap_or(false),
        turns: page["turns"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|turn| Turn {
                turn_id: id_text(&turn["turnId"]),
                user: text(&turn["user"]),
                assistant: turn["assistant"]
                    .is_object()
                    .then(|| text(&turn["assistant"])),
                earlier_assistant_messages: turn["earlierAssistantMessages"]
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .unwrap_or(0),
            })
            .collect(),
        next_before: page.get("nextBefore").filter(|v| !v.is_null()).map(id_text),
    }
}
