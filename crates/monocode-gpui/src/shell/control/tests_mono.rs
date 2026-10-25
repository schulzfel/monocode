//! `mono_list`, `mono_open` and `mono_send`. The shell keeps its store in
//! the checkout's `target/c/`, so the files a Mono opens stay there too.
//! Messages are only ever refused here: delivering one starts a provider.
use super::tests::{CLIENT, approve, run, scratch};
use super::*;
use crate::workspace::Workspace;
use gpui::{TestAppContext, WindowHandle};
use monocode_control::{
    Command, Scope,
    ops::mono::{
        MonoInput, MonoListInput, MonoListOutput, MonoOpenOutput, MonoSendInput, MonoSendOutput,
    },
};
use monocode_core::store::Store;

const HERE: &str = "/work/monocode";

/// A shell on a store in `target/c/<name>`, with one Mono called Scout.
fn fixture(name: &str, cx: &mut TestAppContext) -> (WindowHandle<Shell>, String) {
    let dir = scratch(name);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("monocode.db")).unwrap();
    let session = store.create("codex", HERE).unwrap();
    let mut state = Workspace::default();
    state.add_project(HERE);
    state.open(session.id);
    let saved = serde_json::to_string(&state).unwrap();
    let store = Arc::new(Mutex::new(store));
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Shell::new(store, vec![session], HERE.into(), Some(saved), cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    approve(
        window,
        &[(CLIENT, &[Scope::Read, Scope::Navigate, Scope::Dispatch])],
        cx,
    );
    let id = window
        .update(cx, |shell, _, cx| {
            shell.mono_manager().update(cx, |monos, _| {
                let id = monos.roster.create(vec![HERE.into()]).id.clone();
                monos.roster.get_mut(&id).unwrap().name = "Scout".into();
                id
            })
        })
        .unwrap();
    (window, id)
}

fn send(id: &str) -> Command {
    Command::MonoSend(MonoSendInput {
        mono_id: id.into(),
        text: "What changed today?".into(),
    })
}

fn busy(window: WindowHandle<Shell>, cx: &mut TestAppContext) -> bool {
    let listed: MonoListOutput = run(window, Command::MonoList(MonoListInput {}), cx).unwrap();
    listed.monos[0].busy
}

#[gpui::test]
fn monos_are_listed_and_opened(cx: &mut TestAppContext) {
    let (window, id) = fixture("q7", cx);
    let listed: MonoListOutput = run(window, Command::MonoList(MonoListInput {}), cx).unwrap();
    assert_eq!(listed.monos.len(), 1);
    assert_eq!(listed.monos[0].id, id);
    assert_eq!(listed.monos[0].name, "Scout");
    assert_eq!(listed.monos[0].projects, [HERE]);
    assert!(!listed.monos[0].busy);

    let opened: MonoOpenOutput = run(
        window,
        Command::MonoOpen(MonoInput {
            mono_id: id.clone(),
        }),
        cx,
    )
    .unwrap();
    assert_eq!(opened.name, "Scout");
    window
        .update(cx, |shell, _, cx| {
            let monos = shell.mono_manager();
            assert_eq!(
                monos.read(cx).current().map(|m| m.id.clone()),
                Some(id.clone())
            );
            assert!(monos.read(cx).open_chat(&id).is_some(), "its chat is open");
        })
        .unwrap();
    let missing = run::<MonoOpenOutput>(
        window,
        Command::MonoOpen(MonoInput {
            mono_id: "nope".into(),
        }),
        cx,
    );
    assert_eq!(missing.unwrap_err().code, ErrorCode::NotFound);
}

#[gpui::test]
fn a_busy_mono_refuses_messages(cx: &mut TestAppContext) {
    let (window, id) = fixture("q8", cx);
    // A message waiting in its outbox.
    window
        .update(cx, |shell, _, cx| {
            shell.mono_manager().update(cx, |monos, _| {
                monos
                    .roster
                    .get_mut(&id)
                    .unwrap()
                    .outbox
                    .push(monocode_core::mono::Message {
                        id: "m1".into(),
                        text: "Earlier".into(),
                        sent_at: None,
                        internal: false,
                        error: None,
                        attachments: Vec::new(),
                    });
            });
        })
        .unwrap();
    assert!(busy(window, cx));
    let refused = run::<MonoSendOutput>(window, send(&id), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::SessionBusy);
    assert!(refused.retryable);
    let outbox = |window: WindowHandle<Shell>, cx: &mut TestAppContext| {
        window
            .update(cx, |shell, _, cx| {
                let monos = shell.mono_manager();
                let texts: Vec<_> = monos
                    .read(cx)
                    .roster
                    .get(&id)
                    .unwrap()
                    .outbox
                    .iter()
                    .map(|m| m.text.clone())
                    .collect();
                texts
            })
            .unwrap()
    };
    assert_eq!(
        outbox(window, cx),
        ["Earlier"],
        "the refused message is not queued"
    );

    // Its chat answering.
    window
        .update(cx, |shell, window, cx| {
            shell.mono_manager().update(cx, |monos, cx| {
                monos.roster.get_mut(&id).unwrap().outbox.clear();
                monos.open(&id, window, cx);
            });
        })
        .unwrap();
    cx.run_until_parked();
    assert!(!busy(window, cx));
    window
        .update(cx, |shell, _, cx| {
            let chat = shell.mono_manager().read(cx).open_chat(&id).unwrap();
            chat.update(cx, |chat, _| chat.busy = true);
        })
        .unwrap();
    assert!(busy(window, cx));
    let refused = run::<MonoSendOutput>(window, send(&id), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::SessionBusy);
    assert!(outbox(window, cx).is_empty());

    let blank = run::<MonoSendOutput>(
        window,
        Command::MonoSend(MonoSendInput {
            mono_id: id.clone(),
            text: " ".into(),
        }),
        cx,
    );
    assert_eq!(blank.unwrap_err().code, ErrorCode::InvalidInput);
    let missing = run::<MonoSendOutput>(window, send("nope"), cx);
    assert_eq!(missing.unwrap_err().code, ErrorCode::NotFound);
}
