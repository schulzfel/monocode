//! What a transcript shows for a call to one of the app's tools: the
//! operation's [`TITLE`](crate::Operation::TITLE) and what the call acts on,
//! shortened to fit one line. Display only; the call itself is unchanged.
use crate::{REQUEST_ID, command::caption_spec};
use serde_json::Value;

/// The relay's own tool, which answers without the app and so is not in the
/// [`catalog`](crate::catalog).
pub const APP_LAUNCH: &str = "app_launch";
/// [`APP_LAUNCH`]'s title.
pub const APP_LAUNCH_TITLE: &str = "Launch MonoCode";
/// As the only [`SUBJECT`](crate::Operation::SUBJECT) entry: show the names
/// of the fields a call sets, for operations that change several settings.
pub const SET_FIELDS: &str = "*";

/// The longest prose subject, in characters, before it is cut with "…".
const TEXT_CHARS: usize = 60;
/// How many items of a list a subject names before "+N".
const LIST_ITEMS: usize = 3;
/// Fields that hold a project or file path; a subject shows their last two
/// components.
const PATH_FIELDS: &[&str] = &["project", "path", "paths", "order", "worktreeCwd"];

/// A tool call's caption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caption {
    /// What a person calls the operation, such as "Remove project".
    pub title: &'static str,
    /// What the call acts on, such as "repost-sh/monorepo".
    pub subject: Option<String>,
}

/// The caption for a call of the app's tool `tool` with `input`, or `None`
/// when the app has no tool of that name.
#[must_use]
pub fn caption(tool: &str, input: &Value) -> Option<Caption> {
    if tool == APP_LAUNCH {
        return Some(Caption {
            title: APP_LAUNCH_TITLE,
            subject: None,
        });
    }
    let (title, fields) = caption_spec(tool)?;
    let subject = if fields == [SET_FIELDS] {
        set_fields(input)
    } else {
        fields
            .iter()
            .find_map(|field| subject(field, input.get(*field)?))
    };
    Some(Caption { title, subject })
}

fn subject(field: &str, value: &Value) -> Option<String> {
    let path = PATH_FIELDS.contains(&field);
    match value {
        Value::String(text) if path => Some(short_path(text)).filter(|s| !s.is_empty()),
        Value::String(text) => Some(short_text(text)).filter(|s| !s.is_empty()),
        Value::Number(number) if field == "id" || field.ends_with("Id") => {
            Some(format!("#{number}"))
        }
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(true) => Some(words(field)),
        Value::Array(items) => {
            let items: Vec<String> = items
                .iter()
                .filter_map(Value::as_str)
                .map(|item| {
                    if path {
                        short_path(item)
                    } else {
                        short_text(item)
                    }
                })
                .filter(|item| !item.is_empty())
                .collect();
            (!items.is_empty()).then(|| short_list(&items))
        }
        _ => None,
    }
}

fn set_fields(input: &Value) -> Option<String> {
    let names: Vec<String> = input
        .as_object()?
        .iter()
        .filter(|(name, value)| !value.is_null() && name.as_str() != REQUEST_ID)
        .map(|(name, _)| words(name))
        .collect();
    (!names.is_empty()).then(|| short_list(&names))
}

/// The last two components of a path: `/Users/me/code/acme/app` reads as
/// `acme/app`.
#[must_use]
pub fn short_path(path: &str) -> String {
    let parts: Vec<&str> = path
        .trim()
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    parts[parts.len().saturating_sub(2)..].join("/")
}

/// The first non-blank line of `text`, cut to 60 characters with "…".
#[must_use]
pub fn short_text(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if line.chars().count() <= TEXT_CHARS {
        return line.to_owned();
    }
    let cut: String = line.chars().take(TEXT_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Up to three items joined with commas, then how many more: "a, b, c +2".
#[must_use]
pub fn short_list(items: &[String]) -> String {
    let shown = items[..items.len().min(LIST_ITEMS)].join(", ");
    match items.len().saturating_sub(LIST_ITEMS) {
        0 => shown,
        more => format!("{shown} +{more}"),
    }
}

/// A tool or field name as words in sentence case: `create_issue`,
/// `createIssue` and `create-issue` read as "Create issue".
#[must_use]
pub fn humanize(name: &str) -> String {
    let lower = words(name);
    let mut chars = lower.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A name as lowercase words; an all-caps word such as `URL` keeps its case.
fn words(name: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for part in name.split(['_', '-', ' ', '.']).filter(|p| !p.is_empty()) {
        let mut word = String::new();
        let mut previous_lower = false;
        for c in part.chars() {
            if c.is_uppercase() && previous_lower && !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            previous_lower = c.is_lowercase() || c.is_ascii_digit();
            word.push(c);
        }
        words.push(word);
    }
    words
        .into_iter()
        .map(|word| {
            if word.len() > 1 && word.chars().all(|c| c.is_uppercase() || c.is_ascii_digit()) {
                word
            } else {
                word.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
