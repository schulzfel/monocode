//! The operations only a Mono calls, run on its own things as the old
//! bridge ran its actions: memory, soul and habits in its folder
//! (`control_files`), artifacts in its roster entry and cards in its chat,
//! through the same core functions, so limits and words carry over. A
//! habit run's cards and artifact cards go with its report (`hold_card`).
use super::*;
use monocode_control::{
    Command, ControlError, ErrorCode,
    ops::{
        self,
        mono_artifact::{
            ArtifactKind, DEFAULT_ARTIFACTS_PER_PAGE, MAX_ARTIFACT_BODY_BYTES,
            MAX_ARTIFACT_SUMMARY_CHARS, MAX_ARTIFACT_TITLE_BYTES, MAX_ARTIFACTS_PER_PAGE,
            MonoArtifactListInput, MonoArtifactWriteInput,
        },
        mono_card::MonoCardPostInput,
    },
};
use monocode_core::{
    mono::{Document, files},
    time::now_ms,
};
use serde_json::{Value, json};

/// A call only a Mono makes, for [`Monos::control_own`].
pub(crate) struct Own {
    /// The calling Mono.
    pub(crate) mono: String,
    /// The session it calls from: its main chat or a habit run.
    pub(crate) caller: i64,
    pub(crate) command: Command,
    /// For a `session` card: the session's chat, as the window has it.
    pub(crate) session: Option<Entity<Conversation>>,
}

/// What a Mono's own operation answers: now, or once its work on the files
/// or with GitHub is done.
pub(crate) enum Answer {
    Now(Result<Value, ControlError>),
    Later(gpui::Task<Result<Value, ControlError>>),
}

/// A core function's refusal, in its words: a soul that changed since it
/// was read is a conflict (C-1), a habit that is gone is not found, and the
/// rest is input to change.
fn refused(message: String) -> ControlError {
    let code = if message == files::CONFLICT {
        ErrorCode::RequestConflict
    } else if message == control_files::HABIT_GONE {
        ErrorCode::NotFound
    } else {
        ErrorCode::InvalidInput
    };
    ControlError::new(code, message)
}

/// An artifact kind as the roster keeps it.
fn kind_id(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Document => "document",
        ArtifactKind::Html => "html",
        ArtifactKind::Markdown => "markdown",
    }
}

impl Monos {
    /// Runs `own`'s operation for its Mono, which the shell checked exists.
    pub(crate) fn control_own(&mut self, own: Own, cx: &mut Context<Self>) -> Answer {
        let Own {
            mono,
            caller,
            command,
            session,
        } = own;
        let Some(record) = self.roster.get(&mono).cloned() else {
            return Answer::Now(Err(ControlError::internal("The Mono is gone")));
        };
        match command {
            Command::MonoArtifactList(input) => Answer::Now(Self::artifact_list(&record, &input)),
            Command::MonoArtifactRead(input) => Answer::Now(
                record
                    .documents
                    .iter()
                    .find(|d| d.id == input.id)
                    .map(|d| json!(d))
                    .ok_or_else(|| ControlError::new(ErrorCode::NotFound, "Document not found")),
            ),
            Command::MonoArtifactWrite(input) => {
                Answer::Now(self.artifact_write(&record, caller, input, cx))
            }
            Command::MonoCardPost(input) => self.card_post(&record, caller, &input, session, cx),
            command => {
                let root = self.root.clone();
                Answer::Later(cx.background_executor().spawn(async move {
                    control_files::run(&root, &mono, command).map_err(refused)
                }))
            }
        }
    }

    /// One page of `mono`'s artifacts of the kind asked for, without bodies
    /// (C-7), in the order they were first written.
    fn artifact_list(mono: &Mono, input: &MonoArtifactListInput) -> Result<Value, ControlError> {
        let limit = ops::limit(
            input.limit,
            DEFAULT_ARTIFACTS_PER_PAGE,
            MAX_ARTIFACTS_PER_PAGE,
        )?;
        let kind = input.kind.map(kind_id);
        let listed: Vec<Value> = mono
            .documents
            .iter()
            .filter(|d| kind.is_none_or(|k| k == d.kind))
            .map(|d| {
                json!({
                    "id": d.id, "kind": d.kind, "title": d.title, "summary": d.summary,
                    "createdAt": d.created_at, "updatedAt": d.updated_at,
                })
            })
            .collect();
        let (artifacts, next) = ops::page(&listed, input.cursor.as_deref(), limit)?;
        let mut page = json!({ "artifacts": artifacts, "total": listed.len() });
        if let Some(next) = next {
            page["nextCursor"] = json!(next);
        }
        Ok(page)
    }

    /// Saves an artifact of `mono`'s as the bridge did (MO-31): a known id
    /// keeps its creation time, the cached views of it are dropped, and its
    /// card is held for the chat or habit run `caller`.
    fn artifact_write(
        &mut self,
        mono: &Mono,
        caller: i64,
        input: MonoArtifactWriteInput,
        cx: &mut Context<Self>,
    ) -> Result<Value, ControlError> {
        let kind = kind_id(input.kind.unwrap_or_default());
        let title = control_files::text(&input.title, "title", MAX_ARTIFACT_TITLE_BYTES)
            .map_err(refused)?;
        let body =
            control_files::text(&input.body, "body", MAX_ARTIFACT_BODY_BYTES).map_err(refused)?;
        let id = input.id.unwrap_or_else(mono::new_id);
        let now = now_ms();
        let document = Document {
            id: id.clone(),
            kind: kind.into(),
            title,
            summary: input
                .summary
                .unwrap_or_default()
                .chars()
                .take(MAX_ARTIFACT_SUMMARY_CHARS)
                .collect(),
            body,
            created_at: mono
                .documents
                .iter()
                .find(|d| d.id == id)
                .map_or(now, |d| d.created_at),
            updated_at: now,
        };
        if let Some(record) = self.roster.get_mut(&mono.id) {
            if let Some(old) = record.documents.iter_mut().find(|d| d.id == id) {
                *old = document.clone();
            } else {
                record.documents.push(document.clone());
            }
        }
        self.documents.remove(&id);
        self.html_documents.remove(&id);
        let card = json!({
            "id": mono::new_id(), "type": "document", "documentId": id,
            "title": document.title, "summary": document.summary,
        });
        self.hold_card(caller, &mono.id, card);
        self.save(cx);
        Ok(json!(document))
    }

    /// Posts a card as the bridge did (MO-32): parsed by `cards::parse`, a
    /// session card with its session's title and state, a pull request's
    /// details from GitHub on a worker; then held for `caller`.
    fn card_post(
        &mut self,
        mono: &Mono,
        caller: i64,
        input: &MonoCardPostInput,
        session: Option<Entity<Conversation>>,
        cx: &mut Context<Self>,
    ) -> Answer {
        let parsed = serde_json::to_value(input)
            .map_err(|e| e.to_string())
            .and_then(|wire| mono::cards::parse(&wire));
        let mut card = match parsed {
            Ok(card) => card,
            Err(error) => return Answer::Now(Err(refused(error))),
        };
        card["id"] = json!(mono::new_id());
        if let MonoCardPostInput::Session { .. } = input {
            let Some(view) = session.filter(|v| mono.works_on(&v.read(cx).summary.cwd)) else {
                return Answer::Now(Err(ControlError::invalid_input(
                    "Session is outside assigned projects",
                )));
            };
            let view = view.read(cx);
            card["title"] = json!(view.summary.title);
            // F-7: the state the chat's card refresh shows for it, in the
            // renderer's lowercase words ("working" while busy), not the
            // bridge's "Working" or "Saved" (MO-91).
            card["state"] = json!(cards::launched_state(view, cx));
        }
        if let MonoCardPostInput::Pr { .. } = input {
            let cwd = mono
                .projects
                .first()
                .cloned()
                .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| ".".into()));
            let repo = card["repo"].as_str().map(str::to_owned);
            let number = card["number"].as_u64().unwrap_or_default();
            let work = cx
                .background_executor()
                .spawn(async move { mono::cards::pull_request(&cwd, repo.as_deref(), number) });
            let id = mono.id.clone();
            return Answer::Later(cx.spawn(async move |this, cx| {
                let metadata = work.await.map_err(refused)?;
                for key in ["title", "state", "url", "isDraft", "additions", "deletions"] {
                    card[key] = metadata[key].clone();
                }
                let held = card.clone();
                this.update(cx, |this, cx| {
                    this.hold_card(caller, &id, held);
                    this.save(cx);
                })
                .map_err(|_| ControlError::internal("The Monos closed"))?;
                Ok(card)
            }));
        }
        self.hold_card(caller, &mono.id, card.clone());
        self.save(cx);
        Answer::Now(Ok(card))
    }
}
