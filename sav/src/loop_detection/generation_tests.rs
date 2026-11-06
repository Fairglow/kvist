use super::*;

#[test]
fn retained_words_and_unfinished_unicode_tokens_have_exact_fixed_bounds() {
    let mut detector = ChannelDetector::default();
    for word in 0..10_000 {
        assert!(detector.feed(&format!("unique{word} ")).is_none());
    }
    assert_eq!(detector.words.len(), HISTORY);
    assert_eq!(detector.words.capacity(), HISTORY);
    assert!(
        detector
            .feed(&format!("\"{}", "😀".repeat(50_000)))
            .is_none()
    );
    assert_eq!(detector.scalars.len(), HISTORY);
    assert_eq!(detector.scalars.capacity(), HISTORY);
    assert_eq!(detector.prefix.chars().count(), 8);
    assert!(detector.prefix.len() <= 32);
    assert_eq!(detector.word_chars, 50_001);
}

#[test]
fn valid_code_closure_restores_prose_but_nonwhite_closing_lines_do_not() {
    let near = (0..4)
        .map(|copy| {
            (0..100)
                .map(|word| {
                    if word == 40 {
                        format!("{} ", ["alpha", "beta", "gamma", "delta"][copy])
                    } else {
                        format!("term{word} ")
                    }
                })
                .collect::<String>()
        })
        .collect::<String>();
    let mut detector = ChannelDetector::default();
    assert!(
        detector
            .feed("````text\n```\nordinary code\n````\n")
            .is_none()
    );
    assert!(detector.fence.is_none());
    assert!(detector.feed(&near).is_some());
    let mut detector = ChannelDetector::default();
    assert!(detector.feed("````text\n```` still code\n").is_none());
    assert!(detector.fence.is_some());
    assert!(detector.feed(&near).is_none());
}

#[test]
fn control_checks_bound_large_event_work_and_latch_stream_and_unary_interruption() {
    let text = (0..10_000).map(|n| format!("word{n} ")).collect::<String>();
    let mut detector = StreamLoopDetector::default();
    let mut checks = 0;
    assert!(
        detector
            .observe_while(&ModelStreamEvent::TextDelta(text.clone()), &mut || {
                checks += 1;
                checks <= 2
            })
            .is_none()
    );
    assert_eq!(checks, 3);
    assert!(detector.interrupted);
    assert!(detector.answer.words_seen <= 256);
    assert!(
        detector
            .observe(&ModelStreamEvent::TextDelta("loop ".repeat(100)))
            .is_none()
    );

    let mut detector = StreamLoopDetector::default();
    let mut turn = ModelTurn {
        text,
        reasoning: None,
        tool_intents: vec![],
        finish_reason: FinishReason::Stop,
        provider: crate::LocalModelProvider::LlamaServer,
        model: "fixture".into(),
        response_id: None,
        provider_request_id: None,
        usage: None,
    };
    let mut checks = 0;
    assert!(
        detector
            .finish_while(&turn, &mut || {
                checks += 1;
                checks <= 2
            })
            .is_none()
    );
    assert!(detector.interrupted);
    turn.text = "loop ".repeat(100);
    assert!(detector.finish(&turn).is_none());
}
