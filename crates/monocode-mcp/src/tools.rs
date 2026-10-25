//! Tools generated from the control catalog: one per operation, with its
//! schemas and annotations, and no per-tool code.
use monocode_control::{Command, ControlError, Descriptor, REQUEST_ID, catalog, frame, hints_of};
use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use serde_json::{Value, json};
use std::sync::Arc;

/// Every tool, in catalog order.
pub(crate) fn tools() -> Vec<Tool> {
    catalog().into_iter().map(tool).collect()
}

fn tool(descriptor: Descriptor) -> Tool {
    let hints = descriptor.hints;
    Tool::new(
        descriptor.name,
        descriptor.description,
        Arc::new(descriptor.input_schema),
    )
    .with_title(descriptor.title)
    .with_raw_output_schema(Arc::new(descriptor.output_schema))
    .with_annotations(
        ToolAnnotations::with_title(descriptor.title)
            .read_only(hints.read_only)
            .destructive(hints.destructive)
            .idempotent(hints.idempotent)
            .open_world(false),
    )
}

/// A tool call as a control call: the command, and the client's
/// `requestId` for a tool that changes something.
#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) command: Command,
    pub(crate) request_id: Option<String>,
}

/// The call a tool call stands for. A changing tool's `requestId` argument
/// is passed through as the call's request ID; without one the call acts
/// every time.
///
/// # Errors
/// `invalid_input` for an unknown tool, arguments that do not fit it, or a
/// malformed `requestId`.
pub(crate) fn command(name: &str, arguments: Option<JsonObject>) -> Result<Request, ControlError> {
    let Some(hints) = hints_of(name) else {
        return Err(ControlError::invalid_input(format!(
            "MonoCode has no tool called {name}"
        )));
    };
    let mut arguments = arguments.unwrap_or_default();
    let request_id = if hints.read_only {
        None
    } else {
        match arguments.remove(REQUEST_ID) {
            None => None,
            Some(Value::String(id)) if frame::valid_request_id(&id) => Some(id),
            Some(_) => {
                return Err(ControlError::invalid_input(
                    "requestId must be 1 to 128 letters, digits, underscores or hyphens",
                ));
            }
        }
    };
    let input = Value::Object(arguments);
    let command = serde_json::from_value(json!({ "operation": name, "input": input }))
        .map_err(|e| ControlError::invalid_input(format!("Invalid arguments for {name}: {e}")))?;
    Ok(Request {
        command,
        request_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_titled_like_a_person_says_them() {
        for (tool, descriptor) in tools().into_iter().zip(catalog()) {
            let annotated = tool.annotations.and_then(|a| a.title);
            assert_eq!(tool.title.as_deref(), Some(descriptor.title));
            assert_eq!(annotated.as_deref(), Some(descriptor.title));
        }
        let titled = |name: &str| tools().into_iter().find(|t| t.name == name)?.title;
        assert_eq!(titled("project_delete").as_deref(), Some("Remove project"));
        assert_eq!(titled("session_wait").as_deref(), Some("Wait for session"));
    }
}
