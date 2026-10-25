//! `search_sessions` and `search_project`: the app search's own store and
//! project searches, on a worker, bounded and stopped after a deadline.
use super::*;
use monocode_control::ops::{
    self,
    search::{
        DEFAULT_FILE_MATCHES, DEFAULT_SESSION_HITS, FileMatch, HitKind, MAX_FILE_MATCHES,
        MAX_PREVIEW_CHARS, MAX_QUERY_CHARS, MAX_SESSION_HITS, PROJECT_SEARCH_MS,
        SearchProjectInput, SearchProjectOutput, SearchSessionsInput, SearchSessionsOutput,
        SessionHit,
    },
};
use monocode_core::search::{
    Target,
    project::{SearchOptions, search_project},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn query(text: &str) -> Result<String, ControlError> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_QUERY_CHARS {
        return Err(ControlError::invalid_input(format!(
            "query must be 1 to {MAX_QUERY_CHARS} characters"
        )));
    }
    Ok(text.to_owned())
}

/// A flag that turns on after the project search deadline, and the task
/// that turns it on; drop the task to stop waiting.
fn deadline(cx: &App) -> (Arc<AtomicBool>, gpui::Task<()>) {
    let flag = Arc::new(AtomicBool::new(false));
    let set = flag.clone();
    let timer = cx
        .background_executor()
        .timer(std::time::Duration::from_millis(PROJECT_SEARCH_MS));
    let clock = cx.background_executor().spawn(async move {
        timer.await;
        set.store(true, Ordering::Release);
    });
    (flag, clock)
}

impl Shell {
    pub(super) fn control_search_sessions(
        &self,
        input: &SearchSessionsInput,
        cx: &App,
    ) -> Result<Exec, ControlError> {
        let prepared = (|| {
            let query = query(&input.query)?;
            let limit = ops::limit(input.limit, DEFAULT_SESSION_HITS, MAX_SESSION_HITS)?;
            let project = input
                .project
                .as_deref()
                .map(|p| self.control_project(Some(p)))
                .transpose()?;
            Ok::<_, ControlError>((query, limit, project))
        })();
        let (query, limit, project) = prepared?;
        // Only sessions a client may see: project sessions, not Monos' chats.
        let visible: HashMap<i64, (String, String)> = self
            .session_summaries
            .keys()
            .filter(|id| self.control_session(**id, cx).is_ok())
            .map(|id| {
                let metadata = self.session_metadata(*id, cx);
                (
                    *id,
                    (metadata.summary.cwd.clone(), metadata.display_title()),
                )
            })
            .filter(|(_, (cwd, _))| project.as_ref().is_none_or(|p| p == cwd))
            .collect();
        let store = self.store.clone();
        let (cancel, clock) = deadline(cx);
        Ok(Exec::Later(cx.background_executor().spawn(async move {
            let found = locked(&store)
                .and_then(|db| db.search_reader())
                .and_then(|reader| match reader {
                    Some(reader) => reader.search_sessions(&query, cancel.clone()),
                    None => {
                        locked(&store).and_then(|db| db.search_sessions(&query, cancel.clone()))
                    }
                });
            drop(clock);
            let (mut hits, truncated) = found.map_err(ControlError::internal)?;
            hits.sort_by_key(|hit| std::cmp::Reverse(hit.score));
            let hits: Vec<SessionHit> = hits
                .into_iter()
                .filter_map(|hit| {
                    let (id, kind) = match hit.target {
                        Target::Conversation(id) => (id, HitKind::Title),
                        Target::Message { session, .. } => (session, HitKind::Message),
                        Target::File { .. } | Target::Project(_) => return None,
                    };
                    let (project, title) = visible.get(&id)?.clone();
                    Some(SessionHit {
                        session_id: id,
                        project,
                        snippet: hit.title.chars().take(MAX_PREVIEW_CHARS).collect(),
                        title,
                        kind,
                    })
                })
                .collect();
            output(&SearchSessionsOutput {
                truncated: truncated || hits.len() > limit,
                hits: hits.into_iter().take(limit).collect(),
            })
        })))
    }

    pub(super) fn control_search_project(
        &self,
        input: SearchProjectInput,
        cx: &App,
    ) -> Result<Exec, ControlError> {
        let prepared = (|| {
            let query = query(&input.query)?;
            let limit = ops::limit(input.limit, DEFAULT_FILE_MATCHES, MAX_FILE_MATCHES)?;
            let project = self.control_local_project(input.project.as_deref())?;
            Ok::<_, ControlError>((query, limit, project))
        })();
        let (query, limit, project) = prepared?;
        let options = SearchOptions {
            cwd: project.clone(),
            query,
            case_sensitive: input.case_sensitive.unwrap_or(false),
            whole_word: input.whole_word.unwrap_or(false),
            regex: input.regex.unwrap_or(false),
            include: input.include.filter(|g| !g.trim().is_empty()),
            exclude: input.exclude.filter(|g| !g.trim().is_empty()),
            search_id: String::new(),
        };
        let (cancel, clock) = deadline(cx);
        Ok(Exec::Later(cx.background_executor().spawn(async move {
            let found = search_project(std::path::Path::new(&project), &options, &cancel);
            drop(clock);
            let timed_out = cancel.load(Ordering::Acquire);
            let found = found.map_err(ControlError::invalid_input)?;
            output(&SearchProjectOutput {
                truncated: found.truncated || found.matches.len() > limit,
                matches: found
                    .matches
                    .into_iter()
                    .take(limit)
                    .map(|m| FileMatch {
                        path: m.relative,
                        line: m.line,
                        column: m.column,
                        preview: m
                            .preview
                            .trim_end()
                            .chars()
                            .take(MAX_PREVIEW_CHARS)
                            .collect(),
                    })
                    .collect(),
                project,
                timed_out,
            })
        })))
    }
}
