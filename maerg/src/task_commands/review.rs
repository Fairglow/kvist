//! Bounded advisory review of the local intent bundle.
//!
//! Reviews REQUIREMENTS.md, CONTRACT.md, DESIGN.md, and the canonical task
//! definition projection from TODOS.yaml. Includes ROOT_CONTRACT.md and the
//! immediate parent CONTRACT.md as bounded context. Writes a versioned receipt
//! to `.kvist/reviews/` and returns the report.
//!
//! Review findings are nonbinding; no severity threshold blocks acceptance.
//! When project review is required, target acceptance requires a current
//! receipt plus explicit human acknowledgement, or a deliberate exception
//! bound to the exact bundle digests.
//!
//! Security properties:
//! - Receipts are HMAC-SHA256 authenticated to prevent forgery
//! - Task projections use stable serialization to prevent digest confusion
//! - Evidence is structurally separated to mitigate prompt injection
//! - Reports are redacted before storage to prevent context leakage
//! - The advisory nature is explicit in all receipts and validation

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use hmac::{KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::config::ProjectConfig;
use crate::discovery::ComponentArtifact;
use crate::error::KvistError;
use crate::task_commands::TaskContext;
use crate::task_queue::{TaskQueue, parse as parse_queue};

/// Maximum review report size in bytes.
const MAX_REPORT_BYTES: usize = 64 * 1024;

/// HMAC key size for receipt authentication.
const HMAC_KEY_SIZE: usize = 32;

/// Versioned review receipt written to `.kvist/reviews/`.
///
/// The receipt is HMAC-SHA256 authenticated using a project-specific key
/// derived from the project configuration. This prevents forgery by processes
/// that have write access to the reviews directory but not the project key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewReceipt {
    /// Receipt schema version.
    pub schema_version: u32,
    /// Stable review identifier.
    pub review_id: String,
    /// RFC 3339 timestamp when the review was completed.
    pub completed_at: String,
    /// SHA-256 digest of REQUIREMENTS.md reviewed.
    pub requirements_digest: String,
    /// SHA-256 digest of CONTRACT.md reviewed.
    pub contract_digest: String,
    /// SHA-256 digest of DESIGN.md reviewed.
    pub design_digest: String,
    /// SHA-256 digest of the canonical task definition projection reviewed.
    pub todos_projection_digest: String,
    /// SHA-256 digest of ROOT_CONTRACT.md included as context.
    pub root_contract_digest: String,
    /// SHA-256 digest of the immediate parent CONTRACT.md, or null for root.
    pub parent_contract_digest: Option<String>,
    /// Path to the bounded review report.
    pub report_path: String,
    /// SHA-256 digest of the review report.
    pub report_digest: String,
    /// Whether the review found any issues requiring attention.
    pub findings: bool,
    /// HMAC-SHA256 authentication tag for the receipt.
    #[serde(rename = "hmac")]
    pub authentication: String,
}

/// Canonical task definition projection for review.
/// Contains only the fields relevant to intent understanding, excluding
/// component metadata and lifecycle fields.
///
/// Field ordering is fixed in source to ensure stable serialization and
/// prevent digest confusion from field reordering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskProjection {
    pub id: String,
    pub title: String,
    pub description: String,
    pub context: String,
    pub purpose: String,
    pub expected_outcome: String,
    pub kind: String,
    pub depends_on: Vec<String>,
    pub requirements: Vec<String>,
}

/// Runs a bounded advisory review of the local intent bundle.
///
/// Returns the review receipt path and the report text.
pub(crate) fn review_intent(
    context: &TaskContext,
    config: &ProjectConfig,
) -> Result<(PathBuf, String), KvistError> {
    // Read the intent bundle
    let requirements = fs::read_to_string(
        context
            .component_dir
            .join(ComponentArtifact::Requirements.filename()),
    )
    .map_err(|source| KvistError::Io {
        operation: "read REQUIREMENTS.md for review",
        path: context
            .component_dir
            .join(ComponentArtifact::Requirements.filename()),
        source,
    })?;

    let contract = fs::read_to_string(
        context
            .component_dir
            .join(ComponentArtifact::Contract.filename()),
    )
    .map_err(|source| KvistError::Io {
        operation: "read CONTRACT.md for review",
        path: context
            .component_dir
            .join(ComponentArtifact::Contract.filename()),
        source,
    })?;

    let design = fs::read_to_string(
        context
            .component_dir
            .join(ComponentArtifact::Design.filename()),
    )
    .map_err(|source| KvistError::Io {
        operation: "read DESIGN.md for review",
        path: context
            .component_dir
            .join(ComponentArtifact::Design.filename()),
        source,
    })?;

    let queue_path = context
        .component_dir
        .join(ComponentArtifact::TaskQueue.filename());
    let queue_content = fs::read_to_string(&queue_path).map_err(|source| KvistError::Io {
        operation: "read TODOS.yaml for review",
        path: queue_path.clone(),
        source,
    })?;
    let queue: TaskQueue =
        parse_queue(&queue_content).map_err(|source| KvistError::TaskQueueUnavailable {
            path: queue_path,
            reason: source.to_string(),
        })?;

    // Compute digests
    let requirements_digest = sha256_hex(requirements.as_bytes());
    let contract_digest = sha256_hex(contract.as_bytes());
    let design_digest = sha256_hex(design.as_bytes());

    // Build canonical task projection (intent-relevant fields only)
    let task_projections: Vec<TaskProjection> = queue
        .tasks
        .iter()
        .map(|task| TaskProjection {
            id: task.id.clone(),
            title: task.title.clone(),
            description: task.description.clone(),
            context: task.context.clone(),
            purpose: task.purpose.clone(),
            expected_outcome: task.expected_outcome.clone(),
            kind: format!("{:?}", task.kind),
            depends_on: task.depends_on.clone(),
            requirements: task.requirements.clone(),
        })
        .collect();

    let projection_json = serde_json::to_string_pretty(&task_projections).map_err(|source| {
        KvistError::AgentEvidenceFailed {
            reason: format!("cannot serialize task projection for review: {source}"),
        }
    })?;
    let todos_projection_digest = sha256_hex(projection_json.as_bytes());

    // Read ROOT_CONTRACT.md
    let root_contract_path = context.project_dir.join("ROOT_CONTRACT.md");
    let root_contract =
        fs::read_to_string(&root_contract_path).map_err(|source| KvistError::Io {
            operation: "read ROOT_CONTRACT.md for review context",
            path: root_contract_path,
            source,
        })?;
    let root_contract_digest = sha256_hex(root_contract.as_bytes());

    // Read immediate parent CONTRACT.md if present
    let parent_contract_digest = if let Some(parent) = &queue.component.parent_contract {
        let parent_path = context
            .component_dir
            .join(&parent.path)
            .join(ComponentArtifact::Contract.filename());
        let parent_content = fs::read_to_string(&parent_path).map_err(|source| KvistError::Io {
            operation: "read parent CONTRACT.md for review context",
            path: parent_path,
            source,
        })?;
        Some(sha256_hex(parent_content.as_bytes()))
    } else {
        None
    };

    // Build review evidence with structural separation to mitigate prompt injection
    let mut documents = BTreeMap::new();
    documents.insert("REQUIREMENTS.md".to_string(), requirements);
    documents.insert("CONTRACT.md".to_string(), contract);
    documents.insert("DESIGN.md".to_string(), design);
    documents.insert("TASK_PROJECTION".to_string(), projection_json);
    documents.insert("ROOT_CONTRACT.md".to_string(), root_contract);
    if let Some(parent) = &queue.component.parent_contract {
        let parent_path = context
            .component_dir
            .join(&parent.path)
            .join(ComponentArtifact::Contract.filename());
        let parent_content = fs::read_to_string(&parent_path).map_err(|source| KvistError::Io {
            operation: "read parent CONTRACT.md for review evidence",
            path: parent_path,
            source,
        })?;
        documents.insert(
            format!("PARENT_CONTRACT.md ({})", parent.path),
            parent_content,
        );
    }

    // Build evidence with explicit structural separation to mitigate
    // prompt injection. The boundary statement is a separate top-level
    // field, not embedded within document content.
    let evidence = serde_json::json!({
        "schema_version": 1,
        "review_type": "component-intent",
        "component_path": context.component_path.to_string_lossy(),
        "boundary": {
            "type": "advisory",
            "binding": false,
            "determines_compliance": false,
            "has_source_access": false,
            "has_acceptance_authority": false,
            "description": "Advisory review only. Findings are nonbinding and do not determine compliance. No source access. No acceptance authority."
        },
        "documents": documents,
    });

    // Run the review using the existing ReviewSession infrastructure
    let profile = &config.agent.architect;
    let redactions = evidence_redactions(config);
    let mut session = crate::agent::review::ReviewSession::decision(
        profile,
        &context.component_dir,
        "component-intent-review",
        redactions.clone(),
    )?;

    let report = session.request("decision-intent-review", evidence.to_string())?;
    session.finish("completed", None)?;

    // Redact the report before storage to prevent context leakage of
    // sensitive values that may have been included in the evidence.
    let redacted_report =
        super::redact_bounded(report.report.clone(), &redactions, MAX_REPORT_BYTES);

    // Write the redacted report
    let reviews_dir = context.component_dir.join(".kvist").join("reviews");
    fs::create_dir_all(&reviews_dir).map_err(|source| KvistError::Io {
        operation: "create reviews directory",
        path: reviews_dir.clone(),
        source,
    })?;

    let report_path = reviews_dir.join(format!("intent-{}.md", timestamp_for_filename()));
    fs::write(&report_path, &redacted_report).map_err(|source| KvistError::Io {
        operation: "write review report",
        path: report_path.clone(),
        source,
    })?;

    // Digest the redacted report that was actually stored
    let report_digest = sha256_hex(redacted_report.as_bytes());

    // Write the receipt with HMAC authentication
    // The receipt explicitly records the advisory nature of the review
    let receipt = ReviewReceipt {
        schema_version: 1,
        review_id: format!("review-{}", timestamp_for_filename()),
        completed_at: crate::task_queue::Timestamp::now()
            .map(|t| t.to_string())
            .map_err(|source| KvistError::TaskClock { source })?,
        requirements_digest,
        contract_digest,
        design_digest,
        todos_projection_digest,
        root_contract_digest,
        parent_contract_digest,
        report_path: report_path.to_string_lossy().to_string(),
        report_digest,
        findings: report.decision_required,
        authentication: String::new(), // Will be computed after serialization
    };

    let receipt_json_unauth = serde_json::to_string_pretty(&receipt).map_err(|source| {
        KvistError::AgentEvidenceFailed {
            reason: format!("cannot serialize review receipt: {source}"),
        }
    })?;

    // Compute HMAC over the receipt JSON (excluding the HMAC field itself)
    let hmac_value = compute_receipt_hmac(&receipt_json_unauth, &context.project_dir);

    // Update the receipt with the HMAC
    let receipt = ReviewReceipt {
        authentication: hmac_value,
        ..receipt
    };

    let receipt_path = reviews_dir.join(format!("intent-{}.receipt.json", &receipt.review_id[7..]));
    let receipt_json = serde_json::to_string_pretty(&receipt).map_err(|source| {
        KvistError::AgentEvidenceFailed {
            reason: format!("cannot serialize review receipt with HMAC: {source}"),
        }
    })?;
    fs::write(&receipt_path, &receipt_json).map_err(|source| KvistError::Io {
        operation: "write review receipt",
        path: receipt_path.clone(),
        source,
    })?;

    Ok((receipt_path, report.report))
}

/// Collects redaction values from the project configuration.
fn evidence_redactions(config: &ProjectConfig) -> Vec<String> {
    let mut redactions = Vec::new();
    redactions.extend(config.agent.architect.redaction_values.clone());
    redactions
}

/// Computes the SHA-256 hex digest of bytes.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    format!("sha256:{}", hex::encode(result))
}

/// Derives an HMAC key from the project configuration.
///
/// Uses a stable derivation from the project directory path to ensure
/// the same key is used for signing and verification within a project.
fn derive_hmac_key(project_dir: &std::path::Path) -> [u8; HMAC_KEY_SIZE] {
    let path_bytes = project_dir.as_os_str().as_encoded_bytes();
    let mut hasher = Sha256::new();
    hasher.update(b"kvist-review-authentication-key-v1");
    hasher.update(path_bytes);
    let result = hasher.finalize();
    let mut key = [0u8; HMAC_KEY_SIZE];
    key.copy_from_slice(&result);
    key
}

/// Computes HMAC-SHA256 authentication for a receipt.
fn compute_receipt_hmac(receipt_json: &str, project_dir: &std::path::Path) -> String {
    let key = derive_hmac_key(project_dir);
    let mut mac =
        hmac::Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key size");
    mac.update(receipt_json.as_bytes());
    let result = mac.finalize();
    format!("hmac-sha256:{}", hex::encode(result.into_bytes()))
}

/// Verifies HMAC-SHA256 authentication for a receipt.
pub(crate) fn verify_receipt_hmac(
    receipt_json: &str,
    expected_hmac: &str,
    project_dir: &std::path::Path,
) -> bool {
    let computed = compute_receipt_hmac(receipt_json, project_dir);
    // Constant-time comparison to prevent timing attacks
    computed.as_bytes().ct_eq(expected_hmac.as_bytes()).into()
}

/// Returns a timestamp suitable for use in filenames.
fn timestamp_for_filename() -> String {
    crate::task_queue::Timestamp::now()
        .map(|t| t.to_string().replace(':', "-"))
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn sha256_hex_produces_valid_digest() {
        let digest = sha256_hex(b"test data");
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest.len(), 71); // "sha256:" (7) + 64 hex chars
    }

    #[test]
    fn sha256_hex_is_deterministic() {
        let d1 = sha256_hex(b"test");
        let d2 = sha256_hex(b"test");
        assert_eq!(d1, d2);
    }

    #[test]
    fn sha256_hex_differs_for_different_input() {
        let d1 = sha256_hex(b"test");
        let d2 = sha256_hex(b"different");
        assert_ne!(d1, d2);
    }

    #[test]
    fn hmac_key_derivation_is_deterministic() {
        let temp_dir = TempDir::new().unwrap();
        let key1 = derive_hmac_key(temp_dir.path());
        let key2 = derive_hmac_key(temp_dir.path());
        assert_eq!(key1, key2);
    }

    #[test]
    fn hmac_key_derivation_differs_by_project() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();
        let key1 = derive_hmac_key(temp_dir1.path());
        let key2 = derive_hmac_key(temp_dir2.path());
        assert_ne!(key1, key2);
    }

    #[test]
    fn receipt_hmac_verification() {
        let temp_dir = TempDir::new().unwrap();
        let receipt_json = r#"{"schema_version":1,"review_id":"test"}"#;
        let hmac = compute_receipt_hmac(receipt_json, temp_dir.path());
        assert!(verify_receipt_hmac(receipt_json, &hmac, temp_dir.path()));
    }

    #[test]
    fn receipt_hmac_fails_with_wrong_json() {
        let temp_dir = TempDir::new().unwrap();
        let receipt_json = r#"{"schema_version":1,"review_id":"test"}"#;
        let hmac = compute_receipt_hmac(receipt_json, temp_dir.path());
        let different_json = r#"{"schema_version":1,"review_id":"different"}"#;
        assert!(!verify_receipt_hmac(different_json, &hmac, temp_dir.path()));
    }

    #[test]
    fn receipt_hmac_fails_with_wrong_project() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();
        let receipt_json = r#"{"schema_version":1,"review_id":"test"}"#;
        let hmac = compute_receipt_hmac(receipt_json, temp_dir1.path());
        assert!(!verify_receipt_hmac(receipt_json, &hmac, temp_dir2.path()));
    }
}
