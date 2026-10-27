//! Idempotency receipts, as Operator keeps them: a call with a request ID
//! stores its result under the client and request ID with a fingerprint of
//! its input. A retry with the same input gets the stored result; the same ID
//! with other input is `request_conflict`. Each client keeps its newest
//! [`MAX_PER_CLIENT`] receipts; saving one prunes the oldest beyond that.
use super::*;
use monocode_control::{RECEIPT_PREFIX, frame::Outcome};
use monocode_core::mono::files::content_hash;

/// The most receipts kept per client; older ones are pruned.
pub(super) const MAX_PER_CLIENT: usize = 200;

/// The store key of a receipt.
pub(super) fn key(client: &str, request_id: &str) -> String {
    format!("{}{}", client_prefix(client), content_hash(request_id))
}

/// Where `client`'s receipts are kept.
fn client_prefix(client: &str) -> String {
    format!("{RECEIPT_PREFIX}{}:", content_hash(client))
}

/// The input's fingerprint: the operation and its arguments, as the wire
/// carries them.
pub(super) fn fingerprint(wire: &Value) -> String {
    content_hash(&wire.to_string())
}

/// A stored receipt: the input's fingerprint, the result, and when it was
/// kept (Unix milliseconds, strictly increasing per window).
pub(super) fn encode(wire: &Value, outcome: &Outcome, at: i64) -> String {
    serde_json::json!([fingerprint(wire), outcome, at]).to_string()
}

fn decode(raw: &str) -> Result<(String, Outcome), String> {
    let unreadable = |e: String| format!("Unreadable control receipt: {e}");
    let mut fields: Vec<Value> =
        serde_json::from_str(raw).map_err(|e| unreadable(e.to_string()))?;
    fields.truncate(2);
    serde_json::from_value(Value::Array(fields)).map_err(|e| unreadable(e.to_string()))
}

/// When a stored receipt was kept; receipts from before times were kept
/// count as oldest.
fn kept_at(raw: &str) -> i64 {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|fields| fields.get(2).and_then(Value::as_i64))
        .unwrap_or(0)
}

/// Keeps `receipt` for `client`, then forgets that client's oldest receipts
/// beyond [`MAX_PER_CLIENT`]; they are read only when there are more.
pub(super) fn save(
    store: &monocode_core::store::Store,
    client: &str,
    request_id: &str,
    receipt: &str,
) -> Result<(), String> {
    store.save_setting(&key(client, request_id), receipt)?;
    let prefix = client_prefix(client);
    if store.count_settings_under(&prefix)? <= MAX_PER_CLIENT {
        return Ok(());
    }
    let mut kept = store.settings_under(&prefix)?;
    kept.sort_by_cached_key(|(_, raw)| std::cmp::Reverse(kept_at(raw)));
    let old: Vec<String> = kept
        .split_off(MAX_PER_CLIENT)
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    store.delete_settings(&old)
}

fn conflict(message: &str) -> ControlError {
    ControlError::new(ErrorCode::RequestConflict, message)
}

impl Shell {
    /// Replays the stored result of `call`'s request ID, or runs it.
    pub(super) fn control_with_receipt(
        &mut self,
        call: Pending,
        grant: Grant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(request_id) = call.request_id.clone() else {
            return self.run_control(call, &grant, window, cx);
        };
        let running = (call.caller.clone(), request_id.clone());
        if !self.control.pending.insert(running.clone()) {
            let _ = call.reply.send(Err(conflict(
                "This request ID is still running; retry with the same request ID.",
            )
            .retryable(true)));
            return;
        }
        let store = self.store.clone();
        let stored = key(&call.caller.name(), &request_id);
        let load = cx.background_executor().spawn(async move {
            locked(&store)?
                .setting(&stored)?
                .map(|raw| decode(&raw))
                .transpose()
        });
        cx.spawn(async move |this, cx| {
            let loaded = load.await;
            let _ = this.update_in(cx, |this, window, cx| match loaded {
                // The grant may have gone while the receipt was read
                // (Operator authorized again here); a refusal keeps none.
                Ok(None) => match this.control_grant(&call.caller, &call.command, cx) {
                    Ok(grant) => this.run_control(call, &grant, window, cx),
                    Err(error) => {
                        this.control.pending.remove(&running);
                        this.refuse_control(call, error, cx);
                    }
                },
                Ok(Some((hash, outcome))) => {
                    this.control.pending.remove(&running);
                    let wire = serde_json::to_value(&call.command).unwrap_or_default();
                    let _ = call.reply.send(if hash == fingerprint(&wire) {
                        outcome.into()
                    } else {
                        Err(conflict(
                            "This request ID was already used with different input.",
                        ))
                    });
                }
                Err(error) => {
                    this.control.pending.remove(&running);
                    let _ = call.reply.send(Err(ControlError::internal(error)));
                }
            });
        })
        .detach();
    }

    /// Stores `call`'s receipt, then answers with `result`; a receipt that
    /// cannot be stored fails the call.
    pub(super) fn keep_receipt(
        &mut self,
        call: Pending,
        request_id: String,
        wire: Value,
        result: Result<Value, ControlError>,
        cx: &mut Context<Self>,
    ) {
        self.control.last_receipt = now_ms().max(self.control.last_receipt + 1);
        let receipt = encode(
            &wire,
            &Outcome::from(result.clone()),
            self.control.last_receipt,
        );
        let (store, client, id) = (self.store.clone(), call.caller.name(), request_id.clone());
        let saved = cx
            .background_executor()
            .spawn(async move { save(&*locked(&store)?, &client, &id, &receipt) });
        cx.spawn(async move |this, cx| {
            let saved = saved.await;
            let _ = this.update(cx, |this, _| {
                this.control
                    .pending
                    .remove(&(call.caller.clone(), request_id));
            });
            let _ = call.reply.send(
                saved
                    .map_err(|e| ControlError::internal(format!("Cannot keep the receipt: {e}")))
                    .and(result),
            );
        })
        .detach();
    }
}
