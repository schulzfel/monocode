//! Starting a session in a target project with explicit launch options.
//! `session_start` from an agent session, a Mono or an external client runs
//! this pipeline; only who started the session, and so the idempotency key
//! and the started-by record, differ. `launch_settings`
//! strips Operator, plan, orchestration and environment from every launch,
//! so a launched session never gets control capability of its own.
use super::*;
use monocode_core::{
    accounts::Profile, agent::Settings, app_access as app, mono::files::content_hash,
};
use serde_json::Value;

/// Who started a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    /// Another agent session.
    Session(i64),
    /// An external control client, recorded as `mcp:<client>`.
    Client(String),
    /// Mono `mono`, from its main chat or habit run `caller`. It records the
    /// launch in its roster, not as a started-by link.
    Mono { caller: i64, mono: String },
}

/// A launch resolved on the UI thread, ready for a worker.
pub(super) struct Launch {
    pub(super) project: String,
    pub(super) kind: Kind,
    /// The provider `settings` came from; another provider resets the model.
    pub(super) source_provider: String,
    pub(super) settings: Settings,
    /// Operator-shaped options: `model`, `effort`, `modelSettings`,
    /// `runtimeMode`, `workspaceMode`, `worktreeCwd`, `worktreeBase`, `title`.
    pub(super) input: Value,
    pub(super) prompt: String,
    pub(super) origin: Origin,
    /// Makes a repeated launch find its first session: Operator's request ID,
    /// or `control:<client>:<request id>`.
    pub(super) request: String,
    pub(super) fingerprint: String,
}

/// A launched (or, on a repeat, found) session.
pub(super) struct Launched {
    pub(super) summary: Session,
    pub(super) settings: Settings,
    /// Whether this call created it.
    pub(super) fresh: bool,
}

/// What a launch by an agent session starts from, read on the UI thread.
pub(super) struct Source {
    /// The new session's provider.
    pub(super) kind: Kind,
    /// The provider `settings` came from.
    pub(super) provider: String,
    pub(super) settings: Settings,
}

/// What a new chat starts from, read on the UI thread.
pub(super) struct NewChat {
    pub(super) kind: Kind,
    pub(super) cwd: String,
    pub(super) model: Option<String>,
    pub(super) account: Profile,
    pub(super) access: monocode_core::agent::AccessMode,
    pub(super) effort: Option<String>,
}

impl NewChat {
    /// The settings a new chat starts with: the project's or app's defaults
    /// for its provider, with this window's account, access and effort.
    pub(super) fn settings(&self, store: &monocode_core::store::Store) -> Settings {
        let mut settings = session_loading::new_session_settings(
            store
                .desktop_preferences()
                .and_then(|preferences| preferences.session_defaults(self.kind.id(), &self.cwd)),
            self.model.clone(),
        );
        settings.access = self.access;
        if let Some(effort) = self.effort.clone() {
            settings.model_settings.retain(|key, _| {
                !matches!(
                    key.as_str(),
                    "effort" | "reasoning" | "reasoningEffort" | "reasoning_effort"
                )
            });
            settings.effort = Some(effort);
        }
        settings.account = self.account.clone();
        settings
    }
}

/// Runs on a worker: finds the launch's session or creates it with its
/// checkout, title, agent settings, started-by record and durable prompt.
pub(super) fn launch_session(store: &Database, launch: Launch) -> Result<Launched, String> {
    let Launch {
        project,
        kind,
        source_provider,
        settings,
        input,
        prompt,
        origin,
        request,
        fingerprint,
    } = launch;
    let models = if input.get("model").is_some()
        || input.get("modelSettings").is_some()
        || input.get("effort").is_some()
    {
        monocode_core::catalog::models_for(kind, &settings.account)?
    } else {
        monocode_core::catalog::cached_models_for(kind, &settings.account).unwrap_or_default()
    };
    let mut settings = app::launch_settings(&input, &source_provider, settings, kind, &models)?;
    settings.revision = 0;
    let title = app::optional_text(&input, "title", 160)?
        .unwrap_or_else(|| prompt.chars().take(160).collect());
    let mut store = locked(store)?;
    let (mut summary, fresh) = match &origin {
        Origin::Session(caller) | Origin::Mono { caller, .. } => {
            store.create_app_session(*caller, &request, &fingerprint, kind.id(), &project)?
        }
        Origin::Client(_) => store.create_keyed_session(
            &format!("control-launch:{}", content_hash(&request)),
            &format!("control-{}", content_hash(&request)),
            &fingerprint,
            kind.id(),
            &project,
        )?,
    };
    if fresh {
        if let Err(error) = app::launch_checkout(&input, &project, summary.id, &mut settings) {
            let _ = store.delete(summary.id);
            return Err(error);
        }
        store.title(summary.id, &title)?;
        summary.title = title;
        store.configure_agent(summary.id, kind.id(), &settings)?;
        match &origin {
            Origin::Session(caller) => store.save_started_link(summary.id, *caller)?,
            Origin::Client(client) => store.save_started_client(summary.id, client)?,
            // The Mono records it in its roster once it is installed.
            Origin::Mono { .. } => {}
        }
        store.prepare_app_prompt(summary.id, &request, &prompt)?;
        summary.has_messages = true;
    }
    Ok(Launched {
        summary,
        settings,
        fresh,
    })
}

impl Shell {
    /// What a launch in `project` by session `caller` starts from, as
    /// Operator's do: the caller's live settings, and its provider unless
    /// `harness` names another. The same provider keeps the caller's
    /// account; another gets the one `project` selects. A launch into
    /// another project leaves the caller's worktree behind. `None` while
    /// the caller's chat is not loaded.
    pub(super) fn session_source(
        &self,
        caller: i64,
        harness: Option<Kind>,
        project: &str,
        cx: &App,
    ) -> Option<Source> {
        let view = self.sessions.get(&caller)?.read(cx);
        let provider = view.summary.provider.clone();
        let kind = harness
            .or_else(|| Kind::parse(&provider))
            .unwrap_or_else(|| self.default_kind_for(project));
        let mut settings = view.settings.clone();
        if monocode_core::files::path_key(&view.summary.cwd)
            != monocode_core::files::path_key(project)
        {
            settings.worktree_cwd = None;
        }
        if kind.id() != provider {
            settings.account = crate::accounts::get(cx)
                .map(|center| center.read(cx).selected(kind, project))
                .unwrap_or_default();
        }
        Some(Source {
            kind,
            provider,
            settings,
        })
    }

    /// What a launch in `project` by Mono `mono`, calling from session
    /// `caller`, starts from, as the bridge's did (MO-16, MO-20): the Mono's
    /// main chat, else the calling habit run, and its provider unless
    /// `harness` names another, else Codex. The same provider keeps the
    /// source's account; another gets the one `project` selects. A launch
    /// outside the source's own folder leaves its worktree behind.
    pub(super) fn mono_source(
        &self,
        caller: i64,
        mono: &str,
        harness: Option<Kind>,
        project: &str,
        cx: &App,
    ) -> Source {
        let view = self
            .monos
            .as_ref()
            .and_then(|m| m.read(cx).source_view(caller, mono));
        let view = view.as_ref().map(|v| v.read(cx));
        let kind = harness
            .or_else(|| view.and_then(|v| Kind::parse(&v.summary.provider)))
            .unwrap_or(Kind::Codex);
        let provider = view.map_or_else(|| kind.id().to_owned(), |v| v.summary.provider.clone());
        let mut settings = view.map(|v| v.settings.clone()).unwrap_or_default();
        if view.is_none_or(|v| {
            monocode_core::files::path_key(&v.summary.cwd)
                != monocode_core::files::path_key(project)
        }) {
            settings.worktree_cwd = None;
        }
        if kind.id() != provider {
            settings.account = crate::accounts::get(cx)
                .map(|center| center.read(cx).selected(kind, project))
                .unwrap_or_default();
        }
        Source {
            kind,
            provider,
            settings,
        }
    }

    /// What a new chat of `kind` in `cwd` starts from.
    pub(super) fn new_chat(&self, kind: Kind, cwd: &str, cx: &App) -> NewChat {
        NewChat {
            kind,
            cwd: cwd.to_owned(),
            model: self
                .workspace
                .preferences
                .project_providers
                .get(cwd)
                .and_then(|p| p.models.get(kind.id()))
                .or(self.workspace.preferences.default_models.get(kind.id()))
                .cloned(),
            account: crate::accounts::get(cx)
                .map(|center| center.read(cx).selected(kind, cwd))
                .unwrap_or_default(),
            access: crate::conversation::default_access(cx),
            effort: crate::conversation::preferred_effort(kind.id(), cx),
        }
    }

    /// Shows a freshly launched session: it joins the sidebar as a draft,
    /// records who started it, installs its settings (sending its saved
    /// prompt unless `draft`), and is placed beside a pane or revealed.
    pub(super) fn install_launch(
        &mut self,
        launched: Launched,
        origin: Origin,
        (draft, reveal): (bool, bool),
        placement: Option<(i64, Edge)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Launched {
            summary, settings, ..
        } = launched;
        let id = summary.id;
        self.draft_sessions.insert(id);
        self.sidebar_hidden_sessions.remove(&id);
        self.session_worktrees
            .insert(id, settings.worktree_cwd.clone());
        self.add(summary, cx);
        self.record_started(id, origin);
        if let Some(view) = self.sessions.get(&id) {
            view.update(cx, |v, cx| v.install_app_launch(settings, !draft, cx));
        }
        self.app_place_launch(id, placement, reveal, window, cx);
    }

    /// Places launched session `id`: right of or below an open pane, which
    /// it then focuses, or else revealed in its own tab when `reveal`.
    pub(crate) fn app_place_launch(
        &mut self,
        id: i64,
        placement: Option<(i64, Edge)>,
        reveal: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((target, edge)) =
            placement.filter(|(target, _)| self.workspace.tab_of(*target).is_some())
        {
            self.open(target, window, cx);
            let before = self.active_leaves();
            self.workspace.place(id, target, edge);
            self.begin_pane_motion(before, cx);
            self.fit_sidebar(cx);
            self.activate_session(id, window, cx);
        } else if reveal {
            self.open(id, window, cx);
        }
        self.persist(cx);
    }
}
