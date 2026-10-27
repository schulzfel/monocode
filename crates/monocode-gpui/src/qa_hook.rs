//! The debug-only QA hook: with `MONOCODE_QA_HOOK=<socket path>` set, a
//! development build listens on that Unix socket so the hands-off QA lane
//! can drive the window as the person would, without taking their
//! keyboard, mouse or focus. One JSON line per request, one per answer:
//!
//! - `{"op":"submit","session":ID,"text":"…"}` types into that chat's
//!   composer and presses Enter, so slash commands such as `/operator`
//!   take the person's path. The control layer still refuses `/operator`.
//! - `{"op":"action","name":"workspace::OpenSettings","data":…}` dispatches
//!   a registered action to the window.
//! - `{"op":"view","session":ID}` or `{"op":"view","mono":"ID"}` shows it.
//! - `{"op":"capture","path":"…/x.png"}` writes the window as a PNG inside
//!   the data folder (`capture`).
//! - `{"op":"state"}` says what the window shows; `{"op":"quit"}` quits.
//!
//! With the hook active the app never activates itself or its window.
//! Release builds do not contain this module, and it only runs on
//! development data.
use crate::shell::Shell;
use gpui::{App, AsyncApp, WindowHandle};
use serde_json::{Value, json};
use std::path::PathBuf;

mod capture;
mod request;
mod server;
#[cfg(test)]
mod tests;

pub(crate) use request::{Request, View};

/// The variable naming the hook's socket.
pub(crate) const ENV: &str = "MONOCODE_QA_HOOK";

/// The hook's socket, when the lane asked for one.
pub(crate) fn socket() -> Option<PathBuf> {
    std::env::var_os(ENV)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// Whether this launch runs under the QA hook, and so must not take focus.
pub(crate) fn requested() -> bool {
    socket().is_some()
}

/// Starts the hook for `window` when it was requested on development data.
pub(crate) fn start(window: WindowHandle<Shell>, cx: &mut App) {
    let Some(path) = socket() else {
        return;
    };
    if crate::data_location::mode() != monocode_control::ops::app::DataMode::Dev {
        eprintln!("The QA hook only runs on development data (--dev-data); it is off.");
        return;
    }
    let (calls, requests) = async_channel::unbounded::<server::Call>();
    if let Err(error) = server::listen(&path, crate::data_dir(), calls) {
        eprintln!("The QA hook cannot listen: {error}");
        return;
    }
    eprintln!("QA hook listening on {}", path.display());
    cx.spawn(async move |cx| {
        while let Ok(call) = requests.recv().await {
            let quit = matches!(call.request, Request::Quit {});
            let answer = answer(&call.request, window, cx).await;
            call.answer(match answer {
                Ok(value) => json!({"ok": true, "result": value}),
                Err(error) => json!({"ok": false, "error": error}),
            });
            if quit {
                let _ = window.update(cx, |shell, window, cx| shell.qa_quit(window, cx));
            }
        }
    })
    .detach();
}

async fn answer(
    request: &Request,
    window: WindowHandle<Shell>,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    match request {
        Request::Submit { session, text } => window
            .update(cx, |shell, _, cx| {
                shell.qa_submit(*session, text.clone(), cx)
            })
            .map_err(|e| e.to_string())?,
        Request::Action { name, data } => window
            .update(cx, |_, window, cx| {
                let action = cx
                    .build_action(name, data.clone())
                    .map_err(|e| e.to_string())?;
                window.dispatch_action(action, cx);
                Ok(json!({"dispatched": name}))
            })
            .map_err(|e| e.to_string())?,
        Request::View { .. } => {
            let view = request.view()?;
            window
                .update(cx, |shell, window, cx| shell.qa_view(&view, window, cx))
                .map_err(|e| e.to_string())?
        }
        Request::Capture { path } => capture::capture(window, path, cx).await,
        Request::State {} => window
            .update(cx, |shell, window, cx| shell.qa_state(window, cx))
            .map_err(|e| e.to_string()),
        Request::Quit {} => Ok(json!({"quitting": true})),
    }
}

/// The window's native facts for `state`: its number, whether the app and
/// the window are active, whether it is ordered in, whether other windows
/// cover all of it (`occluded`: AppKit then stops its frames), and whether
/// the app may record the screen.
pub(crate) fn window_facts(window: &gpui::Window) -> Value {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWindowOcclusionState;
        let ns_window = crate::traffic_lights::ns_window(window);
        let app_active = objc2::MainThreadMarker::new()
            .is_some_and(|mtm| objc2_app_kit::NSApplication::sharedApplication(mtm).isActive());
        json!({
            "number": capture::native(window).map(|native| native.number),
            "appActive": app_active,
            "windowActive": window.is_window_active(),
            "key": ns_window.as_ref().is_some_and(|w| w.isKeyWindow()),
            "visible": ns_window.as_ref().is_some_and(|w| w.isVisible()),
            "occluded": ns_window
                .as_ref()
                .is_some_and(|w| !w.occlusionState().contains(NSWindowOcclusionState::Visible)),
            "screenRecordingGranted": capture::preflight(),
        })
    }
    #[cfg(not(target_os = "macos"))]
    json!({"windowActive": window.is_window_active()})
}
