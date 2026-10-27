//! `app help`, generated from the control catalog so it cannot drift from
//! the operations the app answers: each operation's name, its old dotted
//! name, title, first sentence and input fields, then each old name that
//! takes other fields with its own. A Mono's chat and habit runs get the
//! operations a Mono may call (`MONO_OPS`); everyone else gets those only a
//! Mono may not.
use super::legacy;
use crate::shell::MONO_OPS;
use monocode_control::{Descriptor, REQUEST_ID, catalog};
use serde_json::{Map, Value};
use std::fmt::Write;

const HEAD: &str = "MonoCode app access
Usage: monocode-gpui app ACTION --json '{...}' [--request-id ID]
Pass --input FILE or --input - to read a JSON object instead, or pass the object itself.
ACTION is an operation below or its old name in brackets; fields ending in ? are optional.
An old name indented under its operation takes the fields listed with it instead.
Operations:
";

const TAIL: &str = r#"Output: {"ok":true,"result":...,"requestId":ID}, or {"ok":false,"error":MESSAGE,"code":CODE,"retryable":BOOL,"requestId":ID}; the exit code is 1 when ok is false. A failure before the call is sent has no requestId.
Retry: every run has a new request ID unless --request-id ID names one, or requestId in the input of an operation that changes something (a read takes none). A call that changes something acts once per request ID: when its outcome is unknown, or retryable is true, run it again with the same --request-id and identical input. The same ID with other input fails with request_conflict.
Credentials are in the environment; never print them.
"#;

/// The help text: for a Mono's chat (`mono`) the operations a Mono may
/// call, else every operation but those only a Mono may call.
pub(super) fn text(mono: bool) -> String {
    let mut text = String::from(HEAD);
    let listed = |d: &&Descriptor| {
        if mono {
            MONO_OPS.contains(&d.name)
        } else {
            !d.mono_only
        }
    };
    for descriptor in catalog().iter().filter(listed) {
        let _ = writeln!(text, "{}", line(descriptor));
    }
    text.push_str(TAIL);
    text
}

/// `name [old.name] {field,optional?}: Title. First sentence.`, then
/// `  old.name {field,…}` for each old name that takes other fields than
/// the operation, as the old validator listed them (`legacy::fields`).
fn line(descriptor: &Descriptor) -> String {
    let aliases = legacy::aliases(descriptor.name);
    let alias = if aliases.is_empty() {
        String::new()
    } else {
        format!(" [{}]", aliases.join(", "))
    };
    let schema = &descriptor.input_schema;
    let fields = fields(schema, schema);
    let mut line = format!(
        "{}{alias} {{{}}}: {}. {}",
        descriptor.name,
        fields.join(","),
        descriptor.title,
        first_sentence(descriptor.description),
    );
    let mut names: Vec<&str> = fields
        .iter()
        .map(|field| field.split(['?', '{']).next().unwrap_or_default())
        .collect();
    names.sort_unstable();
    for alias in aliases {
        let old: Vec<&str> = legacy::fields(alias)
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        let mut sorted = old.clone();
        sorted.sort_unstable();
        if sorted != names {
            let _ = write!(line, "\n  {alias} {{{}}}", old.join(","));
        }
    }
    line
}

/// A field of an input: its name, whether it is required, and the object
/// it is when it has fields of its own.
type Field<'a> = (String, bool, Option<&'a Map<String, Value>>);

/// The fields of input `schema`, `?` marking the optional ones and a field
/// that is an object of its own followed by its fields, as
/// `schedule{kind,time?}`; `root` holds the definitions it refers to. An
/// input that is one of several shapes lists every shape's fields; a field
/// is required when every shape requires it. `requestId` is left to the
/// retry rules.
fn fields(schema: &Map<String, Value>, root: &Map<String, Value>) -> Vec<String> {
    let shapes: Vec<&Map<String, Value>> = match schema.get("oneOf").or(schema.get("anyOf")) {
        Some(Value::Array(variants)) => variants.iter().filter_map(Value::as_object).collect(),
        _ => vec![schema],
    };
    let mut listed: Vec<Field> = Vec::new();
    for shape in &shapes {
        let properties = shape.get("properties").and_then(Value::as_object);
        for (name, field) in properties.into_iter().flatten() {
            if name != REQUEST_ID && !listed.iter().any(|(known, ..)| known == name) {
                let required = shapes.iter().all(|s| required(s, name));
                listed.push((name.clone(), required, object(field, root)));
            }
        }
    }
    listed
        .into_iter()
        .map(|(name, required, object)| {
            let name = if required { name } else { format!("{name}?") };
            match object {
                Some(object) => format!("{name}{{{}}}", fields(object, root).join(",")),
                None => name,
            }
        })
        .collect()
}

/// The object with fields of its own that `field` is, directly or through
/// a definition in `root`, also when it may be null.
fn object<'a>(field: &'a Value, root: &'a Map<String, Value>) -> Option<&'a Map<String, Value>> {
    let resolved = |value: &'a Value| -> Option<&'a Map<String, Value>> {
        let value = match value.get("$ref").and_then(Value::as_str) {
            Some(path) => root.get("$defs")?.get(path.strip_prefix("#/$defs/")?)?,
            None => value,
        };
        value.as_object().filter(|o| o.contains_key("properties"))
    };
    resolved(field).or_else(|| {
        field
            .get("anyOf")
            .and_then(Value::as_array)?
            .iter()
            .find_map(resolved)
    })
}

fn required(shape: &Map<String, Value>, name: &str) -> bool {
    shape
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| required.iter().any(|r| r.as_str() == Some(name)))
}

/// A description's first sentence: help lists one line per operation.
fn first_sentence(description: &str) -> &str {
    description
        .find(". ")
        .map_or(description, |end| &description[..=end])
}
