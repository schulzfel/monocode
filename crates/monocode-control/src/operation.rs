//! The contract every control operation implements, and its catalog entry.
use crate::{Scope, schema::object_schema};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::{fmt::Debug, time::Duration};

/// How long the app's confirmation prompt waits for the person before the
/// call fails with `user_declined`.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);

/// Behaviour hints, published as MCP tool annotations. Clients use them to
/// decide what to auto-approve, so they must be honest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hints {
    /// The operation changes nothing.
    pub read_only: bool,
    /// The operation may delete or discard something.
    pub destructive: bool,
    /// Repeating the call with the same input has no further effect.
    pub idempotent: bool,
}

impl Hints {
    /// A read that changes nothing.
    pub const READ: Self = Self {
        read_only: true,
        destructive: false,
        idempotent: true,
    };
    /// A change that repeating has no further effect on.
    pub const IDEMPOTENT: Self = Self {
        read_only: false,
        destructive: false,
        idempotent: true,
    };
    /// A change that does more each time it is repeated, such as sending a
    /// message. A `requestId` makes a retry safe.
    pub const CHANGE: Self = Self {
        read_only: false,
        destructive: false,
        idempotent: false,
    };
    /// A change that discards something, such as a queued message; doing it
    /// again has no further effect.
    pub const DISCARD: Self = Self {
        read_only: false,
        destructive: true,
        idempotent: true,
    };
}

/// One control operation: a typed input, a typed output, the scope it needs
/// and how it behaves. Implement it in a domain module under `ops`, then add
/// one line to the registry in `command.rs`.
pub trait Operation {
    /// The tool name: `snake_case` `noun_verb`, at most 64 characters of
    /// `[a-z0-9_]`. It is also the [`Command`](crate::Command) tag.
    const NAME: &'static str;
    /// What a person calls it, in sentence case ("Remove project"). Relays
    /// publish it as the tool's title, and transcripts show it in place of
    /// the tool name.
    const TITLE: &'static str;
    /// The input fields that say what a call acts on, in priority order: a
    /// transcript shows the first one the call sets after [`Self::TITLE`].
    /// [`SET_FIELDS`](crate::SET_FIELDS) alone shows the names of the fields
    /// the call sets instead.
    const SUBJECT: &'static [&'static str] = &[];
    /// The scope a client needs to call it.
    const SCOPE: Scope;
    /// Scopes the calls [`Self::needs_also`] picks need as well. Most
    /// operations need [`Self::SCOPE`] alone.
    const ALSO: &'static [Scope] = &[];
    /// What it does, for the model choosing a tool.
    const DESCRIPTION: &'static str;
    /// How it behaves.
    const HINTS: Hints;
    /// Whether the person confirms each call in the app before it runs: it
    /// deletes, discards or publishes something.
    const CONFIRM: bool = false;
    /// Whether a session may not call it on its own session
    /// ([`Self::target_session`]): stopping, waiting on, archiving, deleting
    /// or reconfiguring itself.
    const NOT_ON_SELF: bool = false;
    /// Whether its target session ([`Self::target_session`]) must be local:
    /// remote sessions are read-only to clients.
    const LOCAL_ONLY: bool = false;
    /// Whether only a Mono's chat or habit run may call it: it acts on the
    /// calling Mono's own memory, soul, habits, artifacts or chat. Every
    /// other caller is refused.
    const MONO_ONLY: bool = false;
    /// The arguments.
    type Input: Serialize + DeserializeOwned + JsonSchema + Debug + Clone + PartialEq + Send;
    /// The result.
    type Output: Serialize + DeserializeOwned + JsonSchema + Debug + Clone + PartialEq + Send;

    /// A one-line summary of a result, for clients that only read text.
    fn summary(output: &Self::Output) -> String;

    /// Whether this call needs [`Self::ALSO`] too, given its input.
    fn needs_also(input: &Self::Input) -> bool {
        let _ = input;
        false
    }

    /// The existing session this call acts on (its `sessionId`), if it
    /// names one. [`Self::NOT_ON_SELF`] and [`Self::LOCAL_ONLY`] apply to it.
    fn target_session(input: &Self::Input) -> Option<i64> {
        let _ = input;
        None
    }

    /// The input's `project`, when an absent one means the current project:
    /// the app fills it in for callers that have a project of their own, so
    /// an agent session's call acts on its session's project. `None` when
    /// the operation, or this input (an update by id), has no such default.
    fn project_slot(input: &mut Self::Input) -> Option<&mut Option<String>> {
        let _ = input;
        None
    }
}

/// The argument a client adds to a changing operation's input to make a
/// retry return the first result. It travels as the call's request ID, not
/// in the operation's input.
pub const REQUEST_ID: &str = "requestId";

/// An operation's catalog entry: everything a relay needs to publish it.
#[derive(Clone, Debug, PartialEq)]
pub struct Descriptor {
    /// The tool name.
    pub name: &'static str,
    /// What a person calls it ([`Operation::TITLE`]).
    pub title: &'static str,
    /// The input fields a caption shows ([`Operation::SUBJECT`]).
    pub subject: &'static [&'static str],
    /// The scope it needs.
    pub scope: Scope,
    /// Scopes some calls need as well, depending on their input; the
    /// description says when.
    pub also: &'static [Scope],
    /// What it does.
    pub description: &'static str,
    /// How it behaves.
    pub hints: Hints,
    /// Whether the person confirms each call in the app
    /// ([`Operation::CONFIRM`]).
    pub confirmed: bool,
    /// Whether only a Mono's chat or habit run may call it
    /// ([`Operation::MONO_ONLY`]); relays list it only to a Mono.
    pub mono_only: bool,
    /// The input's JSON Schema (root type `object`). An operation that
    /// changes something also accepts [`REQUEST_ID`].
    pub input_schema: Map<String, Value>,
    /// The output's JSON Schema.
    pub output_schema: Map<String, Value>,
}

impl Descriptor {
    /// The entry for `Op`.
    #[must_use]
    pub fn of<Op: Operation>() -> Self {
        let mut input_schema = object_schema::<Op::Input>();
        if !Op::HINTS.read_only
            && let Some(Value::Object(properties)) = input_schema.get_mut("properties")
        {
            properties.insert(
                REQUEST_ID.into(),
                serde_json::json!({
                    "type": "string",
                    "pattern": "^[A-Za-z0-9_-]{1,128}$",
                    "description": "Makes a retry return the first result instead of acting again. Reusing it with other input fails with request_conflict."
                }),
            );
        }
        Self {
            name: Op::NAME,
            title: Op::TITLE,
            subject: Op::SUBJECT,
            scope: Op::SCOPE,
            also: Op::ALSO,
            description: Op::DESCRIPTION,
            hints: Op::HINTS,
            confirmed: Op::CONFIRM,
            mono_only: Op::MONO_ONLY,
            input_schema,
            output_schema: object_schema::<Op::Output>(),
        }
    }
}

/// Whether `name` is a valid tool name: 1 to 64 characters of `[a-z0-9_]`,
/// starting with a letter.
#[must_use]
pub fn valid_tool_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
