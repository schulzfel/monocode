//! R4: the queue card's buttons and the queue methods change the queue the
//! same way. Each case runs once by clicking the card and once by calling the
//! method external control calls, and compares what is left.
use super::*;
use gpui::{TestAppContext, VisualTestContext, WindowHandle};
use monocode_core::store::Store;
use std::sync::{Arc, Mutex, mpsc::Receiver};

fn conversation(cx: &mut TestAppContext) -> WindowHandle<Conversation> {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let summary = store.create("codex", "/work/monocode").unwrap();
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        input::bind(cx);
        cx.set_global(crate::settings::Preferences {
            follow_up: "queue".into(),
            ..Default::default()
        });
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| {
                let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                view.loaded = true;
                view
            })
        })
        .unwrap()
    })
}

/// A busy chat with "first" and "second" queued; the ids, and what the
/// provider receives.
fn queued(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Conversation>,
    (String, String),
    Receiver<Control>,
) {
    let window = conversation(cx);
    let (controls, received) = std::sync::mpsc::channel();
    let ids = window
        .update(cx, |view, _, cx| {
            view.busy = true;
            view.controls = Some(controls);
            for text in ["first", "second"] {
                view.input.update(cx, |i, cx| i.set_text(text.into(), cx));
                view.send(cx);
            }
            let ids: Vec<_> = view
                .follow_ups
                .state
                .messages
                .iter()
                .map(|m| m.id.clone())
                .collect();
            (ids[0].clone(), ids[1].clone())
        })
        .unwrap();
    cx.run_until_parked();
    (window, ids, received)
}

/// Clicks the last element `selector` names: the queue's last row.
fn click(window: WindowHandle<Conversation>, selector: &'static str, cx: &mut TestAppContext) {
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.refresh().unwrap();
    visual.refresh().unwrap();
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is shown"));
    visual.simulate_click(bounds.center(), Default::default());
}

/// The queue as the person sees it and as it is saved, with ids replaced by
/// their position so two chats compare.
type Snapshot = (
    Vec<(String, bool, Option<String>)>,
    Option<usize>,
    bool,
    Vec<String>,
);

fn snapshot(window: WindowHandle<Conversation>, cx: &mut TestAppContext) -> Snapshot {
    cx.run_until_parked();
    window
        .update(cx, |view, _, _| {
            let state = &view.follow_ups.state;
            let requested = view
                .follow_ups
                .requested
                .as_ref()
                .and_then(|id| state.messages.iter().position(|m| &m.id == id));
            let saved = locked(&view.store)
                .unwrap()
                .follow_ups(view.summary.id)
                .unwrap()
                .messages
                .into_iter()
                .map(|m| m.text)
                .collect();
            (
                state
                    .messages
                    .iter()
                    .map(|m| (m.text.clone(), m.steer, m.error.clone()))
                    .collect(),
                requested,
                state.paused,
                saved,
            )
        })
        .unwrap()
}

#[gpui::test]
fn remove_from_the_card_and_queue_remove_leave_the_same_queue(cx: &mut TestAppContext) {
    let (clicked, _, _received) = queued(cx);
    click(clicked, "queue-remove", cx);
    let by_card = snapshot(clicked, cx);
    let (called, (_, second), _received) = queued(cx);
    called
        .update(cx, |view, _, cx| view.queue_remove(&second, cx))
        .unwrap()
        .unwrap();
    let by_method = snapshot(called, cx);
    assert_eq!(by_card, by_method);
    assert_eq!(
        by_card.0,
        [("first".to_owned(), false, None)],
        "the second message is gone and saved so"
    );
    assert_eq!(by_card.3, ["first"]);
}

#[gpui::test]
fn steer_from_the_card_and_queue_steer_mark_the_same_message(cx: &mut TestAppContext) {
    let (clicked, _, _received) = queued(cx);
    click(clicked, "queue-steer", cx);
    let by_card = snapshot(clicked, cx);
    let (called, (_, second), _received) = queued(cx);
    called
        .update(cx, |view, _, cx| view.queue_steer(&second, cx))
        .unwrap()
        .unwrap();
    let by_method = snapshot(called, cx);
    assert_eq!(by_card, by_method);
    assert!(by_card.0[1].1, "the second message steers");
    assert_eq!(by_card.1, Some(1), "and is the one requested");
}

#[gpui::test]
fn resume_from_the_card_and_queue_resume_continue_the_same_way(cx: &mut TestAppContext) {
    let pause = |window: WindowHandle<Conversation>, paused: bool, cx: &mut TestAppContext| {
        window
            .update(cx, |view, _, cx| {
                view.busy = false;
                view.follow_ups.state.paused = paused;
                // The continuation first restores the unloaded history, and
                // never reaches a provider: an account switch holds launches.
                view.history_unloaded = true;
                view.account_switching = true;
                cx.notify();
            })
            .unwrap();
    };
    // Whether the continuation ran: history restored for it, nothing left
    // deferred, and the queue no longer resuming.
    let continued = |window: WindowHandle<Conversation>, cx: &mut TestAppContext| {
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                (
                    !view.history_unloaded,
                    view.history_actions.len(),
                    view.follow_ups.resuming,
                )
            })
            .unwrap()
    };
    let (clicked, _, _received) = queued(cx);
    pause(clicked, true, cx);
    click(clicked, "queue-resume", cx);
    let (called, _, _received) = queued(cx);
    pause(called, true, cx);
    called
        .update(cx, |view, _, cx| view.queue_resume(cx))
        .unwrap();
    assert_eq!(continued(clicked, cx), continued(called, cx));
    assert_eq!(continued(called, cx), (true, 0, false));
    let (running, _, _received) = queued(cx);
    pause(running, false, cx);
    running
        .update(cx, |view, _, cx| view.queue_resume(cx))
        .unwrap();
    assert_eq!(
        continued(running, cx),
        (false, 0, false),
        "a queue that is not paused has nothing to resume"
    );
}

#[gpui::test]
fn edits_match_the_inline_editor_and_respect_the_person(cx: &mut TestAppContext) {
    let (edited, (first, _), _received) = queued(cx);
    edited
        .update(cx, |view, window, cx| {
            view.edit_queued(first.clone(), window, cx);
            assert_eq!(
                view.queue_edit(&first, "from a client".into(), cx),
                Err(QueueRefused::Editing),
                "never under the person's cursor"
            );
            view.follow_ups
                .edit
                .as_ref()
                .unwrap()
                .input
                .update(cx, |i, cx| i.set_text("shorter".into(), cx));
            view.finish_queue_edit(true, window, cx);
        })
        .unwrap();
    let by_editor = snapshot(edited, cx);
    let (called, (first, _), _received) = queued(cx);
    called
        .update(cx, |view, _, cx| {
            assert_eq!(
                view.queue_edit(&first, " ".into(), cx),
                Err(QueueRefused::Empty)
            );
            view.queue_edit(&first, "shorter".into(), cx)
        })
        .unwrap()
        .unwrap();
    let by_method = snapshot(called, cx);
    assert_eq!(by_editor, by_method);
    assert_eq!(by_method.3, ["shorter", "second"]);
    called
        .update(cx, |view, _, cx| {
            assert_eq!(view.queue_remove("missing", cx), Err(QueueRefused::Unknown));
            view.follow_ups.sending = Some(view.follow_ups.state.messages[0].id.clone());
            let sending = view.follow_ups.state.messages[0].id.clone();
            assert_eq!(view.queue_steer(&sending, cx), Err(QueueRefused::Sending));
        })
        .unwrap();
}
