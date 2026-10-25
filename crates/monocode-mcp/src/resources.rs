//! Read-only MCP resources, each a view of one read operation. One table
//! maps URIs to operations and to the control topic that follows their
//! changes; reading a resource calls its operation with the URI's parameters
//! and returns the output as JSON text.
use monocode_control::{ControlError, ErrorCode, Topic};
use rmcp::model::{Resource, ResourceContents, ResourceTemplate};
use serde_json::{Map, Value, json};

/// The resources' MIME type.
pub(crate) const MIME: &str = "application/json";

/// How a URI parameter becomes an operation argument.
#[derive(Clone, Copy)]
enum Param {
    /// Percent-decoded text, under this argument name.
    Text(&'static str),
    /// An integer, under this argument name.
    Integer(&'static str),
}

/// Which control topic follows a resource's changes.
#[derive(Clone, Copy)]
enum Follow {
    /// Not followed: read it again when needed.
    Never,
    Workspace,
    Projects,
    /// By the `project` argument.
    ProjectSessions,
    /// By the `sessionId` argument.
    Session,
    /// By the `sessionId` argument.
    Transcript,
}

/// One resource or template and the operation behind it.
pub(crate) struct Mapping {
    /// The URI, with `{name}` placeholders for one path segment each.
    pub(crate) uri: &'static str,
    name: &'static str,
    title: &'static str,
    description: &'static str,
    /// The read operation a read calls.
    pub(crate) operation: &'static str,
    /// Arguments that are always passed.
    fixed: &'static [(&'static str, i64)],
    /// The placeholders, in order.
    params: &'static [Param],
    /// How long a client may treat a read as fresh, in milliseconds.
    pub(crate) ttl_ms: u64,
    follow: Follow,
}

/// Every resource and template.
pub(crate) const MAPPINGS: &[Mapping] = &[
    Mapping {
        uri: "monocode://app",
        name: "app",
        title: "MonoCode",
        description: "The running app: version, data folder and the scopes granted to this client.",
        operation: "app_info",
        fixed: &[],
        params: &[],
        ttl_ms: 60_000,
        follow: Follow::Never,
    },
    Mapping {
        uri: "monocode://workspace",
        name: "workspace",
        title: "Workspace",
        description: "What the window shows: current project, title tabs, the active tab's panes, review tabs, panels and layout.",
        operation: "workspace_read",
        fixed: &[],
        params: &[],
        ttl_ms: 1_000,
        follow: Follow::Workspace,
    },
    Mapping {
        uri: "monocode://projects",
        name: "projects",
        title: "Projects",
        description: "The projects in rail order, then archived ones (the first 200).",
        operation: "project_list",
        fixed: &[("limit", 200)],
        params: &[],
        ttl_ms: 5_000,
        follow: Follow::Projects,
    },
    Mapping {
        uri: "monocode://projects/{project}/sessions",
        name: "project-sessions",
        title: "Project sessions",
        description: "A local project's sessions, most recently active first (the first 200). {project} is the project's path, percent-encoded.",
        operation: "session_list",
        fixed: &[("limit", 200)],
        params: &[Param::Text("project")],
        ttl_ms: 2_000,
        follow: Follow::ProjectSessions,
    },
    Mapping {
        uri: "monocode://sessions/{id}",
        name: "session",
        title: "Session status",
        description: "What a session is doing: status, pending approvals and questions, queued messages, context use, model and access mode.",
        operation: "session_status",
        fixed: &[],
        params: &[Param::Integer("sessionId")],
        ttl_ms: 1_000,
        follow: Follow::Session,
    },
    Mapping {
        uri: "monocode://sessions/{id}/transcript",
        name: "session-transcript",
        title: "Session transcript",
        description: "A session's latest exchanges, newest last: each of the person's messages with the agent's last reply (the last 3, as session_read returns them).",
        operation: "session_read",
        fixed: &[],
        params: &[Param::Integer("sessionId")],
        ttl_ms: 1_000,
        follow: Follow::Transcript,
    },
];

/// The resources without parameters, for `resources/list`.
pub(crate) fn resources() -> Vec<Resource> {
    MAPPINGS
        .iter()
        .filter(|m| m.params.is_empty())
        .map(|m| {
            Resource::new(m.uri, m.name)
                .with_title(m.title)
                .with_description(m.description)
                .with_mime_type(MIME)
        })
        .collect()
}

/// The templates, for `resources/templates/list`.
pub(crate) fn templates() -> Vec<ResourceTemplate> {
    MAPPINGS
        .iter()
        .filter(|m| !m.params.is_empty())
        .map(|m| {
            ResourceTemplate::new(m.uri, m.name)
                .with_title(m.title)
                .with_description(m.description)
                .with_mime_type(MIME)
        })
        .collect()
}

/// The mapping `uri` matches and the operation's arguments.
///
/// # Errors
/// `not_found` for a URI no resource has, `invalid_input` for a bad parameter.
pub(crate) fn resolve(uri: &str) -> Result<(&'static Mapping, Map<String, Value>), ControlError> {
    let unknown = || {
        ControlError::new(
            ErrorCode::NotFound,
            format!("MonoCode has no resource {uri}"),
        )
    };
    for mapping in MAPPINGS {
        let Some(values) = captures(mapping.uri, uri) else {
            continue;
        };
        let mut arguments: Map<String, Value> = mapping
            .fixed
            .iter()
            .map(|(k, v)| ((*k).into(), json!(v)))
            .collect();
        for (param, value) in mapping.params.iter().zip(values) {
            match *param {
                Param::Text(name) => {
                    arguments.insert(name.into(), json!(decode(value)?));
                }
                Param::Integer(name) => {
                    let number: i64 = value.parse().map_err(|_| unknown())?;
                    arguments.insert(name.into(), json!(number));
                }
            }
        }
        return Ok((mapping, arguments));
    }
    Err(unknown())
}

/// The control topic that follows `uri`'s changes.
///
/// # Errors
/// As [`resolve`], or `invalid_input` for a resource that is not followed.
pub(crate) fn topic(uri: &str) -> Result<Topic, ControlError> {
    let (mapping, arguments) = resolve(uri)?;
    let text = |name: &str| {
        arguments
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let id = |name: &str| arguments.get(name).and_then(Value::as_i64);
    let topic = match mapping.follow {
        Follow::Never => None,
        Follow::Workspace => Some(Topic::Workspace),
        Follow::Projects => Some(Topic::Projects),
        Follow::ProjectSessions => {
            text("project").map(|project| Topic::ProjectSessions { project })
        }
        Follow::Session => id("sessionId").map(|id| Topic::Session { id }),
        Follow::Transcript => id("sessionId").map(|id| Topic::SessionTranscript { id }),
    };
    topic.ok_or_else(|| {
        ControlError::invalid_input(format!(
            "{uri} cannot be subscribed to; read it again when needed"
        ))
    })
}

/// The placeholder values of `uri` against `pattern`, one segment each.
fn captures<'a>(pattern: &str, uri: &'a str) -> Option<Vec<&'a str>> {
    let pattern: Vec<_> = pattern.split('/').collect();
    let uri: Vec<_> = uri.split('/').collect();
    if pattern.len() != uri.len() {
        return None;
    }
    let mut values = Vec::new();
    for (want, got) in pattern.iter().zip(&uri) {
        if want.starts_with('{') && want.ends_with('}') {
            if got.is_empty() {
                return None;
            }
            values.push(*got);
        } else if want != got {
            return None;
        }
    }
    Some(values)
}

/// Percent-decodes one URI segment.
fn decode(segment: &str) -> Result<String, ControlError> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = segment
                .get(at + 1..at + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                .ok_or_else(|| {
                    ControlError::invalid_input(format!(
                        "{segment} is not percent-encoded correctly"
                    ))
                })?;
            out.push(hex);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out)
        .map_err(|_| ControlError::invalid_input(format!("{segment} is not UTF-8")))
}

/// A read's contents: the operation's output as JSON text.
pub(crate) fn contents(uri: &str, output: &Value) -> ResourceContents {
    ResourceContents::text(output.to_string(), uri).with_mime_type(MIME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_control::{Scope, catalog};

    #[test]
    fn every_resource_reads_through_a_read_scope_operation() {
        let catalog = catalog();
        for mapping in MAPPINGS {
            let descriptor = catalog
                .iter()
                .find(|d| d.name == mapping.operation)
                .unwrap();
            assert_eq!(descriptor.scope, Scope::Read, "{}", mapping.uri);
            assert!(descriptor.hints.read_only, "{}", mapping.uri);
        }
        let (mapping, arguments) = resolve("monocode://projects/%2Fa%20b/sessions").unwrap();
        assert_eq!(mapping.operation, "session_list");
        assert_eq!(arguments["project"], "/a b");
        let code = |uri| resolve(uri).err().map(|e| e.code);
        assert_eq!(code("monocode://sessions/x"), Some(ErrorCode::NotFound));
        assert_eq!(
            code("monocode://projects/%zz/sessions"),
            Some(ErrorCode::InvalidInput)
        );
    }

    #[test]
    fn every_followed_resource_has_the_topic_of_its_read_operation() {
        let followed = [
            ("monocode://workspace", Topic::Workspace),
            ("monocode://projects", Topic::Projects),
            (
                "monocode://projects/%2Fa%20b/sessions",
                Topic::ProjectSessions {
                    project: "/a b".into(),
                },
            ),
            ("monocode://sessions/7", Topic::Session { id: 7 }),
            (
                "monocode://sessions/7/transcript",
                Topic::SessionTranscript { id: 7 },
            ),
        ];
        for (uri, topic) in followed {
            let (mapping, _) = resolve(uri).unwrap();
            assert_eq!(topic.operation(), mapping.operation, "{uri}");
            assert_eq!(super::topic(uri).unwrap(), topic, "{uri}");
        }
        let app = super::topic("monocode://app").unwrap_err();
        assert_eq!(app.code, ErrorCode::InvalidInput);
        assert_eq!(
            super::topic("monocode://nothing").unwrap_err().code,
            ErrorCode::NotFound
        );
    }
}
