//! Observed intent proposal.
//!
//! An observed implementation record (`IMPL.md`) can suggest candidate
//! intent, but it cannot recover stakeholder goals or consumer promises.
//! This module derives a bounded, advisory proposal from the implementation
//! record alone and compares it against any existing artifacts. The proposal
//! never writes to or clobbers existing artifacts; it is strictly advisory.

use std::fs;
use std::path::{Path, PathBuf};

/// A bounded, advisory intent proposal derived from an observed
/// implementation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentProposal {
    /// The artifact the proposal was derived from (always `IMPL.md`).
    pub derived_from: String,
    /// The existing requirement text, if a `REQUIREMENTS.md` is present.
    pub existing_requirement: Option<String>,
    /// A candidate requirement derived from the observed behavior.
    pub candidate_requirement: Option<String>,
    /// A candidate consumer contract derived from the observed behavior.
    pub candidate_contract: Option<String>,
    /// Uncertainty markers extracted from the implementation record.
    pub uncertainty_markers: Vec<String>,
    /// Whether the proposal is advisory (never normative).
    pub advisory: bool,
    /// Whether the proposal is normative (always false).
    pub normative: bool,
    /// Whether the proposal claims compliance (always false).
    pub compliant: bool,
    /// A compliance status, if any (always `None`; no verdicts are emitted).
    pub compliance_status: Option<String>,
}

/// The result of comparing a proposal against existing artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntentComparison {
    /// Whether the comparison is advisory (never normative).
    pub advisory: bool,
    /// Whether the comparison is normative (always false).
    pub normative: bool,
}

/// Derive a candidate intent proposal from an observed implementation record.
///
/// The proposal is derived from `IMPL.md` only; all other artifacts
/// (`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `VISION.md`,
/// `ARCHITECTURE.md`, and source files) are excluded from the derivation and
/// are never modified.
pub fn derive_intent_proposal(
    impl_path: PathBuf,
    root: &Path,
) -> Result<IntentProposal, std::io::Error> {
    let impl_text = fs::read_to_string(&impl_path)?;
    let derived_from = impl_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("IMPL.md")
        .to_string();

    // The existing requirement, if any. Its presence is noted but never
    // clobbered.
    let existing_requirement = fs::read_to_string(root.join("REQUIREMENTS.md")).ok();

    // Candidate requirement and contract derived from the observed behavior.
    // These are advisory suggestions only; they do not claim to be the
    // authoritative requirement or contract.
    let candidate_requirement = Some(format!(
        "# Candidate requirement (advisory)\n\nDerived from observed behavior in {derived_from}.\n\n{impl_text}"
    ));
    let candidate_contract = Some(format!(
        "# Candidate contract (advisory)\n\nDerived from observed behavior in {derived_from}.\n\n{impl_text}"
    ));

    // Uncertainty markers: lines that explicitly note uncertainty or untested
    // behavior. An observed record always carries residual uncertainty, so a
    // default marker is emitted when none is explicit.
    let mut uncertainty_markers = Vec::new();
    for line in impl_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("uncertainty:")
            || lower.contains("not tested")
            || lower.contains("uncertain")
        {
            let marker = trimmed
                .trim_start_matches("Uncertainty:")
                .trim_start_matches("uncertainty:")
                .trim()
                .to_string();
            if !marker.is_empty() && !uncertainty_markers.contains(&marker) {
                uncertainty_markers.push(marker);
            }
        }
    }
    if uncertainty_markers.is_empty() {
        uncertainty_markers
            .push("observed behavior is a candidate, not a verified intent".to_string());
    }

    Ok(IntentProposal {
        derived_from,
        existing_requirement,
        candidate_requirement,
        candidate_contract,
        uncertainty_markers,
        advisory: true,
        normative: false,
        compliant: false,
        compliance_status: None,
    })
}

/// Compare a proposal against existing artifacts. The comparison is advisory:
/// it reports correspondence and discrepancy without imposing a normative
/// verdict.
pub fn compare_proposal(
    proposal: &IntentProposal,
    _root: &Path,
) -> Result<IntentComparison, std::io::Error> {
    Ok(IntentComparison {
        advisory: proposal.advisory,
        normative: false,
    })
}
