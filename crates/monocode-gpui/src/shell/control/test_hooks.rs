//! How tests outside the control module reach it: a hello or a call from
//! an agent session's credential, handed to the shell as the session's
//! relay connection hands it, and what the activity log shows.
use super::*;
use monocode_control::{ClientInfo, Command, SessionCredential};
use std::sync::mpsc::{Receiver, sync_channel};

impl Shell {
    /// Says hello as session `id`'s relay does, with this run's credential.
    pub(crate) fn control_hello_as(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Receiver<Result<Grant, ControlError>> {
        let (reply, grant) = sync_channel(1);
        let hello = Request::Hello {
            client: ClientInfo {
                name: "Codex".into(),
                version: "1".into(),
            },
            session: Some(SessionCredential {
                id,
                token: monocode_core::app_mcp::token(id),
            }),
            reply,
        };
        self.handle_control(hello, window, cx);
        grant
    }

    /// Calls `command` as session `id`'s relay connection does: as the
    /// session its credential is for.
    pub(crate) fn control_call_as(
        &mut self,
        id: i64,
        request_id: Option<&str>,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Receiver<Result<Value, ControlError>> {
        let (reply, result) = sync_channel(1);
        let call = Request::Call {
            caller: CallerKind::Session(id),
            request_id: request_id.map(str::to_owned),
            command: Box::new(command),
            reply,
        };
        self.handle_control(call, window, cx);
        result
    }

    /// Follows `topics` as session `id`'s relay connection does, and returns
    /// the app's answer.
    pub(crate) fn control_follow_as(
        &mut self,
        id: i64,
        topics: Vec<monocode_control::Topic>,
        cx: &mut Context<Self>,
    ) -> Result<Value, ControlError> {
        let (reply, answer) = sync_channel(1);
        let (events, _queued) = sync_channel(hub::QUEUE);
        self.control_subscribe(
            u64::try_from(id).unwrap_or_default(),
            CallerKind::Session(id),
            topics,
            events,
            &reply,
            cx,
        );
        answer
            .try_recv()
            .expect("a subscription is answered at once")
    }

    /// Makes session `id` one of `lead`'s orchestration workers.
    pub(crate) fn control_make_worker(&mut self, lead: i64, id: i64) {
        self.orchestration_summaries.insert(
            lead,
            monocode_core::orchestration::Summary {
                tasks: vec![monocode_core::orchestration::SummaryTask {
                    session: Some(id),
                    title: "Worker".into(),
                    ..Default::default()
                }],
                live: true,
            },
        );
    }

    /// The activity log's entries, oldest first: who called what, and how
    /// it ended.
    pub(crate) fn control_activity(&self) -> Vec<(String, String, String)> {
        self.control
            .activity
            .entries()
            .map(|e| (e.client.clone(), e.operation.clone(), e.outcome.clone()))
            .collect()
    }
}
