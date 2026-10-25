//! `app_launch`: the one tool the relay answers itself. It starts the app
//! that owns this relay when nothing answers on the endpoint and the stored
//! policy has external control on, then waits for the endpoint. It never
//! starts anything while control is off.
use crate::Options;
use monocode_control::{
    APP_LAUNCH, APP_LAUNCH_TITLE, ControlError, ErrorCode, transport::Connection,
};
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// The tool's name.
pub(crate) const NAME: &str = APP_LAUNCH;
/// How long to wait for a started app to listen.
pub const LAUNCH_WAIT: Duration = Duration::from_secs(30);
/// How often to look for the endpoint while waiting.
const POLL: Duration = Duration::from_millis(100);

/// Starts the app; an error says why it could not.
pub type Spawner = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;
/// Whether the stored policy has external control on, read read-only.
pub type ControlProbe = Arc<dyn Fn() -> bool + Send + Sync>;

/// How the relay starts its app.
#[derive(Clone)]
pub struct Launcher {
    spawn: Spawner,
    enabled: ControlProbe,
    wait: Duration,
}

impl Launcher {
    /// Starts the app with `spawn`, only while `enabled` says control is on.
    #[must_use]
    pub fn new(spawn: Spawner, enabled: ControlProbe) -> Self {
        Self {
            spawn,
            enabled,
            wait: LAUNCH_WAIT,
        }
    }
    /// Waits `wait` for the endpoint instead of [`LAUNCH_WAIT`].
    #[must_use]
    pub fn wait(mut self, wait: Duration) -> Self {
        self.wait = wait;
        self
    }
}

/// The tool, for `tools/list`.
pub(crate) fn tool() -> Tool {
    let input = json!({"type": "object", "properties": {}, "additionalProperties": false});
    let output = json!({
        "type": "object",
        "properties": {
            "launched": {"type": "boolean", "description": "Whether this call started MonoCode."},
            "alreadyRunning": {"type": "boolean", "description": "Whether MonoCode was already listening."},
            "waitedMs": {"type": "integer", "minimum": 0, "description": "How long the call waited for MonoCode to listen."}
        },
        "required": ["launched", "alreadyRunning", "waitedMs"],
        "additionalProperties": false
    });
    let object = |value: Value| match value {
        Value::Object(map) => Arc::new(map),
        _ => Arc::default(),
    };
    Tool::new(
        NAME,
        "Start MonoCode when it is not running, and wait up to 30 seconds until it can be controlled. Does nothing when it already runs. Refused with control_disabled while external control is off in MonoCode's Settings.",
        object(input),
    )
    .with_title(APP_LAUNCH_TITLE)
    .with_raw_output_schema(object(output))
    .with_annotations(
        ToolAnnotations::with_title(APP_LAUNCH_TITLE)
            .read_only(false)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    )
}

/// Runs `app_launch` on a blocking thread.
pub(crate) async fn run(options: Options) -> Result<Value, ControlError> {
    tokio::task::spawn_blocking(move || launch(&options))
        .await
        .map_err(|e| ControlError::internal(format!("The relay's worker stopped: {e}")))?
}

fn launch(options: &Options) -> Result<Value, ControlError> {
    let endpoint = options.endpoint.clone()?;
    let answers = || Connection::connect(&endpoint).is_ok();
    if answers() {
        return Ok(json!({"launched": false, "alreadyRunning": true, "waitedMs": 0}));
    }
    let Some(launcher) = &options.launcher else {
        return Err(ControlError::new(
            ErrorCode::AppNotRunning,
            "MonoCode is not running, and this relay cannot start it.",
        ));
    };
    if !(launcher.enabled)() {
        return Err(ControlError::control_disabled());
    }
    (launcher.spawn)()
        .map_err(|e| ControlError::internal(format!("Cannot start MonoCode: {e}")))?;
    let started = Instant::now();
    loop {
        if answers() {
            let waited = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            return Ok(json!({"launched": true, "alreadyRunning": false, "waitedMs": waited}));
        }
        if started.elapsed() >= launcher.wait {
            return Err(ControlError::new(
                ErrorCode::Timeout,
                format!(
                    "MonoCode was started but did not listen within {} ms",
                    launcher.wait.as_millis()
                ),
            ));
        }
        std::thread::sleep(POLL);
    }
}

/// A one-line summary of a launch, for clients that read only text.
pub(crate) fn summary(output: &Value) -> String {
    if output["alreadyRunning"] == true {
        "MonoCode is already running".into()
    } else {
        format!(
            "Started MonoCode; it listened after {} ms",
            output["waitedMs"]
        )
    }
}
