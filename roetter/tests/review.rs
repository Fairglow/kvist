//! Advisory document review tests.
//!
//! These tests define the expected behavior of the review subsystem:
//! exact-digest review bundles, bounded redacted reports, explicit
//! acknowledgement, per-bundle exceptions, default-on configuration with
//! project opt-out, unavailable agents, canonical task projections,
//! governed project paths, and stale review evidence.

use roetter::review::*;

fn test_project() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(".roetter-review-test-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap()
}

// --- Exact-digest review bundles ---

#[test]
fn review_bundle_requires_exact_digest() {
    let project = test_project();
    let doc = b"important advisory document";
    let digest = document_digest(doc);

    // Bundle with correct digest should be accepted
    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: Some("claude".to_string()),
        report: "All clear.".to_string(),
    };
    let result = register_review(project.path(), bundle);
    assert!(
        result.is_ok(),
        "bundle with exact digest should be accepted"
    );

    // Bundle with wrong digest should be rejected
    let bad_bundle = ReviewBundle {
        document_digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        document: doc.to_vec(),
        agent: Some("claude".to_string()),
        report: "All clear.".to_string(),
    };
    let bad_result = register_review(project.path(), bad_bundle);
    assert!(
        bad_result.is_err(),
        "bundle with wrong digest should be rejected"
    );
}

#[test]
fn document_digest_is_deterministic() {
    let doc = b"test document content";
    let d1 = document_digest(doc);
    let d2 = document_digest(doc);
    assert_eq!(d1, d2, "digest must be deterministic");
    assert!(d1.starts_with("sha256:"), "digest must be sha256-prefixed");

    let other = b"different content";
    let d3 = document_digest(other);
    assert_ne!(d1, d3, "different content must produce different digest");
}

// --- Bounded redacted reports ---

#[test]
fn review_report_is_bounded() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    // Reasonable report should be accepted
    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: Some("test-agent".to_string()),
        report: "This is a reasonable review report.".to_string(),
    };
    assert!(register_review(project.path(), bundle).is_ok());

    // Oversized report should be rejected
    let big_bundle = ReviewBundle {
        document_digest: digest,
        document: doc.to_vec(),
        agent: Some("test-agent".to_string()),
        report: "x".repeat(100_000),
    };
    assert!(
        register_review(project.path(), big_bundle).is_err(),
        "oversized report should be rejected"
    );
}

#[test]
fn review_report_is_redacted_for_storage() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: Some("claude-sonnet".to_string()),
        report: "Found issues: path /workspace/src/main.rs line 42".to_string(),
    };
    register_review(project.path(), bundle).unwrap();

    // Stored report should not contain project paths
    let stored = load_review(project.path(), digest).unwrap();
    assert!(
        !stored.report.contains("/workspace"),
        "stored report should be redacted of project paths"
    );
}

// --- Explicit acknowledgement ---

#[test]
fn review_requires_explicit_acknowledgement() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    // Register review but don't acknowledge
    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: Some("test-agent".to_string()),
        report: "Review complete.".to_string(),
    };
    register_review(project.path(), bundle).unwrap();

    // Should not be considered acknowledged yet
    let status = review_status(project.path(), digest.clone());
    assert_eq!(
        status,
        ReviewStatus::Registered,
        "review should be Registered before acknowledgement"
    );

    // Acknowledge explicitly
    acknowledge_review(project.path(), digest.clone(), "ack by human").unwrap();

    // Now should be acknowledged
    let status = review_status(project.path(), digest);
    assert_eq!(
        status,
        ReviewStatus::Acknowledged,
        "review should be Acknowledged after explicit acknowledgement"
    );
}

#[test]
fn acknowledgement_requires_identity() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: Some("test-agent".to_string()),
        report: "Review complete.".to_string(),
    };
    register_review(project.path(), bundle).unwrap();

    // Empty identity should fail
    let result = acknowledge_review(project.path(), digest, "");
    assert!(
        result.is_err(),
        "empty acknowledgement identity should fail"
    );
}

// --- Per-bundle exceptions ---

#[test]
fn per_bundle_exception_can_override_review_requirement() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    // Without exception, review is required
    let config = ReviewConfig::default();
    assert!(config.review_required(project.path()));

    // With exception, review is not required for this bundle
    add_review_exception(project.path(), digest.clone(), "known safe").unwrap();
    let status = review_status(project.path(), digest);
    assert_eq!(
        status,
        ReviewStatus::Excepted,
        "excepted bundle should show Excepted status"
    );
}

#[test]
fn exception_is_digest_specific() {
    let project = test_project();
    let doc1 = b"document one";
    let doc2 = b"document two";
    let digest1 = document_digest(doc1);
    let digest2 = document_digest(doc2);

    add_review_exception(project.path(), digest1, "only for doc1").unwrap();

    let status1 = review_status(project.path(), document_digest(doc1));
    let status2 = review_status(project.path(), digest2);

    assert_eq!(status1, ReviewStatus::Excepted);
    assert_ne!(
        status2,
        ReviewStatus::Excepted,
        "exception should not apply to other documents"
    );
}

// --- Default-on configuration and project opt-out ---

#[test]
fn review_is_default_on() {
    let project = test_project();
    let config = ReviewConfig::default();
    assert!(
        config.review_required(project.path()),
        "review should be required by default"
    );
}

#[test]
fn project_can_opt_out_of_review() {
    let project = test_project();
    let mut config = ReviewConfig::default();

    // Initially required
    assert!(config.review_required(project.path()));

    // Opt out
    config.opt_out_project(project.path());

    // Now not required
    assert!(
        !config.review_required(project.path()),
        "project should not require review after opt-out"
    );
}

#[test]
fn opt_out_requires_persistent_configuration() {
    let project = test_project();
    let mut config = ReviewConfig::default();
    config.opt_out_project(project.path());

    // Save and reload
    config.save(project.path()).unwrap();
    let reloaded = ReviewConfig::load(project.path()).unwrap();

    assert!(
        !reloaded.review_required(project.path()),
        "opt-out should persist across reloads"
    );
}

// --- Unavailable agents ---

#[test]
fn review_can_be_registered_without_agent() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    // No agent available - human reviewed directly
    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: None,
        report: "Human reviewed without agent assistance.".to_string(),
    };
    let result = register_review(project.path(), bundle);
    assert!(
        result.is_ok(),
        "review should be registrable without an agent"
    );
}

#[test]
fn agent_name_is_optional_metadata() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: Some("claude".to_string()),
        report: "Agent assisted review.".to_string(),
    };
    register_review(project.path(), bundle).unwrap();

    let stored = load_review(project.path(), digest).unwrap();
    assert_eq!(stored.agent, Some("claude".to_string()));
}

// --- Canonical task projections ---

#[test]
fn review_evidence_projects_to_task() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: Some("test".to_string()),
        report: "Review complete.".to_string(),
    };
    register_review(project.path(), bundle).unwrap();
    acknowledge_review(project.path(), digest.clone(), "human").unwrap();

    // Query task projection
    let projection = review_task_projection(project.path(), digest);
    assert!(
        projection.is_some(),
        "acknowledged review should project to task"
    );
    let proj = projection.unwrap();
    assert_eq!(proj.status, "acknowledged");
}

#[test]
fn unacknowledged_review_projects_as_pending() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: Some("test".to_string()),
        report: "Review complete.".to_string(),
    };
    register_review(project.path(), bundle).unwrap();

    let projection = review_task_projection(project.path(), digest);
    assert!(projection.is_some());
    let proj = projection.unwrap();
    assert_eq!(proj.status, "pending_acknowledgement");
}

// --- Governed project paths ---

#[test]
fn review_paths_are_governed_by_project() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: Some("test".to_string()),
        report: "Review complete.".to_string(),
    };
    register_review(project.path(), bundle).unwrap();

    // Review data should be stored under .kvist/review
    let review_dir = project.path().join(".kvist/review");
    assert!(
        review_dir.exists(),
        "review data should be stored under governed .kvist/review path"
    );
}

// --- Stale review evidence ---

#[test]
fn review_becomes_stale_when_document_changes() {
    let project = test_project();
    let doc_v1 = b"advisory version 1";
    let doc_v2 = b"advisory version 2";
    let digest_v1 = document_digest(doc_v1);

    let bundle = ReviewBundle {
        document_digest: digest_v1.clone(),
        document: doc_v1.to_vec(),
        agent: Some("test".to_string()),
        report: "Reviewed v1.".to_string(),
    };
    register_review(project.path(), bundle).unwrap();
    acknowledge_review(project.path(), digest_v1.clone(), "human").unwrap();

    // Review is current for v1
    let status_v1 = review_status(project.path(), digest_v1.clone());
    assert_eq!(status_v1, ReviewStatus::Acknowledged);

    // Register review for v2
    let digest_v2 = document_digest(doc_v2);
    let bundle_v2 = ReviewBundle {
        document_digest: digest_v2.clone(),
        document: doc_v2.to_vec(),
        agent: Some("test".to_string()),
        report: "Reviewed v2.".to_string(),
    };
    register_review(project.path(), bundle_v2).unwrap();

    // v1 review is now stale (superseded)
    let stale_status = review_status(project.path(), digest_v1);
    assert_eq!(
        stale_status,
        ReviewStatus::Stale,
        "previous review should become stale when new version is reviewed"
    );
}

// --- Review evidence vs findings ---

#[test]
fn review_evidence_is_distinct_from_findings() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: Some("test".to_string()),
        report: "Found: typo on line 5. Suggestion: fix it.".to_string(),
    };
    register_review(project.path(), bundle).unwrap();

    let review = load_review(project.path(), digest).unwrap();
    // Report contains findings but review status is about evidence
    assert_eq!(review.status, ReviewStatus::Registered);
    assert!(!review.report.is_empty());
}

// --- No veto authority ---

#[test]
fn review_findings_do_not_block_acceptance() {
    let project = test_project();
    let doc = b"advisory";
    let digest = document_digest(doc);

    let bundle = ReviewBundle {
        document_digest: digest.clone(),
        document: doc.to_vec(),
        agent: Some("test".to_string()),
        report: "Major issues found! Should not accept.".to_string(),
    };
    register_review(project.path(), bundle).unwrap();

    // Human can still acknowledge despite negative findings
    let result = acknowledge_review(project.path(), digest, "human override");
    assert!(
        result.is_ok(),
        "human acknowledgement should succeed regardless of agent findings"
    );
}
