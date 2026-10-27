//! `monocode-gpui mcp`: MonoCode's MCP server on standard input and output,
//! relaying to the window that opened the same data folder. MonoCode's own
//! agent sessions use it; other clients usually connect to the app's HTTP
//! server instead, and this stays as the advanced stdio path.
use crate::data_location::DataLocation;
use monocode_control::{ControlError, Endpoint, POLICY_KEY, Policy};
use std::{path::Path, sync::Arc};

mod launch;
pub(crate) mod session;
#[cfg(test)]
mod tests;

const HELP: &str = "monocode-gpui mcp [--client NAME] [--dev-data | --monocode-data | --monocode-db PATH]\nServes MonoCode's MCP tools over standard input and output for an MCP client, relaying to the running app that uses the same data.\n--client NAME names this client in MonoCode's approval prompt instead of the MCP client's own name.\nThe data flags choose the data folder as for the app itself.\nInside MonoCode's own agent sessions, MonoCode sets MONOCODE_CONTROL_ENDPOINT, MONOCODE_CONTROL_SESSION and MONOCODE_CONTROL_TOKEN, and the relay speaks for that session; MONOCODE_CONTROL_ROLE=mono, set for a Mono's chat and habit runs, also lists the Mono's own tools.";

/// Runs the relay until the MCP client disconnects; returns the exit code.
pub(crate) fn run(args: &[String]) -> i32 {
    let mut location = DataLocation::new();
    let mut client = None;
    // The data flags as given, for `app_launch` to start the app with.
    let mut data_args = Vec::new();
    let mut rest = args.iter().cloned();
    while let Some(arg) = rest.next() {
        let mut taken = None;
        let mut values = rest.by_ref().inspect(|value| taken = Some(value.clone()));
        if location.apply(&arg, &mut values) {
            data_args.push(arg);
            data_args.extend(taken);
            continue;
        }
        match arg.as_str() {
            "--client" => match rest.next() {
                Some(name) => client = Some(name),
                None => {
                    eprintln!("--client needs a name");
                    return 2;
                }
            },
            "--help" | "-h" => {
                println!("{HELP}");
                return 0;
            }
            _ => {
                eprintln!("Unknown option: {arg}");
                return 2;
            }
        }
    }
    let database = location.database();
    let probed = database.clone();
    let launcher = monocode_mcp::Launcher::new(
        Arc::new(move || launch::spawn(&data_args)),
        Arc::new(move || stored_enabled(&probed) == Some(true)),
    );
    // Inside one of MonoCode's sessions the environment names the endpoint
    // and the session; an app that does not answer then is not running.
    let (endpoint, session, role) = match session::from_env(|key| std::env::var(key).ok()) {
        None => (endpoint(&database), None, monocode_mcp::Role::Session),
        Some(Ok(relay)) => (Ok(relay.endpoint), Some(relay.credential), relay.role),
        Some(Err(error)) => (Err(error), None, monocode_mcp::Role::Session),
    };
    let when_unavailable: monocode_mcp::UnavailableProbe = if session.is_some() {
        Arc::new(ControlError::app_not_running)
    } else {
        Arc::new(move || unavailable(&database))
    };
    let options = monocode_mcp::Options::new(endpoint)
        .client_name(client)
        .session(session)
        .role(role)
        .launcher(launcher)
        .when_unavailable(when_unavailable);
    match monocode_mcp::run_stdio(options) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

/// The window's endpoint for `database`: the socket in the `control/`
/// subfolder of the database's folder (a pipe named after it on Windows).
pub(crate) fn endpoint(database: &Path) -> Result<Endpoint, ControlError> {
    database
        .parent()
        .ok_or_else(|| ControlError::invalid_input("The database path has no folder"))
        .and_then(Endpoint::for_data_dir)
}

/// What a caller is told when no app answers: `control_disabled` when the
/// stored policy has control off (or was never set), `app_not_running` when
/// it is on or the data cannot be read. The store is opened read-only.
pub(crate) fn unavailable(database: &Path) -> ControlError {
    if stored_enabled(database) == Some(false) {
        ControlError::control_disabled()
    } else {
        ControlError::app_not_running()
    }
}

/// Whether the stored policy has control on; `None` when the store cannot
/// be read. The store is opened read-only and never created.
pub(crate) fn stored_enabled(database: &Path) -> Option<bool> {
    monocode_core::store::Store::read_monocode(database)
        .ok()
        .and_then(|store| store.setting(POLICY_KEY).ok())
        .map(|raw| Policy::parse(raw.as_deref()).enabled)
}
