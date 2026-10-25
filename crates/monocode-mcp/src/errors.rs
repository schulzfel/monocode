//! Control results as MCP tool results: structured content plus a one-line
//! text summary, and errors as `isError` results with `{code, message,
//! retryable}`.
use monocode_control::{ControlError, ErrorCode, summarize};
use rmcp::ErrorData;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Value, json};

/// A successful call of `name`.
pub(crate) fn success(name: &str, output: Value) -> CallToolResult {
    let summary = summarize(name, &output).unwrap_or_else(|| format!("{name} finished"));
    structured(summary, output)
}

/// A successful result: `output` as structured content, `summary` as text.
pub(crate) fn structured(summary: String, output: Value) -> CallToolResult {
    let mut result = CallToolResult::success(vec![ContentBlock::text(summary)]);
    result.structured_content = Some(output);
    result
}

/// A failed call, visible to the model rather than a protocol error.
pub(crate) fn failure(error: &ControlError) -> CallToolResult {
    let mut result = CallToolResult::structured_error(json!({
        "code": error.code,
        "message": error.message,
        "retryable": error.retryable,
    }));
    result.content = vec![ContentBlock::text(error.to_string())];
    result
}

/// A failed resource read, as a protocol error carrying `{code, message,
/// retryable}`: resources have no `isError` result.
pub(crate) fn protocol(error: &ControlError) -> ErrorData {
    let data = Some(json!({
        "code": error.code,
        "message": error.message,
        "retryable": error.retryable,
    }));
    let message = error.to_string();
    match error.code {
        ErrorCode::NotFound => ErrorData::resource_not_found(message, data),
        ErrorCode::InvalidInput => ErrorData::invalid_params(message, data),
        _ => ErrorData::internal_error(message, data),
    }
}
