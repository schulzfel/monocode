use super::request::{Request, View, inside};
use super::server;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
};

/// A short folder inside the checkout, so the socket fits `sun_path`.
fn scratch(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/c");
    std::fs::create_dir_all(&root).unwrap();
    let dir = root.canonicalize().unwrap().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data/shots")).unwrap();
    dir
}

#[test]
fn every_request_parses_from_one_json_line() {
    let data = scratch("qa-parse").join("data");
    let parse = |line: &str| Request::parse(line, &data);
    assert_eq!(
        parse(r#"{"op":"submit","session":3,"text":"/operator @qa env"}"#).unwrap(),
        Request::Submit {
            session: 3,
            text: "/operator @qa env".into()
        }
    );
    assert_eq!(
        parse(r#"{"op":"action","name":"workspace::OpenSettings"}"#).unwrap(),
        Request::Action {
            name: "workspace::OpenSettings".into(),
            data: None
        }
    );
    assert_eq!(parse(r#"{"op":"state"}"#).unwrap(), Request::State {});
    assert_eq!(parse(r#"{"op":"quit"}"#).unwrap(), Request::Quit {});
    let view = parse(r#"{"op":"view","mono":"m1"}"#).unwrap();
    assert_eq!(view.view().unwrap(), View::Mono("m1".into()));
    assert_eq!(
        parse(r#"{"op":"view","session":7}"#)
            .unwrap()
            .view()
            .unwrap(),
        View::Session(7)
    );
    assert!(
        parse(r#"{"op":"view"}"#)
            .unwrap_err()
            .contains("exactly one")
    );
    assert!(
        parse(r#"{"op":"view","session":1,"mono":"m"}"#)
            .unwrap_err()
            .contains("exactly one")
    );
    assert!(parse(r#"{"op":"type","text":"x"}"#).is_err());
    assert!(parse("not json").is_err());
}

#[test]
fn a_capture_stays_inside_the_data_folder() {
    let dir = scratch("qa-capture-path");
    let data = dir.join("data");
    let shot = data.join("shots/window.png");
    assert_eq!(
        inside(&shot, &data).unwrap(),
        data.canonicalize().unwrap().join("shots/window.png")
    );
    for (path, why) in [
        (dir.join("outside.png"), "outside the data folder"),
        (
            data.join("shots/../../escape.png"),
            "outside the data folder",
        ),
        (PathBuf::from("shots/window.png"), "not an absolute path"),
        (data.join("shots/window.jpg"), "not a .png file"),
        (data.join("missing/window.png"), "cannot be resolved"),
    ] {
        let error = inside(&path, &data).unwrap_err();
        assert!(error.contains(why), "{}: {error}", path.display());
    }
    std::os::unix::fs::symlink(&dir, data.join("link")).unwrap();
    assert!(
        inside(&data.join("link/out.png"), &data)
            .unwrap_err()
            .contains("outside")
    );
}

fn ask(stream: &mut UnixStream, line: &str) -> Value {
    writeln!(stream, "{line}").unwrap();
    let mut answer = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut answer)
        .unwrap();
    serde_json::from_str(&answer).unwrap()
}

#[test]
fn the_socket_answers_each_line_and_refuses_bad_requests_itself() {
    let dir = scratch("qa-socket");
    let socket = dir.join("hook.sock");
    let (calls, requests) = async_channel::unbounded::<server::Call>();
    server::listen(&socket, dir.join("data"), calls).unwrap();
    let window = std::thread::spawn(move || {
        let mut seen = Vec::new();
        while let Ok(call) = requests.recv_blocking() {
            seen.push(format!("{:?}", call.request));
            let quit = matches!(call.request, Request::Quit {});
            call.answer(json!({"ok": true, "result": seen.len()}));
            if quit {
                return seen;
            }
        }
        seen
    });
    let mut stream = UnixStream::connect(&socket).unwrap();
    assert_eq!(ask(&mut stream, r#"{"op":"state"}"#)["result"], json!(1));
    let refused = ask(
        &mut stream,
        &json!({"op": "capture", "path": dir.join("x.png")}).to_string(),
    );
    assert_eq!(refused["ok"], json!(false));
    assert!(refused["error"].as_str().unwrap().contains("outside"));
    assert_eq!(ask(&mut stream, "{")["ok"], json!(false));
    assert_eq!(ask(&mut stream, r#"{"op":"quit"}"#)["result"], json!(2));
    assert_eq!(window.join().unwrap(), ["State", "Quit"]);
    // A stale socket is replaced; any other file is left alone.
    let (calls, _requests) = async_channel::unbounded::<server::Call>();
    server::listen(&socket, dir.join("data"), calls).unwrap();
    let file = dir.join("file.sock");
    std::fs::write(&file, "keep").unwrap();
    let (calls, _requests) = async_channel::unbounded::<server::Call>();
    assert!(
        server::listen(&file, dir.join("data"), calls)
            .unwrap_err()
            .contains("not a socket")
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep");
}

/// Release builds contain no hook: its module, the shell's half and its
/// start are compiled only with debug assertions, and a release launch
/// never reads `MONOCODE_QA_HOOK`.
#[test]
fn release_builds_do_not_contain_the_hook() {
    let main = include_str!("../main.rs");
    let shell = include_str!("../shell.rs");
    assert!(main.contains("#[cfg(debug_assertions)]\nmod qa_hook;\n"));
    assert!(shell.contains("#[cfg(debug_assertions)]\nmod qa;\n"));
    assert!(main.contains("#[cfg(debug_assertions)]\n            qa_hook::start(window, cx);"));
    assert!(
        main.contains(
            "#[cfg(not(debug_assertions))]\nfn qa_hook_requested() -> bool {\n    false\n}"
        )
    );
    let uses = |source: &str| {
        source
            .lines()
            .filter(|line| line.contains("qa_hook::") || line.contains(".qa_"))
            .count()
    };
    // `requested()` in the debug-only `qa_hook_requested` and `start`.
    assert_eq!(uses(main), 2);
    assert_eq!(uses(shell), 0);
}
