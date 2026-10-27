//! Wire frames: newline-delimited JSON, one frame per line.
//!
//! A connection starts with the client's [`Hello`]; the app answers with a
//! [`Welcome`] carrying this client's [`Grant`], or with an error frame and
//! closes. After that the client sends [`Call`]s and the app answers each
//! with a [`Reply`] carrying the same `id`. When the welcome says the app
//! pushes changes, the client may also send [`Subscription`]s (answered with
//! a [`Reply`] of the same `id`), and the app then sends an [`Event`] each
//! time a followed [`Topic`] changes.
use crate::{Command, ControlError, Scope, SessionCredential, Topic};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::io::{self, BufRead, BufReader, Read, Write};

/// The largest frame a client may send.
pub const MAX_REQUEST_BYTES: usize = 1 << 20;
/// The largest frame the app may send.
pub const MAX_RESPONSE_BYTES: usize = 4 << 20;
/// The most connections of external clients the app serves at once.
pub const MAX_CONNECTIONS: usize = 16;
/// The most connections of the agent sessions the app runs itself (hellos
/// with a session credential) it serves at once, beside the external ones.
pub const MAX_APP_CONNECTIONS: usize = 256;
/// The most connections that may wait to say hello at once, before the
/// app knows which kind they are.
pub const MAX_PENDING_HELLOS: usize = 32;

/// Who is calling, as the client reports it. Self-reported, so it guards
/// against surprises, not impersonation. Fields it does not know, such as
/// the `executable` earlier relays sent, are ignored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    /// The client's name, shown in the approval prompt and in Settings.
    pub name: String,
    /// The client's version.
    pub version: String,
}

/// What a client may do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// The scopes granted.
    pub scopes: Vec<Scope>,
}

impl Grant {
    /// Whether `scope` is granted.
    #[must_use]
    pub fn allows(&self, scope: Scope) -> bool {
        self.scopes.contains(&scope)
    }
}

/// The client's first frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// The client's protocol major version.
    pub version: u32,
    /// Who is calling.
    pub client: ClientInfo,
    /// Set by the relay of an agent session the app runs: the session's
    /// credential. Without it the client is external.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionCredential>,
}

/// One operation call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Call {
    /// Echoed in the [`Reply`]; unique per connection.
    pub id: u64,
    /// Makes a retry return the first result instead of running again.
    /// `[A-Za-z0-9_-]{1,128}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// The operation and its input.
    pub command: Command,
}

/// Starts or stops following topics. Each topic needs the `read` scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    /// Echoed in the [`Reply`]; shares the numbering of calls.
    pub id: u64,
    /// The topics, at most [`MAX_TOPICS`](crate::MAX_TOPICS) per connection.
    pub topics: Vec<Topic>,
}

/// Frames a client sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientFrame {
    /// Opens the conversation.
    Hello(Hello),
    /// Calls an operation.
    Call(Call),
    /// Follows topics; the reply lists them.
    Subscribe(Subscription),
    /// Stops following topics.
    Unsubscribe(Subscription),
}

/// The app's answer to [`Hello`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    /// The app's protocol major version.
    pub version: u32,
    /// The app's version.
    pub app_version: String,
    /// What this client may do.
    pub grant: Grant,
    /// Whether the app takes subscriptions and pushes [`Event`]s. An app
    /// from before subscriptions leaves it out.
    #[serde(default)]
    pub push: bool,
}

/// The outcome of a call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The operation's output.
    Ok(Value),
    /// Why it failed.
    Error(ControlError),
}

impl From<Result<Value, ControlError>> for Outcome {
    fn from(result: Result<Value, ControlError>) -> Self {
        match result {
            Ok(value) => Self::Ok(value),
            Err(error) => Self::Error(error),
        }
    }
}

impl From<Outcome> for Result<Value, ControlError> {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Ok(value) => Ok(value),
            Outcome::Error(error) => Err(error),
        }
    }
}

/// The app's answer to a [`Call`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    /// The call's `id`.
    pub id: u64,
    /// What happened.
    pub result: Outcome,
}

/// A followed topic changed: read it again. Changes close together arrive
/// as one event, and `revision` only grows, so a later event with a higher
/// revision covers an earlier one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// What changed, as the client subscribed to it.
    pub topic: Topic,
    /// The app's change counter when it changed.
    pub revision: u64,
}

/// Frames the app sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerFrame {
    /// Accepts the client.
    Welcome(Welcome),
    /// Refuses the client or a malformed frame; the app closes after it.
    Error(ControlError),
    /// Answers a call.
    Reply(Reply),
    /// A followed topic changed.
    Event(Event),
}

/// Whether `id` is a valid request ID: 1 to 128 of `[A-Za-z0-9_-]`.
#[must_use]
pub fn valid_request_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A failure to read or write a frame.
#[derive(Debug)]
pub enum FrameError {
    /// The connection failed or closed mid-frame.
    Io(io::Error),
    /// The frame is over its size limit.
    TooLarge(usize),
    /// The frame is not valid JSON for its type.
    Malformed(String),
}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<FrameError> for ControlError {
    fn from(error: FrameError) -> Self {
        match error {
            FrameError::Io(error) if error.kind() == io::ErrorKind::TimedOut => {
                Self::new(crate::ErrorCode::Timeout, "MonoCode did not answer in time")
            }
            FrameError::Io(error) if error.kind() == io::ErrorKind::WouldBlock => {
                Self::new(crate::ErrorCode::Timeout, "MonoCode did not answer in time")
            }
            FrameError::Io(error) => {
                Self::internal(format!("The control connection failed: {error}")).retryable(true)
            }
            FrameError::TooLarge(limit) => {
                Self::invalid_input(format!("The frame is larger than {limit} bytes"))
            }
            FrameError::Malformed(error) => {
                Self::invalid_input(format!("The frame is malformed: {error}"))
            }
        }
    }
}

/// Reads one line of at most `limit` bytes, without its newline. `None` at a
/// clean end of stream.
///
/// # Errors
/// [`FrameError::TooLarge`] past `limit`; [`FrameError::Io`] on failure or a
/// stream that ends mid-line.
pub fn read_line<R: BufRead>(reader: &mut R, limit: usize) -> Result<Option<Vec<u8>>, FrameError> {
    let mut line = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let read = Read::take(&mut *reader, cap).read_until(b'\n', &mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
    } else if line.len() > limit {
        return Err(FrameError::TooLarge(limit));
    } else {
        return Err(FrameError::Io(io::ErrorKind::UnexpectedEof.into()));
    }
    if line.len() > limit {
        return Err(FrameError::TooLarge(limit));
    }
    Ok(Some(line))
}

/// Reads and parses one frame. `None` at a clean end of stream.
///
/// # Errors
/// As [`read_line`], or [`FrameError::Malformed`] for invalid JSON.
pub fn read_frame<R: BufRead, T: DeserializeOwned>(
    reader: &mut R,
    limit: usize,
) -> Result<Option<T>, FrameError> {
    read_line(reader, limit)?
        .map(|line| serde_json::from_slice(&line).map_err(|e| FrameError::Malformed(e.to_string())))
        .transpose()
}

/// Writes one frame and its newline, refusing frames over `limit`.
///
/// # Errors
/// [`FrameError::TooLarge`] past `limit`; [`FrameError::Io`] on failure.
pub fn write_frame<W: Write, T: Serialize>(
    writer: &mut W,
    frame: &T,
    limit: usize,
) -> Result<(), FrameError> {
    let mut line = serde_json::to_vec(frame).map_err(|e| FrameError::Malformed(e.to_string()))?;
    if line.len() > limit {
        return Err(FrameError::TooLarge(limit));
    }
    line.push(b'\n');
    writer.write_all(&line)?;
    writer.flush()?;
    Ok(())
}

/// Parses a client frame. On failure, returns the call's or subscription's
/// `id` when it can still be read, so the app can answer it instead of
/// hanging up.
///
/// # Errors
/// The `id`, if readable, and an `invalid_input` error.
pub fn decode_client(line: &[u8]) -> Result<ClientFrame, (Option<u64>, ControlError)> {
    serde_json::from_slice(line).map_err(|error| {
        let id = serde_json::from_slice::<Value>(line)
            .ok()
            .filter(|frame| {
                ["call", "subscribe", "unsubscribe"]
                    .contains(&frame["type"].as_str().unwrap_or_default())
            })
            .and_then(|frame| frame["id"].as_u64());
        (
            id,
            ControlError::invalid_input(format!("The call is malformed: {error}")),
        )
    })
}

/// Reads lines from a stream and keeps a partly read line across read
/// timeouts, so a reader that waits for pushed events can wake up, look
/// around, and read on without losing a frame.
pub struct Lines<R> {
    inner: BufReader<R>,
    partial: Vec<u8>,
}

impl<R: Read> Lines<R> {
    /// Lines of `inner`.
    pub fn new(inner: R) -> Self {
        Self {
            inner: BufReader::new(inner),
            partial: Vec::new(),
        }
    }
    /// The next line of at most `limit` bytes, without its newline. `None`
    /// at a clean end of stream. After a read timeout, call again: what was
    /// read so far is kept.
    ///
    /// # Errors
    /// [`FrameError::TooLarge`] past `limit`; [`FrameError::Io`] on failure,
    /// a timeout, or a stream that ends mid-line.
    pub fn next_line(&mut self, limit: usize) -> Result<Option<Vec<u8>>, FrameError> {
        loop {
            let available = match self.inner.fill_buf() {
                Ok(available) => available,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(FrameError::Io(error)),
            };
            if available.is_empty() {
                if self.partial.is_empty() {
                    return Ok(None);
                }
                self.partial.clear();
                return Err(FrameError::Io(io::ErrorKind::UnexpectedEof.into()));
            }
            let (taken, end) = match available.iter().position(|b| *b == b'\n') {
                Some(at) => (at + 1, Some(at)),
                None => (available.len(), None),
            };
            self.partial
                .extend_from_slice(&available[..end.unwrap_or(taken)]);
            self.inner.consume(taken);
            if end.is_none() {
                // Room for a trailing `\r` before the newline arrives.
                if self.partial.len() > limit.saturating_add(1) {
                    self.partial.clear();
                    return Err(FrameError::TooLarge(limit));
                }
                continue;
            }
            let mut line = std::mem::take(&mut self.partial);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.len() > limit {
                return Err(FrameError::TooLarge(limit));
            }
            return Ok(Some(line));
        }
    }
    /// Reads and parses the next frame, as [`Self::next_line`].
    ///
    /// # Errors
    /// As [`Self::next_line`], or [`FrameError::Malformed`] for invalid JSON.
    pub fn next_frame<T: DeserializeOwned>(
        &mut self,
        limit: usize,
    ) -> Result<Option<T>, FrameError> {
        self.next_line(limit)?
            .map(|line| {
                serde_json::from_slice(&line).map_err(|e| FrameError::Malformed(e.to_string()))
            })
            .transpose()
    }
}

/// Whether `error` is a read timeout rather than a broken connection.
#[must_use]
pub fn timed_out(error: &FrameError) -> bool {
    matches!(error, FrameError::Io(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock))
}
