//! One connection's calls: registering them, handing the reading turn
//! between waiting callers, and reading frames for everyone.
use super::{Calls, Delivery, Ended, Notice, Session, lock};
use crate::{
    ControlError,
    frame::{
        self, ClientFrame, FrameError, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, Outcome, ServerFrame,
    },
};
use std::{sync::mpsc::Sender, time::Instant};

impl Session {
    /// Registers call `id`. Returns whether it reads the connection, or how
    /// the connection already ended.
    pub(super) fn register(&self, id: u64, deliver: Sender<Delivery>) -> Result<bool, Ended> {
        let mut calls = lock(&self.calls);
        if let Some(ended) = &calls.ended {
            return Err(ended.clone());
        }
        calls.waiting.insert(id, deliver);
        Ok(!std::mem::replace(&mut calls.reading, true))
    }
    /// Forgets call `id`; when it was reading, another waiting call reads.
    pub(super) fn leave(&self, id: u64, reading: bool) {
        let mut calls = lock(&self.calls);
        calls.waiting.remove(&id);
        if reading {
            Self::hand_over(&mut calls);
        }
    }
    fn hand_over(calls: &mut Calls) {
        calls.reading = false;
        let next: Vec<u64> = calls.waiting.keys().copied().collect();
        for id in next {
            let Some(deliver) = calls.waiting.get(&id) else {
                continue;
            };
            if deliver.send(Delivery::Lead).is_ok() {
                calls.reading = true;
                return;
            }
            calls.waiting.remove(&id);
        }
    }
    pub(super) fn end(&self, ended: &Ended) {
        let mut calls = lock(&self.calls);
        calls.ended = Some(ended.clone());
        calls.reading = false;
        for (_, deliver) in calls.waiting.drain() {
            let _ = deliver.send(Delivery::Ended(ended.clone()));
        }
    }
    pub(super) fn ended(&self) -> bool {
        lock(&self.calls).ended.is_some()
    }
    pub(super) fn write(&self, frame: &ClientFrame) -> Result<(), FrameError> {
        let _writing = lock(&self.writing);
        frame::write_frame(&mut &*self.connection, frame, MAX_REQUEST_BYTES)
    }
    /// Reads frames for everyone until call `id`'s reply arrives, the
    /// connection ends, or `deadline` passes.
    pub(super) fn read_for(&self, id: u64, deadline: Instant) -> Option<Result<Outcome, Ended>> {
        let mut reader = lock(&self.reader);
        loop {
            let read = reader.next_frame::<ServerFrame>(MAX_RESPONSE_BYTES);
            let ended = match read {
                Ok(Some(ServerFrame::Reply(reply))) if reply.id == id => {
                    drop(reader);
                    self.leave(id, true);
                    return Some(Ok(reply.result));
                }
                Ok(Some(ServerFrame::Reply(reply))) => {
                    // A call that gave up is no longer listening.
                    if let Some(deliver) = lock(&self.calls).waiting.remove(&reply.id) {
                        let _ = deliver.send(Delivery::Reply(reply.result));
                    }
                    None
                }
                Ok(Some(ServerFrame::Event(event))) => {
                    self.notice(Notice::Changed(event));
                    None
                }
                Ok(Some(ServerFrame::Error(error))) => Some(Ended::Failed(error)),
                Ok(Some(ServerFrame::Welcome(_))) => Some(Ended::Failed(ControlError::internal(
                    "MonoCode sent an unexpected welcome",
                ))),
                Ok(None) => Some(Ended::Closed),
                Err(error) => Some(Ended::Failed(error.into())),
            };
            if let Some(ended) = ended {
                self.end(&ended);
                return Some(Err(ended));
            }
            if Instant::now() >= deadline {
                drop(reader);
                self.leave(id, true);
                return None;
            }
        }
    }
}
