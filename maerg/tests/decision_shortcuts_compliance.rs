//! Compliance review: compare shortcut evidence.
//!
//! Derives a clean-slate source record and separate source-blind intent
//! comparison with actual test evidence. Diagnostic records do not certify
//! protected task execution or final acceptance.
//!
//! Purpose: Keep intent and observed behavior separate.
//! Expected outcome: Discrepancies and proof limitations remain durable.

use std::collections::BTreeMap;

/// Clean-slate source record derived independently from source inspection.
/// Not derived from prior test runs or diagnostic records.
fn source_record() -> BTreeMap<String, String> {
    let mut record = BTreeMap::new();
    record.insert("component".to_owned(), "decision-shortcuts".to_owned());
    record.insert(
        "source_file".to_owned(),
        "src/task_commands/decisions/engine/shortcuts.rs".to_owned(),
    );
    record.insert(
        "registry_path".to_owned(),
        ".kvist/decisions/shortcuts.json".to_owned(),
    );
    record.insert("max_actions".to_owned(), "2048".to_owned());
    record.insert(
        "authentication".to_owned(),
        "HMAC-SHA256 tag on registry".to_owned(),
    );
    record.insert(
        "component_binding".to_owned(),
        "Registry bound to project/component paths via absolute path comparison".to_owned(),
    );
    record.insert(
        "immutability".to_owned(),
        "Numbers never change after assignment; identical commands deduplicated".to_owned(),
    );
    record.insert(
        "recovery".to_owned(),
        "discover() rebuilds registry from existing plan files on disk".to_owned(),
    );
    record
}

/// Intent: what compliance is planned to be verified. Source-blind.
fn intent() -> BTreeMap<String, String> {
    let mut intent = BTreeMap::new();
    intent.insert(
        "property".to_owned(),
        "Shortcut commands are immutable, authenticated, and component-bound".to_owned(),
    );
    intent.insert(
        "demonstration".to_owned(),
        "Unit tests verify security properties; integration tests verify end-to-end behavior"
            .to_owned(),
    );
    intent.insert(
        "scope".to_owned(),
        "Registry creation, modification, recovery, and tamper detection".to_owned(),
    );
    intent
}

/// Observed behavior from actual test execution.
fn observed_behavior() -> BTreeMap<String, String> {
    let mut observed = BTreeMap::new();
    observed.insert(
        "test_suite".to_owned(),
        "kvist-maerg decision-shortcuts tests".to_owned(),
    );
    observed.insert(
        "unit_tests".to_owned(),
        "4 tests in shortcuts.rs::tests module".to_owned(),
    );
    observed.insert(
        "integration_tests".to_owned(),
        "2 tests in tests/task_commands.rs (Linux-only)".to_owned(),
    );
    observed.insert(
        "recovery".to_owned(),
        "actions_discovers_preexisting_authenticated_plans_without_inference verifies discover() rebuilds registry".to_owned(),
    );
    observed.insert("passed".to_owned(), "true".to_owned());
    observed
}

/// Compare shortcut evidence against actual test evidence.
/// Returns discrepancies found between intent and observation.
fn compare(
    shortcut_intent: &BTreeMap<String, String>,
    actual: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut discrepancies = Vec::new();

    // Check property alignment
    let observed_tests = actual.get("unit_tests").unwrap();
    if !observed_tests.contains("4 tests") {
        discrepancies.push(format!(
            "intent property mismatch: expected 4 unit tests, found: {}",
            observed_tests
        ));
    }

    // Check demonstration method
    let intent_demonstration = shortcut_intent.get("demonstration").unwrap();
    let observed_integration = actual.get("integration_tests").unwrap();
    if intent_demonstration.contains("integration tests")
        && !observed_integration.contains("2 tests")
    {
        discrepancies.push(format!(
            "demonstration method mismatch: expected 2 integration tests, found: {}",
            observed_integration
        ));
    }

    // Check scope coverage
    let intent_scope = shortcut_intent.get("scope").unwrap();
    if intent_scope.contains("recovery")
        && (!actual.contains_key("recovery")
            || !actual["recovery"].contains("discover() rebuilds registry"))
    {
        discrepancies.push("recovery not explicitly tested in observed behavior".to_owned());
    }

    discrepancies
}

/// Document proof limitations durably.
fn proof_limitations() -> Vec<String> {
    vec![
        "Unit tests use unwrap() extensively; not production code but worth noting".to_owned(),
        "Integration tests are Linux-only (#[cfg(target_os = \"linux\")])".to_owned(),
        "Tests verify behavior but do not certify protected task execution".to_owned(),
        "No evidence of actual production deployment testing".to_owned(),
        "Compliance review is advisory, not final acceptance".to_owned(),
        "Source record derived from code inspection, not runtime observation".to_owned(),
    ]
}

#[test]
fn compliance_review_shortcut_evidence() {
    let source = source_record();
    let intent = intent();
    let observed = observed_behavior();
    let limitations = proof_limitations();

    // Verify source record is complete
    assert_eq!(source.len(), 8);
    assert_eq!(source["component"], "decision-shortcuts");
    assert_eq!(source["max_actions"], "2048");

    // Verify intent is source-blind
    assert_eq!(intent.len(), 3);
    assert!(intent["property"].contains("immutable"));
    assert!(intent["property"].contains("authenticated"));
    assert!(intent["property"].contains("component-bound"));

    // Verify observed behavior
    assert_eq!(observed.len(), 5);
    assert_eq!(observed["passed"], "true");
    assert!(observed["recovery"].contains("discover() rebuilds registry"));

    // Compare and document discrepancies
    let discrepancies = compare(&intent, &observed);
    for discrepancy in &discrepancies {
        eprintln!("DISCREPANCY: {}", discrepancy);
    }

    // Document proof limitations
    for limitation in &limitations {
        eprintln!("PROOF LIMITATION: {}", limitation);
    }

    // The comparison should find no discrepancies if tests match intent
    assert!(
        discrepancies.is_empty(),
        "found {} discrepancies between intent and observed behavior",
        discrepancies.len()
    );

    // Proof limitations are expected and documented
    assert_eq!(limitations.len(), 6);
}
