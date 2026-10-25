//! R3: the question card and `answer_question` answer the same way.
use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_core::{
    agent::{QuestionItem, QuestionOption},
    store::Store,
};
use std::sync::{Arc, Mutex, mpsc::Receiver};

fn conversation(cx: &mut TestAppContext) -> WindowHandle<Conversation> {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let summary = store.create("claude", "/work/monocode").unwrap();
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        input::bind(cx);
        cx.set_global(crate::settings::Preferences::default());
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

fn question(id: &str) -> Question {
    let option = |id: &str, label: &str| QuestionOption {
        id: id.into(),
        label: label.into(),
        description: String::new(),
    };
    Question {
        id: id.into(),
        title: "Which way?".into(),
        auto_resolve_at: None,
        items: vec![QuestionItem {
            id: "way".into(),
            prompt: "Pick a way".into(),
            multiple: false,
            allow_custom: true,
            options: vec![option("left", "Left"), option("right", "Right")],
        }],
    }
}

/// What each path leaves behind: the decision sent, the open questions and
/// the status line.
fn outcome(
    window: WindowHandle<Conversation>,
    received: &Receiver<Control>,
    cx: &mut TestAppContext,
) -> (String, String, usize, String) {
    cx.run_until_parked();
    let control = received.try_recv().expect("the provider gets one decision");
    let decision = format!("{:?}", control.decision);
    window
        .update(cx, |view, _, _| {
            (
                control.id,
                decision,
                view.questions.len(),
                view.status.to_string(),
            )
        })
        .unwrap()
}

#[gpui::test]
fn the_card_and_answer_question_send_the_same_answer_and_settle_alike(cx: &mut TestAppContext) {
    let answer = Some(vec![vec!["Right".to_owned()]]);
    // The card's own reply, as clicking an option and Continue emits it.
    let card_window = conversation(cx);
    let received = card_window
        .update(cx, |view, _, cx| view.test_ask(question("q1"), cx))
        .unwrap();
    let card = card_window
        .update(cx, |view, _, _| view.questions[0].0.clone())
        .unwrap();
    card.update(cx, |_, cx| {
        cx.emit(crate::questions::Answered {
            id: "q1".into(),
            answers: answer.clone(),
        });
    });
    let from_card = outcome(card_window, &received, cx);
    // The same answer through the method external control calls.
    let method_window = conversation(cx);
    let received = method_window
        .update(cx, |view, _, cx| {
            let received = view.test_ask(question("q1"), cx);
            assert_eq!(view.answer_question("q1", answer.clone(), cx), Ok(()));
            received
        })
        .unwrap();
    let from_method = outcome(method_window, &received, cx);
    assert_eq!(from_card, from_method);
    assert_eq!(from_card.1, r#"Answer([["Right"]])"#);
    assert_eq!(from_card.2, 0, "the card is released");
}

#[gpui::test]
fn declining_from_either_path_denies(cx: &mut TestAppContext) {
    let window = conversation(cx);
    let received = window
        .update(cx, |view, _, cx| view.test_ask(question("q1"), cx))
        .unwrap();
    let card = window
        .update(cx, |view, _, _| view.questions[0].0.clone())
        .unwrap();
    card.update(cx, |_, cx| {
        cx.emit(crate::questions::Answered {
            id: "q1".into(),
            answers: None,
        });
    });
    assert_eq!(outcome(window, &received, cx).1, "Deny");
    let window = conversation(cx);
    let received = window
        .update(cx, |view, _, cx| {
            let received = view.test_ask(question("q1"), cx);
            assert_eq!(view.answer_question("q1", None, cx), Ok(()));
            received
        })
        .unwrap();
    assert_eq!(outcome(window, &received, cx).1, "Deny");
}

#[gpui::test]
fn only_the_first_open_question_is_answered_and_a_gone_provider_shows_why(cx: &mut TestAppContext) {
    let window = conversation(cx);
    window
        .update(cx, |view, _, cx| {
            let received = view.test_ask(question("q1"), cx);
            view.ask_question(question("q2"), cx);
            assert_eq!(
                view.answer_question("q2", None, cx),
                Err(AnswerRefused::NotFirst("q1".into()))
            );
            assert_eq!(
                view.answer_question("nope", None, cx),
                Err(AnswerRefused::Unknown)
            );
            assert!(received.try_recv().is_err(), "nothing was sent");
            assert_eq!(view.questions.len(), 2);
            view.controls = None;
            assert_eq!(
                view.answer_question("q1", None, cx),
                Err(AnswerRefused::Gone)
            );
            assert_eq!(view.questions.len(), 2, "the card stays to retry");
            assert!(matches!(view.status, SessionStatus::Error { .. }));
        })
        .unwrap();
}
