//! Tools generated from the control catalog: one per operation, with its
//! schemas and annotations, and no per-tool code.
use crate::Role;
use monocode_control::{Command, ControlError, Descriptor, REQUEST_ID, catalog, frame, hints_of};
use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use serde_json::{Value, json};
use std::sync::Arc;

/// The tools a relay for `role` lists, in catalog order: every operation,
/// less those only a Mono may call unless it speaks for one. Calls are not
/// limited to the list ([`command`]); the app decides.
pub(crate) fn tools(role: Role) -> Vec<Tool> {
    catalog()
        .into_iter()
        .filter(|d| role == Role::Mono || !d.mono_only)
        .map(tool)
        .collect()
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
        for (tool, descriptor) in tools(Role::Mono).into_iter().zip(catalog()) {
            let annotated = tool.annotations.and_then(|a| a.title);
            assert_eq!(tool.title.as_deref(), Some(descriptor.title));
            assert_eq!(annotated.as_deref(), Some(descriptor.title));
        }
        let titled = |name: &str| {
            tools(Role::Session)
                .into_iter()
                .find(|t| t.name == name)?
                .title
        };
        assert_eq!(titled("project_delete").as_deref(), Some("Remove project"));
        assert_eq!(titled("session_wait").as_deref(), Some("Wait for session"));
    }

    #[test]
    fn only_a_monos_relay_lists_the_tools_only_a_mono_may_call() {
        let names = |role| -> Vec<String> {
            tools(role)
                .into_iter()
                .map(|t| t.name.to_string())
                .collect()
        };
        let mono_only: Vec<_> = catalog()
            .into_iter()
            .filter(|d| d.mono_only)
            .map(|d| d.name.to_owned())
            .collect();
        assert_eq!(mono_only.len(), 16);
        let all: Vec<_> = catalog().into_iter().map(|d| d.name.to_owned()).collect();
        assert_eq!(names(Role::Mono), all);
        let session = names(Role::Session);
        assert_eq!(session.len(), all.len() - mono_only.len());
        assert!(session.iter().all(|name| !mono_only.contains(name)));
        // Listed or not, a call reaches the app, which decides.
        let call = command("mono_soul_read", None).unwrap();
        assert!(call.command.mono_only());
    }
}
