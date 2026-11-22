//! Compliance review: attention briefs compared with intent.
//!
//! Durable diagnostic record: `tests/compliance_review_attention_brief.md`,
//! the independent comparison of `REQ-DECISION-SURFACING`, the contract's
//! "Human attention briefs" clause, and the design's "Human decision and
//! blocker presentation" section against retained source-only observations,
//! the current attention presentation layer, and bounded test evidence.
//!
//! These tests pin the record's diagnostic substance and check that its
//! test-evidence citations name functions that exist. They read the record
//! and test source only; they do not execute a task run and do not certify
//! the implementation, accept intent, or mint a protected review receipt.

use std::fs;
use std::path::PathBuf;

fn component_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn attention_record() -> String {
    fs::read_to_string(component_dir().join("tests/compliance_review_attention_brief.md"))
        .expect("read the attention-brief compliance review record")
}

fn attention_brief_test_source() -> String {
    fs::read_to_string(component_dir().join("tests/task_commands.rs"))
        .expect("read the task-command test source")
}

/// The record exists, identifies its chain stage, and preserves the two-phase
/// observation/comparison structure required by the task.
#[test]
fn attention_brief_compliance_record_is_present_and_two_phase() {
    let record = attention_record();

    assert!(
        record.starts_with("<!-- kvist-compliance-review-version: 1 -->"),
        "the compliance record must carry the kvist-compliance-review version marker"
    );
    for marker in [
        "attention-brief-compliance-review",
        "REQ-DECISION-SURFACING",
        "Human attention briefs",
        "Human decision and blocker presentation",
        "Phase A",
        "Phase B",
        "not self-certification",
        "not acceptance",
        "human arbitration",
    ] {
        assert!(
            record.contains(marker),
            "the compliance record must identify and scope: {marker}"
        );
    }

    // The runbook ideal's strict two-context isolation is disclosed, not
    // claimed: the record must state both what is preserved and what is not.
    assert!(
        record.contains("Independence that is preserved"),
        "the record must state which independence is preserved"
    );
    assert!(
        record.contains("Independence that is NOT preserved, disclosed"),
        "the record must disclose which independence is not preserved"
    );
}

/// The diagnostic comparison retains a bounded, classified set of findings
/// whose summary table agrees with the finding sections, and keeps the
/// arbitration points and coverage limits open for the human.
#[test]
fn attention_brief_compliance_comparison_records_coverage_and_limits() {
    let record = attention_record();

    for number in 1..=15 {
        let heading = format!("### F-{number} ");
        assert!(
            record.contains(&heading),
            "the record must retain finding F-{number}"
        );
    }
    let heading = "### F-14 Operational-evidence exclusion from checkpoint ownership — underspecified in the supplied evidence";
    assert!(
        record.contains(heading),
        "the checkpoint-ownership exclusion must be classified underspecified, \
         not silently dropped"
    );

    // The summary table classifies every finding; the only open items are the
    // underspecified exclusion and the retained arbitration points.
    assert!(
        record.contains("**Established mismatches requiring arbitration:** 0"),
        "the record must state its mismatch count explicitly"
    );
    for heading in [
        "## Arbitration points retained for the human",
        "A-1",
        "A-2",
        "A-3",
        "A-4",
        "A-5",
        "## Coverage and limits of this review",
    ] {
        assert!(
            record.contains(heading),
            "the record must retain for human arbitration: {heading:?}"
        );
    }

    // The boundary discipline is retained: no execution result is asserted
    // from the automatically run suite, and the record does not claim to be
    // a protected receipt.
    assert!(
        record.contains("this record asserts no pass/fail count from that run"),
        "the record must not assert execution results it did not perform"
    );
    assert!(
        record.contains("diagnostic comparison, not acceptance"),
        "the record must end without claiming acceptance"
    );
}

/// Every bounded test the record cites as evidence must name a function that
/// actually exists in the cited test source.
#[test]
fn attention_brief_compliance_evidence_citations_exist() {
    let record = attention_record();
    let test_source = attention_brief_test_source();

    for cited in [
        "task_run_refuses_missing_sandbox_before_transition",
        "continuous_run_pauses_on_review_decision_without_running_later_items",
        "tool_only_authoring_decision_has_agent_markdown_and_response_steps",
        "decision_brief_write_failure_retains_the_original_stop_and_unresolved_attempt",
        "continuous_run_retries_failed_checkpoint_without_replaying_completed_tasks",
        "continuous_run_rejects_malformed_reviews_and_forged_ledger_state",
        "independent_review_redacts_escaped_secrets_from_stream_logs_and_reports",
        "task_run_cancels_agent_output_and_persists_redacted_bounded_evidence",
    ] {
        assert!(
            test_source.contains(&format!("fn {cited}")),
            "the record cites test `{cited}`, which must exist in \
             tests/task_commands.rs"
        );
        assert!(
            record.contains(cited),
            "the record must name the bounded evidence it relies on: {cited}"
        );
    }
}
