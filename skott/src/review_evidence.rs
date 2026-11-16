//! Versioned advisory review evidence types.
//!
//! Evidence is written under `.kvist/reviews/` within the component directory.
//! Each evidence type has a schema version, exact target digests, and bounded
//! provenance. Kvist alone writes canonical evidence; the model cannot mint it.
//! Raw transcripts and secrets are never persisted.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Current schema version for all review evidence.
pub const EVIDENCE_SCHEMA_VERSION: u32 = 1;

/// Directory name for review evidence within a component.
pub const REVIEWS_DIR: &str = ".kvist/reviews";

/// Maximum size for any single review evidence file.
pub const MAX_EVIDENCE_BYTES: u64 = 64 * 1024;

/// SHA-256 digest of a review target file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TargetDigest {
    /// Hex-encoded SHA-256 digest.
    pub sha256: String,
}

impl TargetDigest {
    /// Compute the digest of file contents.
    pub fn compute(content: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(content);
        let result = hasher.finalize();
        Self {
            sha256: hex::encode(result),
        }
    }

    /// Compute the digest of a file on disk.
    pub fn compute_file(path: &Path) -> Result<Self> {
        let content = fs::read(path).map_err(|source| Error::Io {
            operation: "read review target for digest".to_string(),
            path: Some(path.to_string_lossy().to_string()),
            source,
        })?;
        Ok(Self::compute(&content))
    }

    /// Validate that the digest has the correct format.
    pub fn validate(&self) -> Result<()> {
        if self.sha256.len() != 64 {
            return Err(Error::InvalidReviewEvidence {
                reason: "digest must be 64 hex characters".into(),
            });
        }
        for ch in self.sha256.chars() {
            if !ch.is_ascii_hexdigit() {
                return Err(Error::InvalidReviewEvidence {
                    reason: "digest must contain only hex characters".into(),
                });
            }
        }
        Ok(())
    }
}

/// A review target: a file or set of files under review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewTarget {
    /// Path relative to the component directory.
    pub path: String,
    /// SHA-256 digest of the target at review time.
    pub digest: TargetDigest,
}

/// Versioned advisory review receipt.
///
/// Records that a review was executed, with exact target digests, scope,
/// and provenance. Written by Kvist alone after the review completes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    /// Evidence schema version.
    pub schema_version: u32,
    /// Stable review identifier (UUID or similar).
    pub id: String,
    /// ISO 8601 timestamp of review completion.
    pub timestamp: String,
    /// Review targets with their digests at review time.
    pub targets: Vec<ReviewTarget>,
    /// Review scope description.
    pub scope: String,
    /// Reviewer identity (role + profile), if known.
    pub reviewer: Option<String>,
    /// Authoring context identity, if known.
    pub author: Option<String>,
    /// Tool identity (provider/profile/model).
    pub tool: Option<String>,
    /// Kvist version that produced this receipt.
    pub kvist_version: String,
    /// Path to the associated report, if any.
    pub report_path: Option<String>,
    /// SHA-256 digest of the report, if any.
    pub report_digest: Option<TargetDigest>,
    /// Acknowledgement status.
    pub acknowledged: Option<String>,
    /// Exception status.
    pub exception: Option<String>,
}

/// Versioned advisory review report.
///
/// The bounded, redacted review findings. Never raw transcripts or secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    /// Evidence schema version.
    pub schema_version: u32,
    /// Reference to the receipt this report belongs to.
    pub receipt_id: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Review scope description.
    pub scope: String,
    /// Bounded review findings (redacted).
    pub findings: String,
    /// Whether findings require human acknowledgement.
    pub requires_acknowledgement: bool,
}

/// Versioned review acknowledgement.
///
/// Records that a human has acknowledged the review findings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acknowledgement {
    /// Evidence schema version.
    pub schema_version: u32,
    /// Reference to the receipt being acknowledged.
    pub receipt_id: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Actor who acknowledged.
    pub actor: String,
    /// Optional acknowledgement notes.
    pub notes: Option<String>,
}

/// Versioned review exception.
///
/// Records a deliberate opt-out of review, with actor, timestamp, reason,
/// and exact target digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    /// Evidence schema version.
    pub schema_version: u32,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Review targets that were skipped.
    pub targets: Vec<ReviewTarget>,
    /// Actor who raised the exception.
    pub actor: String,
    /// Reason for the exception.
    pub reason: String,
}

/// Resolve the reviews directory for a component.
pub fn reviews_dir(component_path: &Path) -> PathBuf {
    component_path.join(REVIEWS_DIR)
}

/// Ensure the reviews directory exists.
pub fn ensure_reviews_dir(component_path: &Path) -> Result<PathBuf> {
    let dir = reviews_dir(component_path);
    fs::create_dir_all(&dir).map_err(|source| Error::Io {
        operation: "create review evidence directory".to_string(),
        path: Some(dir.to_string_lossy().to_string()),
        source,
    })?;
    Ok(dir)
}

/// Write a receipt to disk.
pub fn write_receipt(component_path: &Path, id: &str, receipt: &Receipt) -> Result<PathBuf> {
    let dir = ensure_reviews_dir(component_path)?;
    let path = dir.join(format!("receipt-{id}.json"));
    write_evidence_file(&path, receipt)?;
    Ok(path)
}

/// Write a report to disk.
pub fn write_report(component_path: &Path, id: &str, report: &Report) -> Result<PathBuf> {
    let dir = ensure_reviews_dir(component_path)?;
    let path = dir.join(format!("report-{id}.json"));
    write_evidence_file(&path, report)?;
    Ok(path)
}

/// Write an acknowledgement to disk.
pub fn write_acknowledgement(
    component_path: &Path,
    id: &str,
    acknowledgement: &Acknowledgement,
) -> Result<PathBuf> {
    let dir = ensure_reviews_dir(component_path)?;
    let path = dir.join(format!("ack-{id}.json"));
    write_evidence_file(&path, acknowledgement)?;
    Ok(path)
}

/// Write an exception to disk.
pub fn write_exception(component_path: &Path, id: &str, exception: &Exception) -> Result<PathBuf> {
    let dir = ensure_reviews_dir(component_path)?;
    let path = dir.join(format!("exception-{id}.json"));
    write_evidence_file(&path, exception)?;
    Ok(path)
}

fn write_evidence_file<T: Serialize>(path: &Path, evidence: &T) -> Result<()> {
    let content =
        serde_json::to_string_pretty(evidence).map_err(|source| Error::InvalidReviewEvidence {
            reason: format!("cannot serialize evidence: {source}"),
        })?;
    if content.len() > MAX_EVIDENCE_BYTES as usize {
        return Err(Error::InvalidReviewEvidence {
            reason: "evidence exceeds maximum size".into(),
        });
    }
    let path_string = path.to_string_lossy().to_string();
    let mut file = fs::File::create(path).map_err(|source| Error::Io {
        operation: "write review evidence".to_string(),
        path: Some(path_string.clone()),
        source,
    })?;
    file.write_all(content.as_bytes())
        .map_err(|source| Error::Io {
            operation: "write review evidence content".to_string(),
            path: Some(path_string.clone()),
            source,
        })?;
    file.sync_all().map_err(|source| Error::Io {
        operation: "sync review evidence".to_string(),
        path: Some(path_string.clone()),
        source,
    })?;
    Ok(())
}

/// Load a receipt from disk.
pub fn load_receipt(component_path: &Path, id: &str) -> Result<Receipt> {
    let path = reviews_dir(component_path).join(format!("receipt-{id}.json"));
    load_evidence_file(&path)
}

/// Load a report from disk.
pub fn load_report(component_path: &Path, id: &str) -> Result<Report> {
    let path = reviews_dir(component_path).join(format!("report-{id}.json"));
    load_evidence_file(&path)
}

/// Load an acknowledgement from disk.
pub fn load_acknowledgement(component_path: &Path, id: &str) -> Result<Acknowledgement> {
    let path = reviews_dir(component_path).join(format!("ack-{id}.json"));
    load_evidence_file(&path)
}

/// Load an exception from disk.
pub fn load_exception(component_path: &Path, id: &str) -> Result<Exception> {
    let path = reviews_dir(component_path).join(format!("exception-{id}.json"));
    load_evidence_file(&path)
}

fn load_evidence_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let content = fs::read_to_string(path).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            Error::ReviewEvidenceNotFound {
                path: path.to_path_buf(),
            }
        } else {
            Error::Io {
                operation: "read review evidence".to_string(),
                path: Some(path.to_string_lossy().to_string()),
                source,
            }
        }
    })?;
    serde_json::from_str(&content).map_err(|source| Error::InvalidReviewEvidence {
        reason: format!("cannot parse evidence at {}: {source}", path.display()),
    })
}

/// Validate that a receipt's target digests match the current files.
pub fn validate_receipt_targets(receipt: &Receipt, component_path: &Path) -> Result<()> {
    for target in &receipt.targets {
        let path = component_path.join(&target.path);
        if !path.exists() {
            return Err(Error::InvalidReviewEvidence {
                reason: format!("receipt target {} does not exist", target.path),
            });
        }
        let current_digest = TargetDigest::compute_file(&path)?;
        if current_digest != target.digest {
            return Err(Error::StaleReviewEvidence {
                reason: format!(
                    "receipt target {} has changed (expected {}, got {})",
                    target.path, target.digest.sha256, current_digest.sha256
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_digest_compute_and_validate() {
        let digest = TargetDigest::compute(b"test content");
        assert_eq!(digest.sha256.len(), 64);
        digest.validate().unwrap();
    }

    #[test]
    fn target_digest_invalid_length() {
        let digest = TargetDigest {
            sha256: "short".to_string(),
        };
        assert!(digest.validate().is_err());
    }

    #[test]
    fn target_digest_invalid_chars() {
        let digest = TargetDigest {
            sha256: "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg".to_string(),
        };
        assert!(digest.validate().is_err());
    }

    #[test]
    fn receipt_serialization() {
        let receipt = Receipt {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            id: "test-receipt".to_string(),
            timestamp: "2026-11-15T00:00:00Z".to_string(),
            targets: vec![],
            scope: "test".to_string(),
            reviewer: None,
            author: None,
            tool: None,
            kvist_version: "0.3.3".to_string(),
            report_path: None,
            report_digest: None,
            acknowledged: None,
            exception: None,
        };
        let serialized = serde_json::to_string(&receipt).unwrap();
        let parsed: Receipt = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed, receipt);
    }

    #[test]
    fn report_serialization() {
        let report = Report {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            receipt_id: "test-receipt".to_string(),
            timestamp: "2026-11-15T00:00:00Z".to_string(),
            scope: "test".to_string(),
            findings: "No issues found.".to_string(),
            requires_acknowledgement: false,
        };
        let serialized = serde_json::to_string(&report).unwrap();
        let parsed: Report = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed, report);
    }

    #[test]
    fn acknowledgement_serialization() {
        let ack = Acknowledgement {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            receipt_id: "test-receipt".to_string(),
            timestamp: "2026-11-15T00:00:00Z".to_string(),
            actor: "test-user".to_string(),
            notes: None,
        };
        let serialized = serde_json::to_string(&ack).unwrap();
        let parsed: Acknowledgement = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed, ack);
    }

    #[test]
    fn exception_serialization() {
        let exc = Exception {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            timestamp: "2026-11-15T00:00:00Z".to_string(),
            targets: vec![],
            actor: "test-user".to_string(),
            reason: "Deliberate skip for testing.".to_string(),
        };
        let serialized = serde_json::to_string(&exc).unwrap();
        let parsed: Exception = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed, exc);
    }

    #[test]
    fn evidence_file_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.json");
        let receipt = Receipt {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            id: "test".to_string(),
            timestamp: "2026-11-15T00:00:00Z".to_string(),
            targets: vec![],
            scope: "test".to_string(),
            reviewer: None,
            author: None,
            tool: None,
            kvist_version: "0.3.3".to_string(),
            report_path: None,
            report_digest: None,
            acknowledged: None,
            exception: None,
        };
        write_evidence_file(&path, &receipt).unwrap();
        let loaded: Receipt = load_evidence_file(&path).unwrap();
        assert_eq!(loaded, receipt);
    }

    #[test]
    fn reviews_dir_path() {
        let component = Path::new("/tmp/test-component");
        let dir = reviews_dir(component);
        assert_eq!(dir, component.join(".kvist/reviews"));
    }
}
