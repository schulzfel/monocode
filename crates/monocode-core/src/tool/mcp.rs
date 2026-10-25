//! Captions for MCP tool calls, read like the built-in tools: MonoCode's own
//! tools by their curated titles, other servers' tools by their names as
//! words with the server as the source, and Claude's deferred-tool loader by
//! the tools it loads. Display only; the recorded name and input stay as the
//! provider sent them.
use super::{ToolCall, ToolKind, ToolPresentation};
use crate::{app_mcp::CONTROL_SERVER, visual_bridge};
use monocode_control::caption::{humanize, short_list, short_path, short_text};
use serde_json::Value;

#[cfg(test)]
mod tests;

/// Claude's deferred-tool loader.
const TOOL_SEARCH: &str = "ToolSearch";
/// Fields another server's tool most often names its subject with.
const COMMON_FIELDS: &[&str] = &["query", "path", "url", "title", "name", "id"];

/// The caption for `call` when it is an MCP tool or `ToolSearch`. `input` is
/// the parsed input and `kind` the provider's semantic kind, if any.
pub(super) fn present(call: &ToolCall, input: &Value, kind: &str) -> Option<ToolPresentation> {
    // Restored desktop rows wrap the arguments in an envelope.
    let args = input
        .get("rawInput")
        .filter(|raw| raw.is_object())
        .unwrap_or(input);
    let (action, target, source) = if call.name == TOOL_SEARCH {
        let query = args["query"].as_str().unwrap_or("").trim();
        match query.strip_prefix("select:") {
            Some(names) => ("Load tools".into(), loaded(names), None),
            None => ("Find tools".into(), short_text(query), None),
        }
    } else {
        let (server, tool) = identity(&call.name, kind)?;
        named(server, tool, args)
    };
    Some(ToolPresentation {
        action,
        target,
        kind: ToolKind::Mcp { source },
        ..ToolPresentation::default()
    })
}

/// What Claude's approval prompt calls a tool: an MCP tool by its title
/// ("Remove project", "Create issue · Linear"), any other by its name.
pub(crate) fn title(name: &str) -> String {
    let Some((server, tool)) = identity(name, "") else {
        return name.into();
    };
    match named(server, tool, &Value::Null) {
        (action, _, Some(source)) => format!("{action} · {source}"),
        (action, _, None) => action,
    }
}

/// An MCP tool's server and tool name, in each provider's naming: Claude's
/// `mcp__<server>__<tool>` (server names may hold single underscores),
/// Codex's `<server>:<tool>`, and ACP titles `<server>/<tool>` or
/// `Tool: <server>/<tool>`. ACP agents title their own tools in prose with a
/// semantic kind, so the short forms only count when there is no kind.
fn identity<'a>(name: &'a str, kind: &str) -> Option<(&'a str, &'a str)> {
    let name = name.trim();
    let (server, tool) = if let Some(rest) = name.strip_prefix("mcp__") {
        rest.split_once("__")?
    } else if kind.is_empty() {
        let short = name.strip_prefix("Tool: ").unwrap_or(name);
        short.split_once(':').or_else(|| short.split_once('/'))?
    } else {
        return None;
    };
    (word(server) && word(tool)).then_some((server, tool))
}

fn word(part: &str) -> bool {
    !part.is_empty()
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Action, target and source for `server`'s `tool` called with `args`.
fn named(server: &str, tool: &str, args: &Value) -> (String, String, Option<String>) {
    match server {
        CONTROL_SERVER => monocode_control::caption(tool, args).map_or_else(
            || (humanize(tool), String::new(), None),
            |caption| {
                let subject = caption.subject.unwrap_or_default();
                (caption.title.into(), subject, None)
            },
        ),
        visual_bridge::SERVER => {
            let title = visual_bridge::tool_title(tool).map_or_else(|| humanize(tool), Into::into);
            let subject = ["title", "id"]
                .iter()
                .find_map(|field| args[*field].as_str())
                .map(short_text)
                .unwrap_or_default();
            (title, subject, None)
        }
        _ => {
            let subject = COMMON_FIELDS
                .iter()
                .find_map(|field| {
                    let value = args[*field].as_str().filter(|v| !v.trim().is_empty())?;
                    Some(if *field == "path" {
                        short_path(value)
                    } else {
                        short_text(value)
                    })
                })
                .unwrap_or_default();
            (humanize(tool), subject, Some(server_label(server)))
        }
    }
}

/// The tools a `select:` query loads, by their titles.
fn loaded(names: &str) -> String {
    let titles: Vec<String> = names
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            identity(name, "").map_or_else(
                || name.to_owned(),
                |(server, tool)| named(server, tool, &Value::Null).0,
            )
        })
        .collect();
    short_list(&titles)
}

/// A server name as a person writes it: `linear` is "Linear", Claude's
/// `claude_ai_Gmail` connector is "Gmail", `integrated-browser-mcp` is
/// "Integrated Browser".
fn server_label(server: &str) -> String {
    let name = server.strip_prefix("claude_ai_").unwrap_or(server);
    let mut words: Vec<&str> = name
        .split(['_', '-', ' '])
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() > 1 {
        words.retain(|w| !w.eq_ignore_ascii_case("mcp"));
    }
    words
        .iter()
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}
