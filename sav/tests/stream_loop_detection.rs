use sav::loop_detection::{RecoveryBudget, RepetitionChannel, RepetitionUnit, StreamLoopDetector};
use sav::{FinishReason, LocalModelProvider, ModelStreamEvent, ModelTurn, ToolIntent};

fn turn(text: &str, length: bool) -> ModelTurn {
    ModelTurn {
        text: text.into(),
        reasoning: None,
        tool_intents: vec![],
        finish_reason: if length {
            FinishReason::Length
        } else {
            FinishReason::Stop
        },
        provider: LocalModelProvider::LlamaServer,
        model: "fixture".into(),
        response_id: None,
        provider_request_id: None,
        usage: None,
    }
}

fn near_passages(changes: usize, numeric: bool) -> String {
    (0..4)
        .map(|copy| {
            (0..100)
                .map(|word| {
                    if (40..40 + changes).contains(&word) {
                        if numeric {
                            format!("version{copy} ")
                        } else {
                            format!("{} ", ["alpha", "beta", "gamma", "delta"][copy])
                        }
                    } else {
                        format!("term{word} ")
                    }
                })
                .collect::<String>()
        })
        .collect()
}

#[test]
fn exact_long_periods_report_measurable_words_and_channels_across_unicode_chunks() {
    for period in [100, 150, 200, 512] {
        let motif = (0..period)
            .map(|n| format!("éclair{n} "))
            .collect::<String>();
        for reasoning in [false, true] {
            let mut detector = StreamLoopDetector::default();
            let report = motif
                .repeat(4)
                .chars()
                .find_map(|ch| {
                    detector.observe(&if reasoning {
                        ModelStreamEvent::ReasoningDelta(ch.to_string())
                    } else {
                        ModelStreamEvent::TextDelta(ch.to_string())
                    })
                })
                .expect("three substantial periods");
            assert_eq!(report.period, period);
            assert_eq!(report.copies, 3);
            assert_eq!(report.similarity_percent, 100);
            assert_eq!(report.unit, RepetitionUnit::Words);
            assert_eq!(
                report.channel,
                if reasoning {
                    RepetitionChannel::Reasoning
                } else {
                    RepetitionChannel::Answer
                }
            );
        }
    }
}

#[test]
fn terminal_flush_detects_exactly_three_copies_without_trailing_whitespace() {
    let motif = (0..100).map(|n| format!("word{n} ")).collect::<String>();
    let text = motif.repeat(3).trim_end().to_string();
    let mut detector = StreamLoopDetector::default();
    assert!(
        detector
            .observe(&ModelStreamEvent::TextDelta(text.clone()))
            .is_none()
    );
    let report = detector.finish(&turn(&text, false)).unwrap();
    assert_eq!((report.period, report.copies), (100, 3));
}

#[test]
fn unary_two_copy_length_warning_is_prose_only() {
    let motif = (0..200).map(|n| format!("word{n} ")).collect::<String>();
    let text = motif.repeat(2);
    assert!(
        StreamLoopDetector::default()
            .finish(&turn(&text, false))
            .is_none()
    );
    let report = StreamLoopDetector::default()
        .finish(&turn(&text, true))
        .unwrap();
    assert_eq!((report.period, report.copies), (200, 2));
    let code = format!("```text\n{text}\n```");
    assert!(
        StreamLoopDetector::default()
            .finish(&turn(&code, true))
            .is_none()
    );
}

#[test]
fn near_matching_accepts_94_percent_prose_identity_but_not_93_percent() {
    let mut detector = StreamLoopDetector::default();
    let report = detector
        .observe(&ModelStreamEvent::TextDelta(near_passages(6, false)))
        .expect("94-percent prose recurrence");
    assert_eq!((report.period, report.similarity_percent), (100, 94));
    let text = near_passages(7, false);
    let mut detector = StreamLoopDetector::default();
    assert!(
        detector
            .observe(&ModelStreamEvent::TextDelta(text.clone()))
            .is_none()
    );
    assert!(detector.finish(&turn(&text, false)).is_none());
}

#[test]
fn terminal_near_detection_survives_changes_at_every_stream_anchor() {
    let text = (0..3)
        .map(|copy| {
            (0..100)
                .map(|word| {
                    if matches!(word, 0 | 50 | 92) {
                        format!("{} ", ["alpha", "beta", "gamma"][copy])
                    } else {
                        format!("term{word} ")
                    }
                })
                .collect::<String>()
        })
        .collect::<String>();
    let mut detector = StreamLoopDetector::default();
    assert!(
        detector
            .observe(&ModelStreamEvent::TextDelta(text.clone()))
            .is_none()
    );
    let report = detector
        .finish(&turn(&text, false))
        .expect("97-percent terminal recurrence");
    assert_eq!((report.period, report.similarity_percent), (100, 97));
}

#[test]
fn changed_numeric_words_are_not_cosmetic_near_repetition() {
    let text = near_passages(1, true);
    let mut detector = StreamLoopDetector::default();
    assert!(
        detector
            .observe(&ModelStreamEvent::TextDelta(text.clone()))
            .is_none()
    );
    assert!(detector.finish(&turn(&text, false)).is_none());
}

#[test]
fn near_matching_ignores_structured_rows_quotes_and_fences() {
    let near = near_passages(4, false);
    let words = near.split_whitespace().collect::<Vec<_>>();
    for prefix in ["| ", "> ", "# ", "1. ", "- "] {
        let text = words
            .chunks(20)
            .map(|line| format!("{prefix}{}\n", line.join(" ")))
            .collect::<String>();
        let mut detector = StreamLoopDetector::default();
        assert!(
            detector
                .observe(&ModelStreamEvent::TextDelta(text.clone()))
                .is_none(),
            "{prefix}"
        );
        assert!(detector.finish(&turn(&text, false)).is_none(), "{prefix}");
    }
    let code = format!("```text\n{near}\n```");
    assert!(
        StreamLoopDetector::default()
            .finish(&turn(&code, false))
            .is_none()
    );
}

#[test]
fn multiline_quotations_remain_ineligible_for_near_matching() {
    for (open, close) in [("\"", "\""), ("“", "”"), ("‘", "’")] {
        let text = format!(
            "An excerpt follows:\n{open}\n{}\n{close}",
            near_passages(4, false)
        );
        let mut detector = StreamLoopDetector::default();
        assert!(
            detector
                .observe(&ModelStreamEvent::TextDelta(text.clone()))
                .is_none(),
            "{open}"
        );
        assert!(detector.finish(&turn(&text, false)).is_none(), "{open}");
    }
}

#[test]
fn shorter_internal_fences_do_not_reclassify_code_as_prose() {
    let text = format!("````text\n```\n{}\n````", near_passages(4, false));
    let mut detector = StreamLoopDetector::default();
    assert!(
        detector
            .observe(&ModelStreamEvent::TextDelta(text.clone()))
            .is_none()
    );
    assert!(detector.finish(&turn(&text, false)).is_none());
}

#[test]
fn multiline_inline_code_remains_ineligible_for_near_matching() {
    for ticks in ["`", "``"] {
        let text = format!(
            "An excerpt follows:\n{ticks}\n{}\n{ticks}",
            near_passages(4, false)
        );
        let mut detector = StreamLoopDetector::default();
        assert!(
            detector
                .observe(&ModelStreamEvent::TextDelta(text.clone()))
                .is_none(),
            "{ticks}"
        );
        assert!(detector.finish(&turn(&text, false)).is_none(), "{ticks}");
    }
}

#[test]
fn wordless_unicode_repetition_is_detected_without_waiting_for_a_delimiter() {
    let mut detector = StreamLoopDetector::default();
    let report = "呆咕噜"
        .repeat(600)
        .chars()
        .find_map(|ch| detector.observe(&ModelStreamEvent::ReasoningDelta(ch.to_string())))
        .unwrap();
    assert_eq!(report.unit, RepetitionUnit::Characters);
    assert_eq!(report.channel, RepetitionChannel::Reasoning);
    assert_eq!(report.period, 3);
    assert!(report.period * report.copies >= 1024);
}

#[test]
fn long_quoted_code_payloads_are_not_character_prose_loops() {
    for text in [
        format!("`{}`", "abcde".repeat(1000)),
        format!("[{}]", "0,".repeat(2000)),
        format!("<row>{}</row>", "value".repeat(1000)),
    ] {
        let mut detector = StreamLoopDetector::default();
        assert!(
            detector
                .observe(&ModelStreamEvent::TextDelta(text.clone()))
                .is_none()
        );
        assert!(detector.finish(&turn(&text, false)).is_none());
    }
}

#[test]
fn answer_and_reasoning_do_not_combine_into_a_false_three_copy_period() {
    let motif = (0..100).map(|n| format!("word{n} ")).collect::<String>();
    let mut detector = StreamLoopDetector::default();
    assert!(
        detector
            .observe(&ModelStreamEvent::TextDelta(motif.repeat(2)))
            .is_none()
    );
    assert!(
        detector
            .observe(&ModelStreamEvent::ReasoningDelta(motif.repeat(2)))
            .is_none()
    );
}

#[test]
fn bulk_tool_arguments_are_not_prose_and_corrections_are_not_resettable() {
    let mut detector = StreamLoopDetector::default();
    assert!(
        detector
            .observe(&ModelStreamEvent::ToolIntent(ToolIntent {
                id: "proposal".into(),
                provider_id: None,
                name: "write".into(),
                arguments: serde_json::json!({"contents":"repetitive ".repeat(10000)}),
            }))
            .is_none()
    );
    let mut budget = RecoveryBudget::default();
    assert_eq!(budget.take().unwrap().attempt, 1);
    assert_eq!(budget.take().unwrap().attempt, 2);
    for _ in 0..10 {
        assert!(budget.take().is_none());
    }
}
