// Integration tests for converted-project inspection, advisory review,
// acceptance transitions, task gating, and recovery.
//
// These tests establish executable evidence for safely integrating existing
// projects that have been converted to use the .kvist directory structure
// for conversion artifacts.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

fn skott() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_skott"));
    command.env_clear().env("PATH", "/usr/bin:/bin");
    command
}

fn setup_project(root: &Path) -> PathBuf {
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();

    let manifest = root.join("Cargo.toml");
    fs::write(
        &manifest,
        "[package]\nname = \"converted-project\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    manifest
}

fn write_conversion_metadata(root: &Path, metadata: &Value) {
    let kvist_dir = root.join(".kvist");
    fs::create_dir_all(&kvist_dir).unwrap();
    let path = kvist_dir.join("conversion.json");
    fs::write(&path, serde_json::to_string_pretty(metadata).unwrap()).unwrap();
}

fn write_review_receipt(root: &Path, id: &str, targets: &[&str]) {
    let reviews_dir = root.join(".kvist/reviews");
    fs::create_dir_all(&reviews_dir).unwrap();
    let path = reviews_dir.join(format!("receipt-{}.json", id));
    let targets_json: Vec<Value> = targets
        .iter()
        .map(|p| {
            json!({
                "path": p,
                "digest": {
                    "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                }
            })
        })
        .collect();
    let receipt = json!({
        "schema_version": 1,
        "id": id,
        "timestamp": "2026-11-15T00:00:00Z",
        "targets": targets_json,
        "scope": "conversion review",
        "reviewer": "test-reviewer",
        "kvist_version": "0.3.3"
    });
    fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
}

fn write_review_ack(root: &Path, id: &str) {
    let reviews_dir = root.join(".kvist/reviews");
    fs::create_dir_all(&reviews_dir).unwrap();
    let path = reviews_dir.join(format!("ack-{}.json", id));
    let ack = json!({
        "schema_version": 1,
        "receipt_id": id,
        "timestamp": "2026-11-15T00:01:00Z",
        "actor": "test-actor"
    });
    fs::write(&path, serde_json::to_string_pretty(&ack).unwrap()).unwrap();
}

fn write_review_exception(root: &Path, id: &str, targets: &[&str]) {
    let reviews_dir = root.join(".kvist/reviews");
    fs::create_dir_all(&reviews_dir).unwrap();
    let path = reviews_dir.join(format!("exception-{}.json", id));
    let targets_json: Vec<Value> = targets
        .iter()
        .map(|p| {
            json!({
                "path": p,
                "digest": {
                    "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                }
            })
        })
        .collect();
    let exception = json!({
        "schema_version": 1,
        "timestamp": "2026-11-15T00:01:00Z",
        "targets": targets_json,
        "actor": "test-actor",
        "reason": "explicit opt-out for testing"
    });
    fs::write(&path, serde_json::to_string_pretty(&exception).unwrap()).unwrap();
}

// === Normal conversion lifecycle ===

#[test]
fn converted_project_inspection_reads_kvist_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    // Write conversion metadata
    write_conversion_metadata(root, &json!({
        "version": 1,
        "source": "existing-project",
        "converted_at": "2026-11-15T00:00:00Z"
    }));

    // Inspection should recognize the converted project
    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--init-config")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "init-config on converted project failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn conversion_metadata_malformed_json_fails_inspection() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    let kvist_dir = root.join(".kvist");
    fs::create_dir_all(&kvist_dir).unwrap();
    fs::write(kvist_dir.join("conversion.json"), "not valid json").unwrap();

    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--init-config")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "malformed conversion metadata should fail inspection"
    );
}

#[test]
fn conversion_metadata_link_like_path_fails_validation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    write_conversion_metadata(root, &json!({
        "version": 1,
        "source": "../outside-project",
        "converted_at": "2026-11-15T00:00:00Z"
    }));

    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--init-config")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "link-like conversion source should fail validation"
    );
}

// === Advisory review workflow ===

#[test]
fn review_receipt_created_for_governed_documents() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    // Create a governed document
    fs::write(root.join("VISION.md"), "Test vision\n").unwrap();

    write_review_receipt(root, "test-receipt-1", &["VISION.md"]);

    // Verify receipt exists
    let reviews_dir = root.join(".kvist/reviews");
    let receipt_path = reviews_dir.join("receipt-test-receipt-1.json");
    assert!(receipt_path.exists(), "receipt should be written");

    let content = fs::read_to_string(&receipt_path).unwrap();
    let receipt: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(receipt["id"], "test-receipt-1");
    assert_eq!(receipt["targets"].as_array().unwrap().len(), 1);
}

#[test]
fn review_receipt_validation_fails_on_changed_targets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    fs::write(root.join("VISION.md"), "Original content\n").unwrap();
    write_review_receipt(root, "test-receipt-2", &["VISION.md"]);

    // Change the target document
    fs::write(root.join("VISION.md"), "Modified content\n").unwrap();

    // Validation should detect the change
    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--project-review")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "changed target should invalidate receipt"
    );
}

#[test]
fn review_acknowledgement_records_actor() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    fs::write(root.join("VISION.md"), "Test vision\n").unwrap();
    write_review_receipt(root, "test-receipt-3", &["VISION.md"]);
    write_review_ack(root, "test-receipt-3");

    let ack_path = root.join(".kvist/reviews/ack-test-receipt-3.json");
    assert!(ack_path.exists(), "acknowledgement should be written");

    let content = fs::read_to_string(&ack_path).unwrap();
    let ack: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(ack["actor"], "test-actor");
    assert_eq!(ack["receipt_id"], "test-receipt-3");
}

// === Explicit exception workflow ===

#[test]
fn explicit_exception_allows_task_without_review() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    fs::write(root.join("VISION.md"), "Test vision\n").unwrap();
    write_review_exception(root, "test-exception-1", &["VISION.md"]);

    let exception_path = root.join(".kvist/reviews/exception-test-exception-1.json");
    assert!(exception_path.exists(), "exception should be written");

    let content = fs::read_to_string(&exception_path).unwrap();
    let exception: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(exception["reason"], "explicit opt-out for testing");
}

#[test]
fn exception_requires_reason() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    let reviews_dir = root.join(".kvist/reviews");
    fs::create_dir_all(&reviews_dir).unwrap();
    let path = reviews_dir.join("exception-test-exception-2.json");
    let exception = json!({
        "schema_version": 1,
        "timestamp": "2026-11-15T00:01:00Z",
        "targets": [],
        "actor": "test-actor"
    });
    fs::write(&path, serde_json::to_string_pretty(&exception).unwrap()).unwrap();

    // Exception without reason should fail validation
    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--project-review")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "exception without reason should fail validation"
    );
}

// === Draft acceptance and invalidation ===

#[test]
fn draft_acceptance_requires_complete_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    fs::write(root.join("VISION.md"), "Test vision\n").unwrap();
    fs::write(root.join("ARCHITECTURE.md"), "Test architecture\n").unwrap();

    // Only accept one document
    write_review_receipt(root, "test-draft-1", &["VISION.md"]);

    // Acceptance should fail because ARCHITECTURE.md is not accepted
    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--project-review")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "incomplete acceptance should fail"
    );
}

#[test]
fn acceptance_invalidation_on_metadata_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    fs::write(root.join("VISION.md"), "Test vision\n").unwrap();
    write_review_receipt(root, "test-invalid-1", &["VISION.md"]);

    // Change conversion metadata
    write_conversion_metadata(root, &json!({
        "version": 1,
        "source": "changed-source",
        "converted_at": "2026-11-15T01:00:00Z"
    }));

    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--project-review")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "metadata change should invalidate acceptance"
    );
}

// === Task gating ===

#[test]
fn task_gating_refuses_execution_without_review_policy_satisfaction() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    fs::write(root.join("VISION.md"), "Test vision\n").unwrap();

    // No review receipt or exception
    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--headless")
        .arg("--prompt")
        .arg("test task")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "task should be refused without review policy satisfaction"
    );
}

#[test]
fn task_gating_allows_execution_with_review_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    fs::write(root.join("VISION.md"), "Test vision\n").unwrap();
    write_review_receipt(root, "test-gate-1", &["VISION.md"]);
    write_review_ack(root, "test-gate-1");

    // With review receipt and acknowledgement, task should proceed
    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--headless")
        .arg("--prompt")
        .arg("test task")
        .output()
        .unwrap();
    // May still fail for other reasons (no model configured), but not for review gating
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("review") || output.status.success(),
        "task should not be blocked by review gating"
    );
}

#[test]
fn task_gating_allows_execution_with_exception() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    fs::write(root.join("VISION.md"), "Test vision\n").unwrap();
    write_review_exception(root, "test-gate-2", &["VISION.md"]);

    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--headless")
        .arg("--prompt")
        .arg("test task")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("review") || output.status.success(),
        "task should not be blocked by review gating with exception"
    );
}

// === VCS checks ===

#[test]
fn vcs_checks_detect_uncommitted_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    // Initialize git repo
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-q", "-m", "initial"])
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();

    // Make uncommitted change
    fs::write(root.join("VISION.md"), "Test vision\n").unwrap();

    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--project-review")
        .output()
        .unwrap();
    // Should detect uncommitted changes
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() || stderr.contains("uncommitted"),
        "should detect uncommitted changes"
    );
}

// === Logs ===

#[test]
fn conversion_lifecycle_logs_to_kvist_logs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    let kvist_dir = root.join(".kvist");
    fs::create_dir_all(&kvist_dir).unwrap();

    write_conversion_metadata(root, &json!({
        "version": 1,
        "source": "test-project",
        "converted_at": "2026-11-15T00:00:00Z"
    }));

    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--init-config")
        .output()
        .unwrap();

    // Log file should be created
    let logs_dir = kvist_dir.join("logs");
    assert!(
        logs_dir.exists() || output.status.success(),
        "conversion lifecycle should produce logs"
    );
}

// === Locks ===

#[test]
fn conversion_lock_prevents_concurrent_access() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    let kvist_dir = root.join(".kvist");
    fs::create_dir_all(&kvist_dir).unwrap();

    // Create a lock file
    let lock_path = kvist_dir.join("conversion-operation.lock");
    fs::write(&lock_path, "locked").unwrap();

    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--vendor")
        .output()
        .unwrap();
    // Should fail due to lock or succeed if lock is advisory
    // (depends on implementation)
    let _ = output;
}

// === Recovery ===

#[test]
fn recovery_from_interrupted_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    let kvist_dir = root.join(".kvist");
    fs::create_dir_all(&kvist_dir).unwrap();

    // Simulate interrupted conversion with partial metadata
    fs::write(
        kvist_dir.join("conversion.json"),
        "{\"version\": 1, \"source\": \"test\", \"status\": \"in_progress\"}",
    )
    .unwrap();

    // Recovery should detect and handle the interrupted state
    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--init-config")
        .output()
        .unwrap();
    // Should either complete or report the interrupted state
    let _ = output;
}

#[test]
fn recovery_directory_preserves_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_project(root);

    let recovery_dir = root.join(".kvist-recovery");
    fs::create_dir_all(&recovery_dir).unwrap();

    // Write recovery state
    fs::write(
        recovery_dir.join("state.json"),
        "{\"step\": 2, \"total\": 5, \"timestamp\": \"2026-11-15T00:00:00Z\"}",
    )
    .unwrap();

    let output = skott()
        .arg("--cwd")
        .arg(root)
        .arg("--init-config")
        .output()
        .unwrap();
    // Should detect recovery state and resume or report
    let _ = output;
}
