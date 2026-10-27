//! T-P1 through a real turn: the Operator instructions a turn's agent reads
//! follow the control environment that turn gets, computed once for both.
use super::*;
use monocode_core::app_mcp::Access;
use std::{os::unix::fs::PermissionsExt, path::Path};

/// Runs one Codex turn of an Operator thread of `store` in `cwd`, against
/// a mock Codex that records the text it is sent; returns that text.
fn operator_turn(store: &Database, cwd: &Path) -> String {
    let record = cwd.join("prompt.txt");
    let binary = cwd.join("mock-codex");
    std::fs::write(
        &binary,
        format!(
            r#"#!/usr/bin/env python3
import json, sys
def send(value):
    print(json.dumps(value), flush=True)
for line in sys.stdin:
    value = json.loads(line)
    if value.get('id') == 1:
        send({{'id': 1, 'result': {{}}}})
    elif value.get('id') == 2:
        send({{'id': 2, 'result': {{'thread': {{'id': 'operator-test'}}}}}})
    elif value.get('id') == 3:
        with open({record:?}, 'w') as output:
            output.write(value['params']['input'][0]['text'])
        send({{'method': 'turn/completed', 'params': {{'turn': {{'status': 'completed'}}}}}})
        break
"#
        ),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let summary = locked(store)
        .unwrap()
        .create("codex", cwd.to_str().unwrap())
        .unwrap();
    let settings = Settings {
        operator_enabled: true,
        ..Settings::default()
    };
    let (updates, receive) = async_channel::bounded(64);
    let (_controls, control_rx) = std::sync::mpsc::channel();
    let store = store.clone();
    let worker = std::thread::spawn(move || {
        run_turn_using(
            TurnInput {
                summary,
                prompt: "List my notes".into(),
                settings,
            },
            control_rx,
            store,
            Arc::new(AtomicBool::new(false)),
            updates,
            |_| Some(binary),
        );
    });
    while !receive
        .recv_blocking()
        .expect("Runtime closed before its final update")
        .finished
    {}
    worker.join().unwrap();
    std::fs::read_to_string(record).unwrap()
}

#[test]
fn an_operator_turn_is_told_the_tools_its_turn_gets() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/c/op1");
    let _ = std::fs::remove_dir_all(&dir);
    for project in ["served", "unserved"] {
        std::fs::create_dir_all(dir.join(project)).unwrap();
    }
    let dir = dir.canonicalize().unwrap();
    let store = Arc::new(Mutex::new(
        Store::open_monocode(&dir.join("monocode.db")).unwrap(),
    ));
    let endpoint = monocode_control::Endpoint::for_data_dir(&dir).unwrap();

    let serving = app_control::serving(&endpoint);
    let cwd = dir.join("served");
    assert_eq!(
        operator_turn(&store, &cwd),
        operator_prompt::wrap(Access::Mcp, cwd.to_str().unwrap(), "", "List my notes"),
        "Codex gets the relay, so it is told the MCP tools"
    );
    drop(serving);

    let cwd = dir.join("unserved");
    assert_eq!(
        operator_turn(&store, &cwd),
        operator_prompt::wrap(
            Access::Unavailable("MonoCode's control socket is not served"),
            cwd.to_str().unwrap(),
            "",
            "List my notes"
        ),
        "without a served socket it is told the tools are unavailable"
    );
}
