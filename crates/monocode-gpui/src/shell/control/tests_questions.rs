//! `question_list`, `question_answer` and the queue operations against the
//! fixture shell. Questions are put in front of a chat as a turn would; the
//! provider's side is a channel, so no provider process ever starts.
use super::tests::{CLIENT, approve, run, two_sessions};
use super::*;
use gpui::TestAppContext;
use monocode_control::{
    Command, Scope,
    ops::{
        question::{
            ItemAnswer, QuestionAnswerInput, QuestionAnswerOutput, QuestionListInput,
            QuestionListOutput,
        },
        queue::{QueueEditInput, QueueItemInput, QueueListOutput, QueueOutput},
        session::{SessionInput, SessionStatus as Status},
    },
};
use monocode_core::agent::{Approval, Control, Decision, Question, QuestionItem, QuestionOption};
use std::sync::mpsc::Receiver;

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

/// Session `id` asks `q1`, then `q2`, and waits on an approval.
fn ask(window: gpui::WindowHandle<Shell>, id: i64, cx: &mut TestAppContext) -> Receiver<Control> {
    let received = window
        .update(cx, |shell, _, cx| {
            let view = shell.ensure_session(id, cx).unwrap();
            view.update(cx, |view, cx| {
                let received = view.test_ask(question("q1"), cx);
                view.test_approval(
                    Approval {
                        id: "a1".into(),
                        title: "Run cargo test".into(),
                        detail: "x".repeat(3000),
                        can_remember: true,
                    },
                    cx,
                );
                received
            })
        })
        .unwrap();
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&id].update(cx, |view, cx| view.test_ask_more(question("q2"), cx));
        })
        .unwrap();
    received
}

fn answer(id: i64, question: &str, answers: Vec<ItemAnswer>) -> Command {
    Command::QuestionAnswer(QuestionAnswerInput {
        session_id: id,
        question_id: question.into(),
        answers,
    })
}

fn pick(options: &[&str], text: Option<&str>) -> Vec<ItemAnswer> {
    vec![ItemAnswer {
        item_id: "way".into(),
        options: options.iter().map(|o| (*o).to_owned()).collect(),
        text: text.map(str::to_owned),
    }]
}

#[gpui::test]
fn question_list_shows_questions_and_approvals_but_only_questions_are_answerable(
    cx: &mut TestAppContext,
) {
    let (window, _, id) = two_sessions(&[Scope::Read, Scope::Dispatch], cx);
    let _received = ask(window, id, cx);
    let list: QuestionListOutput = run(
        window,
        Command::QuestionList(QuestionListInput::default()),
        cx,
    )
    .unwrap();
    let ids: Vec<_> = list
        .questions
        .iter()
        .map(|q| (q.session_id, q.question_id.as_str(), q.answerable))
        .collect();
    assert_eq!(ids, [(id, "q1", true), (id, "q2", false)]);
    let item = &list.questions[0].items[0];
    assert_eq!(
        (
            item.id.as_str(),
            item.options[1].id.as_str(),
            item.options[1].label.as_str()
        ),
        ("way", "right", "Right")
    );
    assert_eq!(list.approvals.len(), 1);
    assert_eq!(list.approvals[0].approval_id, "a1");
    assert_eq!(list.approvals[0].detail.chars().count(), 2000, "capped");
    let page: QuestionListOutput = run(
        window,
        Command::QuestionList(QuestionListInput {
            limit: Some(1),
            ..QuestionListInput::default()
        }),
        cx,
    )
    .unwrap();
    assert_eq!(page.questions.len(), 1);
    assert_eq!(page.next_cursor.as_deref(), Some("1"));
}

#[gpui::test]
fn question_answer_sends_the_cards_answer_and_releases_the_card(cx: &mut TestAppContext) {
    let (window, _, id) = two_sessions(&[Scope::Read, Scope::Dispatch], cx);
    let received = ask(window, id, cx);
    let later = run::<QuestionAnswerOutput>(window, answer(id, "q2", pick(&["left"], None)), cx);
    assert_eq!(
        later.unwrap_err().code,
        ErrorCode::InvalidInput,
        "q1 comes first"
    );
    for (bad, why) in [
        (pick(&["up"], None), "an unknown choice"),
        (
            pick(&["left", "right"], None),
            "two choices for a single item",
        ),
        (
            vec![ItemAnswer {
                item_id: "nope".into(),
                ..ItemAnswer::default()
            }],
            "an unknown item",
        ),
    ] {
        let error = run::<QuestionAnswerOutput>(window, answer(id, "q1", bad), cx).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{why}");
    }
    assert!(received.try_recv().is_err(), "nothing reached the provider");
    // By label or id; typed text replaces a single choice, as on the card.
    let done: QuestionAnswerOutput = run(
        window,
        answer(id, "q1", pick(&["Left"], Some("  Neither  "))),
        cx,
    )
    .unwrap();
    assert!(!done.declined);
    let control = received.try_recv().unwrap();
    assert_eq!(control.id, "q1");
    assert!(
        matches!(&control.decision, Decision::Answer(a) if *a == [vec!["Neither".to_owned()]]),
        "{:?}",
        control.decision
    );
    assert_eq!(
        done.status,
        Status::AwaitingApproval,
        "the approval still waits"
    );
    let declined: QuestionAnswerOutput = run(window, answer(id, "q2", Vec::new()), cx).unwrap();
    assert!(declined.declined);
    assert!(matches!(
        received.try_recv().unwrap().decision,
        Decision::Deny
    ));
    let list: QuestionListOutput = run(
        window,
        Command::QuestionList(QuestionListInput::default()),
        cx,
    )
    .unwrap();
    assert!(list.questions.is_empty(), "both cards are gone");
    assert_eq!(list.approvals.len(), 1, "approvals stay with the person");
    let gone = run::<QuestionAnswerOutput>(window, answer(id, "q1", Vec::new()), cx);
    assert_eq!(gone.unwrap_err().code, ErrorCode::NotFound);
}

#[gpui::test]
fn answering_needs_dispatch_and_listing_needs_read(cx: &mut TestAppContext) {
    let (window, _, id) = two_sessions(&[Scope::Read, Scope::Dispatch], cx);
    let _received = ask(window, id, cx);
    approve(window, &[(CLIENT, &[Scope::Read])], cx);
    let denied = run::<QuestionAnswerOutput>(window, answer(id, "q1", Vec::new()), cx);
    assert_eq!(denied.unwrap_err().code, ErrorCode::ScopeDenied);
    approve(window, &[(CLIENT, &[Scope::Dispatch])], cx);
    let denied = run::<QuestionListOutput>(
        window,
        Command::QuestionList(QuestionListInput::default()),
        cx,
    );
    assert_eq!(denied.unwrap_err().code, ErrorCode::ScopeDenied);
}

/// Session `id` is busy with "first" and "second" queued; their ids.
fn queue(window: gpui::WindowHandle<Shell>, id: i64, cx: &mut TestAppContext) -> Vec<String> {
    window
        .update(cx, |shell, _, cx| {
            let view = shell.ensure_session(id, cx).unwrap();
            view.update(cx, |view, cx| {
                view.busy = true;
                for text in ["first", "second"] {
                    view.submit_external(text.into(), Vec::new(), Some(false), cx);
                }
            });
        })
        .unwrap();
    cx.run_until_parked();
    let list: QueueListOutput = run(
        window,
        Command::QueueList(SessionInput { session_id: id }),
        cx,
    )
    .unwrap();
    list.messages.into_iter().map(|m| m.id).collect()
}

fn item(id: i64, message: &str) -> QueueItemInput {
    QueueItemInput {
        session_id: id,
        message_id: message.into(),
    }
}

#[gpui::test]
fn queue_operations_list_edit_steer_and_remove_through_the_cards_actions(cx: &mut TestAppContext) {
    let (window, _, id) = two_sessions(&[Scope::Read, Scope::Dispatch], cx);
    let ids = queue(window, id, cx);
    assert_eq!(ids.len(), 2);
    let edited: QueueOutput = run(
        window,
        Command::QueueEdit(QueueEditInput {
            session_id: id,
            message_id: ids[1].clone(),
            text: "second, shorter".into(),
        }),
        cx,
    )
    .unwrap();
    assert_eq!(edited.queue_length, 2);
    let steered: QueueOutput = run(window, Command::QueueSteer(item(id, &ids[1])), cx).unwrap();
    assert_eq!(steered.queue_length, 2);
    let list: QueueListOutput = run(
        window,
        Command::QueueList(SessionInput { session_id: id }),
        cx,
    )
    .unwrap();
    let shown: Vec<_> = list
        .messages
        .iter()
        .map(|m| (m.text.as_str(), m.steer))
        .collect();
    assert_eq!(shown, [("first", false), ("second, shorter", true)]);
    // Removing discards the person's text: destructive, and confirmed.
    approve(
        window,
        &[(CLIENT, &[Scope::Read, Scope::Dispatch, Scope::Destructive])],
        cx,
    );
    let (reply, answer) = std::sync::mpsc::sync_channel(1);
    window
        .update(cx, |shell, window, cx| {
            let command = Command::QueueRemove(item(id, &ids[0]));
            let request = Request::Call {
                caller: CLIENT.into(),
                request_id: None,
                command,
                reply,
            };
            shell.handle_control(request, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    cx.simulate_prompt_answer("Remove");
    cx.run_until_parked();
    let removed: QueueOutput = serde_json::from_value(answer.try_recv().unwrap().unwrap()).unwrap();
    assert_eq!(removed.queue_length, 1);
    let missing = run::<QueueOutput>(window, Command::QueueRemove(item(id, &ids[0])), cx);
    assert_eq!(missing.unwrap_err().code, ErrorCode::NotFound);
    window
        .update(cx, |shell, _, cx| {
            let saved = locked(&shell.store).unwrap().follow_ups(id).unwrap();
            assert_eq!(saved.messages.len(), 1, "the queue is saved");
            assert_eq!(saved.messages[0].text, "second, shorter");
            let _ = cx;
        })
        .unwrap();
}

#[gpui::test]
fn queue_resume_releases_a_paused_queue_and_is_a_no_op_otherwise(cx: &mut TestAppContext) {
    let (window, _, id) = two_sessions(&[Scope::Read, Scope::Dispatch], cx);
    let _ = queue(window, id, cx);
    let running: QueueOutput = run(
        window,
        Command::QueueResume(SessionInput { session_id: id }),
        cx,
    )
    .unwrap();
    assert!(!running.paused, "nothing to resume while it runs");
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&id].update(cx, |view, cx| view.test_pause_queue(cx));
        })
        .unwrap();
    let list: QueueListOutput = run(
        window,
        Command::QueueList(SessionInput { session_id: id }),
        cx,
    )
    .unwrap();
    assert!(list.paused);
    let resumed: QueueOutput = run(
        window,
        Command::QueueResume(SessionInput { session_id: id }),
        cx,
    )
    .unwrap();
    assert_eq!(
        resumed.queue_length, 2,
        "the messages wait for the continuation"
    );
}
