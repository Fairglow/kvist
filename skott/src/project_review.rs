//! Project-level document review and acceptance.
//!
//! Validates exact governed paths (VISION.md, ARCHITECTURE.md, ROOT_CONTRACT.md,
//! ADRs, and referenced normative schemas) and produces durable review evidence.
//! Arbitrary repository documentation is not treated as governed artifacts.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::review_evidence::Receipt;
use crate::review_evidence::{EVIDENCE_SCHEMA_VERSION, ReviewTarget, TargetDigest, write_receipt};

/// The governed project documents that participate in project acceptance.
/// Only these paths are validated; arbitrary repository documentation is excluded.
pub const GOVERNED_DOCUMENTS: &[&str] = &["VISION.md", "ARCHITECTURE.md", "ROOT_CONTRACT.md"];

/// Directory name for ADRs (Architecture Decision Records).
pub const ADRS_DIR: &str = "docs/adr";

/// Maximum size for any project document.
pub const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;

/// A governed project document with its current digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedDocument {
    /// Path relative to the project root.
    pub path: String,
    /// SHA-256 digest of the document at review time.
    pub digest: TargetDigest,
    /// Whether the document exists on disk.
    pub exists: bool,
}

/// Project acceptance status for a single document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AcceptanceStatus {
    /// The document has been reviewed and accepted.
    Accepted {
        /// Receipt id for the acceptance.
        receipt_id: String,
    },
    /// The document has not yet been reviewed.
    Pending,
    /// The document is explicitly opted out of review.
    OptedOut {
        /// Reason for opting out.
        reason: String,
    },
}

/// Project review configuration loaded from `kvist.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectReviewConfig {
    /// Whether project document review is required before acceptance.
    #[serde(default = "default_required")]
    pub required: bool,
    /// Documents explicitly opted out of review.
    #[serde(default)]
    pub opt_out: Vec<String>,
}

fn default_required() -> bool {
    true
}

impl Default for ProjectReviewConfig {
    fn default() -> Self {
        Self {
            required: true,
            opt_out: Vec::new(),
        }
    }
}

/// Discover all governed documents in a project.
pub fn discover_governed_documents(project_path: &Path) -> Result<Vec<GovernedDocument>> {
    let mut documents = Vec::new();

    // Check the fixed governed documents.
    for doc in GOVERNED_DOCUMENTS {
        let path = project_path.join(doc);
        let exists = path.is_file();
        let digest = if exists {
            TargetDigest::compute_file(&path)?
        } else {
            TargetDigest::compute(b"")
        };
        documents.push(GovernedDocument {
            path: doc.to_string(),
            digest,
            exists,
        });
    }

    // Discover ADRs in the ADR directory.
    let adr_path = project_path.join(ADRS_DIR);
    if adr_path.is_dir() {
        let entries = fs::read_dir(&adr_path).map_err(|source| Error::Io {
            operation: "read ADR directory".to_string(),
            path: Some(adr_path.to_string_lossy().to_string()),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| Error::Io {
                operation: "read ADR directory entry".to_string(),
                path: Some(adr_path.to_string_lossy().to_string()),
                source,
            })?;
            if entry.path().is_file() {
                let filename = entry.file_name().to_string_lossy().to_string();
                let relative = format!("{}/{}", ADRS_DIR, filename);
                let digest = TargetDigest::compute_file(&entry.path())?;
                documents.push(GovernedDocument {
                    path: relative,
                    digest,
                    exists: true,
                });
            }
        }
    }

    Ok(documents)
}

/// Validate that a set of documents matches the expected governed set.
pub fn validate_governed_paths(paths: &[String]) -> Result<()> {
    for path in paths {
        let is_governed = GOVERNED_DOCUMENTS.iter().any(|doc| *path == *doc)
            || path.starts_with(&format!("{}/", ADRS_DIR));
        if !is_governed {
            return Err(Error::InvalidPath {
                path: path.clone(),
                reason: "not a governed project document".into(),
            });
        }
    }
    Ok(())
}

/// Create a project acceptance receipt for the given documents.
pub fn accept_project_documents(
    project_path: &Path,
    documents: &[GovernedDocument],
    scope: &str,
    reviewer: Option<&str>,
) -> Result<Receipt> {
    if documents.is_empty() {
        return Err(Error::InvalidReviewEvidence {
            reason: "no documents to accept".into(),
        });
    }

    let targets: Vec<ReviewTarget> = documents
        .iter()
        .map(|doc| ReviewTarget {
            path: doc.path.clone(),
            digest: doc.digest.clone(),
        })
        .collect();

    let receipt = Receipt {
        schema_version: EVIDENCE_SCHEMA_VERSION,
        id: generate_receipt_id(),
        timestamp: timestamp_now(),
        targets,
        scope: scope.to_string(),
        reviewer: reviewer.map(|s| s.to_string()),
        author: None,
        tool: None,
        kvist_version: env!("CARGO_PKG_VERSION").to_string(),
        report_path: None,
        report_digest: None,
        acknowledged: None,
        exception: None,
    };

    write_receipt(project_path, &receipt.id, &receipt)?;

    Ok(receipt)
}

/// Generate a stable receipt identifier.
fn generate_receipt_id() -> String {
    let mut hasher = Sha256::new();
    hasher.update(timestamp_now().as_bytes());
    let result = hasher.finalize();
    hex::encode(result)[..16].to_string()
}

/// Current UTC timestamp in ISO 8601 format.
fn timestamp_now() -> String {
    let now = SystemTime::now();
    let duration = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    // Simple ISO 8601 without subsecond precision.
    let days = secs / 86400;
    let rem = secs % 86400;
    let hours = rem / 3600;
    let mins = (rem % 3600) / 60;
    let seconds = rem % 60;
    // Days since epoch to date (simplified).
    let mut year = 1970;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    let mut month = 1;
    let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let days_in_feb = if is_leap(year) { 29 } else { 28 };
    let mut md = month_days;
    md[1] = days_in_feb;
    for (i, &d) in md.iter().enumerate() {
        if remaining_days < d as i64 {
            month = i + 1;
            break;
        }
        remaining_days -= d as i64;
    }
    let day = remaining_days + 1;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, mins, seconds
    )
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// List acceptance status for all governed documents.
pub fn list_acceptance_status(
    project_path: &Path,
    documents: &[GovernedDocument],
) -> Result<Vec<(String, AcceptanceStatus)>> {
    let mut statuses = Vec::new();

    for doc in documents {
        if !doc.exists {
            statuses.push((doc.path.clone(), AcceptanceStatus::Pending));
            continue;
        }

        // Check for existing receipt.
        let reviews_dir = project_path.join(".kvist/reviews");
        if reviews_dir.is_dir() {
            let entries = fs::read_dir(&reviews_dir).map_err(|source| Error::Io {
                operation: "read reviews directory".to_string(),
                path: Some(reviews_dir.to_string_lossy().to_string()),
                source,
            })?;
            for entry in entries {
                let entry = entry.map_err(|source| Error::Io {
                    operation: "read reviews entry".to_string(),
                    path: Some(reviews_dir.to_string_lossy().to_string()),
                    source,
                })?;
                let filename = entry.file_name().to_string_lossy().to_string();
                if filename.starts_with("receipt-") && filename.ends_with(".json") {
                    // Check if this receipt covers this document.
                    let receipt_path = entry.path();
                    let content =
                        fs::read_to_string(&receipt_path).map_err(|source| Error::Io {
                            operation: "read receipt".to_string(),
                            path: Some(receipt_path.to_string_lossy().to_string()),
                            source,
                        })?;
                    let receipt: Receipt = serde_json::from_str(&content).map_err(|source| {
                        Error::InvalidReviewEvidence {
                            reason: format!("cannot parse receipt: {source}"),
                        }
                    })?;
                    if receipt.targets.iter().any(|t| t.path == doc.path) {
                        statuses.push((
                            doc.path.clone(),
                            AcceptanceStatus::Accepted {
                                receipt_id: receipt.id.clone(),
                            },
                        ));
                        break;
                    }
                }
            }
        }

        // If no receipt found, status is pending.
        if statuses.iter().all(|(path, _)| path != &doc.path) {
            statuses.push((doc.path.clone(), AcceptanceStatus::Pending));
        }
    }

    Ok(statuses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn governed_documents_are_fixed() {
        assert!(GOVERNED_DOCUMENTS.contains(&"VISION.md"));
        assert!(GOVERNED_DOCUMENTS.contains(&"ARCHITECTURE.md"));
        assert!(GOVERNED_DOCUMENTS.contains(&"ROOT_CONTRACT.md"));
    }

    #[test]
    fn validate_governed_paths_accepts_valid() {
        let paths = vec![
            "VISION.md".to_string(),
            "ARCHITECTURE.md".to_string(),
            "docs/adr/0001-example.md".to_string(),
        ];
        assert!(validate_governed_paths(&paths).is_ok());
    }

    #[test]
    fn validate_governed_paths_rejects_invalid() {
        let paths = vec!["README.md".to_string()];
        assert!(validate_governed_paths(&paths).is_err());
    }

    #[test]
    fn project_review_config_defaults() {
        let config = ProjectReviewConfig::default();
        assert!(config.required);
        assert!(config.opt_out.is_empty());
    }

    #[test]
    fn governed_document_serialization() {
        let doc = GovernedDocument {
            path: "VISION.md".to_string(),
            digest: TargetDigest::compute(b"test"),
            exists: true,
        };
        let serialized = serde_json::to_string(&doc).unwrap();
        let parsed: GovernedDocument = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed, doc);
    }
}
