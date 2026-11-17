//! Advisory document review evidence.
//!
//! Records bounded, redacted review reports bound to the exact digest of a
//! governed document, together with explicit human acknowledgement and
//! per-bundle exceptions. Review is default-on; a project may opt out through
//! a persisted configuration. Evidence is stored under the governed
//! `.kvist/review` path and never captures raw project paths.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Maximum stored review report size in bytes. Larger reports are rejected so
/// that evidence stays bounded and inspectable.
const MAX_REPORT_BYTES: usize = 64 * 1024;

/// Governed directory (relative to the project root) that holds review
/// evidence.
const REVIEW_DIR: &str = ".kvist/review";

/// A review report bound to the exact digest of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewBundle {
    /// The `sha256:`-prefixed digest the review was produced against.
    pub document_digest: String,
    /// The document bytes the review was produced against.
    pub document: Vec<u8>,
    /// The agent that assisted the review, if any. Optional metadata; a review
    /// may be registered without an agent (human-reviewed directly).
    pub agent: Option<String>,
    /// The review report. Bounded and redacted before storage.
    pub report: String,
}

/// The lifecycle status of a review for a document digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewStatus {
    /// Registered but not yet acknowledged.
    Registered,
    /// Explicitly acknowledged by a named human.
    Acknowledged,
    /// Carries a deliberate per-bundle exception.
    Excepted,
    /// Superseded by a newer review of a different document version.
    Stale,
}

/// A stored review record, with the report already redacted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredReview {
    document_digest: String,
    agent: Option<String>,
    report: String,
    acknowledged_by: Option<String>,
    exception: Option<String>,
    /// Monotonic registration sequence, used to detect superseded reviews.
    sequence: u64,
}

/// The loaded, read-only view of a review: the redacted report, the optional
/// agent, and the current lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// The document digest the review is bound to.
    pub document_digest: String,
    /// The agent that assisted the review, if any.
    pub agent: Option<String>,
    /// The redacted review report.
    pub report: String,
    /// The current lifecycle status of the review.
    pub status: ReviewStatus,
}

/// The canonical task projection of a review, used by the task queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewTaskProjection {
    /// The document digest the review is bound to.
    pub document_digest: String,
    /// `"acknowledged"` or `"pending_acknowledgement"`.
    pub status: &'static str,
}

/// Review configuration. Review is default-on; a project may opt out by
/// recording itself in a persisted configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewConfig {
    /// Projects that have explicitly opted out of review.
    opted_out: Vec<String>,
}

impl ReviewConfig {
    /// Whether review is required for the given project. Default-on unless the
    /// project has opted out.
    pub fn review_required(&self, project: &Path) -> bool {
        !self.opted_out.iter().any(|p| Path::new(p) == project)
    }

    /// Record that the given project has opted out of review.
    pub fn opt_out_project(&mut self, project: &Path) {
        let key = project.to_string_lossy().into_owned();
        if !self.opted_out.contains(&key) {
            self.opted_out.push(key);
        }
    }

    /// Persist the configuration to the governed path under the project.
    pub fn save(&self, project: &Path) -> Result<(), ReviewError> {
        let dir = project.join(REVIEW_DIR);
        fs::create_dir_all(&dir)?;
        let path = dir.join("config.json");
        let json = serde_json::to_string(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load the configuration from the governed path, defaulting to required
    /// when absent.
    pub fn load(project: &Path) -> Result<Self, ReviewError> {
        let path = project.join(REVIEW_DIR).join("config.json");
        match fs::read_to_string(path) {
            Ok(json) => Ok(serde_json::from_str(&json)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
}

/// Review-specific failures.
#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    /// The bundle digest does not match the document bytes.
    #[error("review digest mismatch: expected {expected}, found {found}")]
    DigestMismatch { expected: String, found: String },
    /// The report exceeds the bounded size.
    #[error("review report exceeds {limit} bytes")]
    ReportTooLarge { limit: usize },
    /// The acknowledgement identity is empty.
    #[error("acknowledgement requires a non-empty identity")]
    EmptyIdentity,
    /// The review is not registered.
    #[error("review not registered for {digest}")]
    NotRegistered { digest: String },
    /// A filesystem or serialization failure.
    #[error("review storage failed: {0}")]
    Storage(#[from] std::io::Error),
    /// A serialization failure.
    #[error("review serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Compute the `sha256:`-prefixed digest of a document.
pub fn document_digest(document: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(document);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// The governed review directory under a project.
fn review_dir(project: &Path) -> PathBuf {
    project.join(REVIEW_DIR)
}

/// The governed path of a single review record. The digest's `:` is replaced
/// so the name is a safe file name.
fn review_path(project: &Path, digest: &str) -> PathBuf {
    review_dir(project).join(format!("{digest}.json").replace(':', "_"))
}

/// The path of the registration manifest, which records review order.
fn manifest_path(project: &Path) -> PathBuf {
    review_dir(project).join("manifest.json")
}

/// A review report with absolute paths redacted to `<path>`.
fn redact_paths(report: &str) -> String {
    let mut out = String::with_capacity(report.len());
    let mut chars = report.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' {
            out.push_str("<path>");
            // Consume the rest of the path token (until whitespace).
            while let Some(&next) = chars.peek() {
                if next.is_whitespace() {
                    break;
                }
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Load the registration manifest, defaulting to empty when absent.
fn load_manifest(project: &Path) -> Result<BTreeMap<String, u64>, ReviewError> {
    match fs::read_to_string(manifest_path(project)) {
        Ok(json) => Ok(serde_json::from_str(&json)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error.into()),
    }
}

/// Store the registration manifest.
fn save_manifest(project: &Path, manifest: &BTreeMap<String, u64>) -> Result<(), ReviewError> {
    let json = serde_json::to_string(manifest)?;
    fs::write(manifest_path(project), json)?;
    Ok(())
}

/// Load a stored review record, or `None` when absent.
fn load_record(project: &Path, digest: &str) -> Result<Option<StoredReview>, ReviewError> {
    match fs::read_to_string(review_path(project, digest)) {
        Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Register a review for a document, verifying the digest and storing a
/// bounded, redacted report under the governed path.
pub fn register_review(project: &Path, bundle: ReviewBundle) -> Result<(), ReviewError> {
    let found = document_digest(&bundle.document);
    if found != bundle.document_digest {
        return Err(ReviewError::DigestMismatch {
            expected: bundle.document_digest,
            found,
        });
    }
    if bundle.report.len() > MAX_REPORT_BYTES {
        return Err(ReviewError::ReportTooLarge {
            limit: MAX_REPORT_BYTES,
        });
    }
    let dir = review_dir(project);
    fs::create_dir_all(&dir)?;

    let mut manifest = load_manifest(project)?;
    let next_sequence = manifest.values().max().copied().unwrap_or(0) + 1;
    manifest.insert(bundle.document_digest.clone(), next_sequence);
    save_manifest(project, &manifest)?;

    let record = StoredReview {
        document_digest: bundle.document_digest,
        agent: bundle.agent,
        report: redact_paths(&bundle.report),
        acknowledged_by: None,
        exception: None,
        sequence: next_sequence,
    };
    let json = serde_json::to_string(&record)?;
    fs::write(review_path(project, &record.document_digest), json)?;
    Ok(())
}

/// Load the stored (redacted) review record for a digest, exposing the report,
/// the optional agent, and the current lifecycle status.
pub fn load_review(project: &Path, digest: String) -> Result<Review, ReviewError> {
    let record = load_record(project, &digest)?.ok_or(ReviewError::NotRegistered {
        digest: digest.clone(),
    })?;
    Ok(Review {
        status: status_of(project, &record),
        document_digest: record.document_digest,
        agent: record.agent,
        report: record.report,
    })
}

/// Explicitly acknowledge a review with a non-empty identity.
pub fn acknowledge_review(
    project: &Path,
    digest: String,
    identity: &str,
) -> Result<(), ReviewError> {
    if identity.trim().is_empty() {
        return Err(ReviewError::EmptyIdentity);
    }
    fs::create_dir_all(review_dir(project))?;
    let mut record = load_record(project, &digest)?.ok_or(ReviewError::NotRegistered {
        digest: digest.clone(),
    })?;
    record.acknowledged_by = Some(identity.to_string());
    let json = serde_json::to_string(&record)?;
    fs::write(review_path(project, &digest), json)?;
    Ok(())
}

/// Record a deliberate per-bundle exception for a digest. The exception is
/// bound to the exact digest and overrides the review requirement for that
/// bundle alone; it does not require a prior registration.
pub fn add_review_exception(
    project: &Path,
    digest: String,
    reason: &str,
) -> Result<(), ReviewError> {
    fs::create_dir_all(review_dir(project))?;
    let mut manifest = load_manifest(project)?;
    let mut record = match load_record(project, &digest)? {
        Some(record) => record,
        None => {
            let next_sequence = manifest.values().max().copied().unwrap_or(0) + 1;
            manifest.insert(digest.clone(), next_sequence);
            StoredReview {
                document_digest: digest,
                agent: None,
                report: String::new(),
                acknowledged_by: None,
                exception: None,
                sequence: next_sequence,
            }
        }
    };
    record.exception = Some(reason.to_string());
    save_manifest(project, &manifest)?;
    let json = serde_json::to_string(&record)?;
    fs::write(review_path(project, &record.document_digest), json)?;
    Ok(())
}

/// The lifecycle status of a review for a digest.
pub fn review_status(project: &Path, digest: String) -> ReviewStatus {
    match load_record(project, &digest).ok().flatten() {
        Some(record) => status_of(project, &record),
        None => ReviewStatus::Registered,
    }
}

/// Compute the lifecycle status of a stored record.
fn status_of(project: &Path, record: &StoredReview) -> ReviewStatus {
    if record.exception.is_some() {
        return ReviewStatus::Excepted;
    }
    let manifest = load_manifest(project).ok().unwrap_or_default();
    let is_latest = manifest
        .values()
        .max()
        .is_some_and(|latest| *latest == record.sequence);
    if !is_latest {
        return ReviewStatus::Stale;
    }
    if record.acknowledged_by.is_some() {
        ReviewStatus::Acknowledged
    } else {
        ReviewStatus::Registered
    }
}

/// The canonical task projection of a review, if one is registered.
pub fn review_task_projection(project: &Path, digest: String) -> Option<ReviewTaskProjection> {
    let record = load_record(project, &digest).ok().flatten()?;
    let status = if record.acknowledged_by.is_some() {
        "acknowledged"
    } else {
        "pending_acknowledgement"
    };
    Some(ReviewTaskProjection {
        document_digest: record.document_digest,
        status,
    })
}
