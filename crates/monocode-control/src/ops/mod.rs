//! Inputs and outputs, one module per domain. Field names are `camelCase` on
//! the wire, as MCP tools conventionally use.
pub mod app;
pub mod automation;
pub mod changes;
pub mod configure;
pub mod dispatch;
pub mod layout;
pub mod lifecycle;
pub mod model;
pub mod mono;
pub mod mono_artifact;
pub mod mono_card;
pub mod mono_habit;
pub mod mono_memory;
pub mod mono_soul;
pub mod notes;
pub mod organize;
pub mod project;
pub mod question;
pub mod queue;
pub mod rail;
pub mod search;
pub mod session;
pub mod settings;
pub mod tab;
pub mod vcs;
pub mod workspace;
pub mod worktree;

use crate::ControlError;

/// Reads an optional `limit` within `1..=max`, defaulting to `default`.
///
/// # Errors
/// `invalid_input` when the limit is zero or above `max`.
pub fn limit(limit: Option<u32>, default: u32, max: u32) -> Result<usize, ControlError> {
    let value = limit.unwrap_or(default);
    if value == 0 || value > max {
        return Err(ControlError::invalid_input(format!(
            "limit must be between 1 and {max}"
        )));
    }
    Ok(value as usize)
}

/// One page of `items` from an opaque `cursor`, and the cursor for the page
/// after it, if any.
///
/// # Errors
/// `invalid_input` when the cursor was not issued by a previous page.
pub fn page<T: Clone>(
    items: &[T],
    cursor: Option<&str>,
    limit: usize,
) -> Result<(Vec<T>, Option<String>), ControlError> {
    let start = match cursor {
        None => 0,
        Some(cursor) => cursor
            .parse::<usize>()
            .ok()
            .filter(|start| *start <= items.len())
            .ok_or_else(|| ControlError::invalid_input("cursor is not valid for this list"))?,
    };
    let end = start.saturating_add(limit).min(items.len());
    let next = (end < items.len()).then(|| end.to_string());
    Ok((items[start..end].to_vec(), next))
}
