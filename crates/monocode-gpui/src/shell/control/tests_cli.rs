//! T-L5: the `app` command against a real control socket under
//! `target/c/`, with the test passing the listener's requests to the shell.
//! Its call reaches the shell as the session its environment names, the
//! shell resolves it to that session or, for a Mono's chat, to the Mono,
//! and its answer comes back in the CLI's envelope.
use super::tests::{scratch, sessions, setup};
use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_core::app_mcp::{ENV_ENDPOINT, ENV_ROLE, ENV_SESSION, ENV_TOKEN, ROLE_MONO, token};
use std::collections::HashMap;

/// A window with a listener on a fresh socket.
struct Window {
    window: WindowHandle<Shell>,
    endpoint: Endpoint,
    requests: async_channel::Receiver<Request>,
    /// Where the CLI's thread says it is done, after the listener's requests.
    done: async_channel::Sender<Request>,
    _server: listener::Server,
}

/// What the CLI's thread sends once its run is over.
const DONE: u64 = u64::MAX;

fn window(name: &str, cx: &mut TestAppContext) -> Window {
    serve(setup(cx), name)
}

/// `window` with a listener on a fresh socket named `name`.
fn serve(window: WindowHandle<Shell>, name: &str) -> Window {
    let endpoint = Endpoint::for_data_dir(&scratch(name)).unwrap();
    let (done, requests) = async_channel::unbounded();
    let server = listener::Server::start_with(
        Listener::bind(&endpoint).unwrap(),
        &endpoint,
        "1".into(),
        done.clone(),
    )
    .unwrap();
    Window {
        window,
        endpoint,
        requests,
        done,
        _server: server,
    }
}

/// Runs `app WORDS…` as session `id`'s agent would, with `role` as its
/// `MONOCODE_CONTROL_ROLE` when given, on its own thread, handing the
/// listener's requests to the shell until the run is over. Returns what it
/// printed, its exit code, and who the shell took each call to be from.
fn app_as(
    w: &Window,
    id: i64,
    role: Option<&str>,
    words: &[&str],
    cx: &mut TestAppContext,
) -> (Value, i32, Vec<CallerKind>) {
    let mut env = HashMap::from([
        (ENV_ENDPOINT, w.endpoint.describe()),
        (ENV_SESSION, id.to_string()),
        (ENV_TOKEN, token(id)),
    ]);
    if let Some(role) = role {
        env.insert(ENV_ROLE, role.to_owned());
    }
    let args: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
    let done = w.done.clone();
    let run = std::thread::spawn(move || {
        let outcome =
            crate::app_cli::execute(&args, |key| env.get(key).cloned(), &mut std::io::empty());
        let _ = done.send_blocking(Request::Closed { connection: DONE });
        (outcome.output, outcome.code)
    });
    let mut callers = Vec::new();
    loop {
        let request = w.requests.recv_blocking().unwrap();
        if matches!(request, Request::Closed { connection: DONE }) {
            break;
        }
        w.window
            .update(cx, |shell, window, cx| {
                // Who the shell takes the call to be from, as it decides.
                if let Request::Call { caller, .. } = &request {
                    callers.push(shell.resolve_caller(caller, cx).unwrap());
                }
                shell.handle_control(request, window, cx)
            })
            .unwrap();
        cx.run_until_parked();
    }
    let (output, code) = run.join().unwrap();
    (output, code, callers)
}

/// [`app_as`] for a run that makes one call.
fn app(w: &Window, id: i64, words: &[&str], cx: &mut TestAppContext) -> (Value, i32, CallerKind) {
    let (output, code, callers) = app_as(w, id, None, words, cx);
    let [caller] = callers.try_into().unwrap();
    (output, code, caller)
}

#[gpui::test]
fn the_app_command_calls_as_its_session_over_the_socket(cx: &mut TestAppContext) {
    let w = window("cl1", cx);
    let (_, b) = sessions(w.window, cx);
    let (output, code, caller) = app(&w, b, &["session_list"], cx);
    assert_eq!(caller, CallerKind::Session(b), "its session's own");
    assert_eq!(code, 0, "{output}");
    assert_eq!(output["ok"], true);
    assert!(output["result"]["sessions"].is_array(), "{output}");
    assert!(monocode_control::frame::valid_request_id(
        output["requestId"].as_str().unwrap()
    ));

    // An old action, translated on the way.
    let (legacy, code, caller) = app(&w, b, &["sessions.list"], cx);
    assert_eq!((code, caller), (0, CallerKind::Session(b)), "{legacy}");
    let listed: Vec<i64> = legacy["result"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_i64().unwrap())
        .collect();
    assert!(listed.contains(&b), "{legacy}");

    // The app's refusal, in the envelope with its code and request ID.
    let (refused, code, _) = app(
        &w,
        b,
        &["session_read", "--json", r#"{"sessionId":987654}"#],
        cx,
    );
    assert_eq!(code, 1);
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["code"], "not_found", "{refused}");
    assert_eq!(refused["retryable"], false);
    assert!(refused["requestId"].is_string());
}

/// T-L2: a change made again with the same `--request-id` returns the
/// first result and acts once; other input with it conflicts.
#[gpui::test]
fn a_retried_change_with_its_request_id_acts_once(cx: &mut TestAppContext) {
    let w = window("cl2", cx);
    let (_, b) = sessions(w.window, cx);
    let write = |body: &str, cx: &mut TestAppContext| {
        let input = format!(r#"{{"title":"Plan","body":"{body}"}}"#);
        app(
            &w,
            b,
            &["notes.write", "--json", &input, "--request-id", "cli-note"],
            cx,
        )
    };
    let (first, code, _) = write("Ship", cx);
    assert_eq!(code, 0, "{first}");
    assert_eq!(first["requestId"], "cli-note");
    let (again, _, _) = write("Ship", cx);
    assert_eq!(again, first, "the first result");
    let notes = w
        .window
        .update(cx, |shell, _, _| {
            locked(&shell.store).unwrap().list_notes().unwrap()
        })
        .unwrap();
    assert_eq!(notes.iter().filter(|n| n.title == "Plan").count(), 1);
    let (conflict, code, _) = write("Other", cx);
    assert_eq!(code, 1);
    assert_eq!(conflict["code"], "request_conflict", "{conflict}");
}

/// R13: a request ID the control rules would refuse is turned into one they
/// take, the same one on every run, so a retry still acts once; the output
/// names the ID as given.
#[gpui::test]
fn any_request_id_dedupes_its_retry(cx: &mut TestAppContext) {
    let w = window("cl3", cx);
    let (_, b) = sessions(w.window, cx);
    let write = |id: &str, cx: &mut TestAppContext| {
        let input = r#"{"title":"Odd","body":"Ship"}"#;
        app(
            &w,
            b,
            &["notes.write", "--json", input, "--request-id", id],
            cx,
        )
    };
    let (first, code, _) = write("note #1 (retry me)", cx);
    assert_eq!(code, 0, "{first}");
    assert_eq!(first["requestId"], "note #1 (retry me)");
    let (again, _, _) = write("note #1 (retry me)", cx);
    assert_eq!(again, first, "the first result");
    let (other, code, _) = write("note #2 (retry me)", cx);
    assert_eq!(code, 0, "{other}");
    let notes = w
        .window
        .update(cx, |shell, _, _| {
            locked(&shell.store).unwrap().list_notes().unwrap()
        })
        .unwrap();
    assert_eq!(notes.iter().filter(|n| n.title == "Odd").count(), 2);
}

/// R7: the old `sessions.list` returned every session; the `app` command
/// follows the pages and prints them as one list. The operation itself
/// still pages.
#[gpui::test]
fn the_old_session_list_returns_every_page(cx: &mut TestAppContext) {
    let w = window("cl4", cx);
    let (_, b) = sessions(w.window, cx);
    let project = w
        .window
        .update(cx, |shell, _, cx| {
            let summary = shell.session_metadata(b, cx).summary.clone();
            for n in 1..=250 {
                let mut extra = summary.clone();
                extra.id = 100_000 + n;
                shell.session_summaries.insert(extra.id, extra);
                shell.order.push(100_000 + n);
            }
            summary.cwd
        })
        .unwrap();
    let (legacy, code, callers) = app_as(&w, b, None, &["sessions.list"], cx);
    assert_eq!(code, 0, "{legacy}");
    assert_eq!(callers.len(), 2, "two pages");
    let listed = legacy["result"]["sessions"].as_array().unwrap();
    assert_eq!(listed.len(), 251, "{} listed", listed.len());
    assert_eq!(legacy["result"]["total"], 251);
    assert_eq!(legacy["result"]["project"], project);
    assert!(legacy["result"].get("nextCursor").is_none());
    let (page, code, _) = app(&w, b, &["session_list", "--json", r#"{"limit":200}"#], cx);
    assert_eq!(code, 0, "{page}");
    assert_eq!(page["result"]["sessions"].as_array().unwrap().len(), 200);
    assert!(page["result"]["nextCursor"].is_string());
}

/// R20: the `app` command run in a Mono's chat, with role `mono`, calls as
/// the Mono: the shell resolves it so, a Mono's own operation works, and
/// the activity log names the Mono.
#[gpui::test]
fn the_app_command_in_a_monos_chat_calls_as_the_mono(cx: &mut TestAppContext) {
    let s = crate::monos::tests::setup(cx);
    crate::monos::tests_parity_mono::chat(&s, cx);
    let w = serve(s.window, "cl5");
    let (read, code, callers) = app_as(&w, s.session, Some(ROLE_MONO), &["soul.read"], cx);
    assert_eq!(code, 0, "{read}");
    let mono = CallerKind::Mono {
        session: s.session,
        mono: s.id.clone(),
        run: false,
    };
    assert_eq!(callers, std::slice::from_ref(&mono));
    let input = r#"{"fact":"The build uses cargo +stable"}"#;
    let (added, code, callers) = app_as(
        &w,
        s.session,
        Some(ROLE_MONO),
        &["memory.add", "--json", input],
        cx,
    );
    assert_eq!(code, 0, "{added}");
    assert_eq!(callers, [mono]);
    let label = s
        .manager
        .read_with(cx, |m, _| m.roster.get(&s.id).unwrap().label());
    let activity = s
        .window
        .update(cx, |shell, _, _| shell.control_activity())
        .unwrap();
    assert!(
        activity
            .iter()
            .any(|(client, op, _)| *client == label && op == "mono_memory_add"),
        "{activity:?}"
    );
}
