//! Observed intent proposal tests.
//!
//! An observed implementation can suggest candidate intent but cannot recover
//! stakeholder goals or consumer promises. These tests prove that proposal and
//! comparison are bounded advisory operations that preserve every existing
//! intended and observed artifact.
//!
//! These tests are failing because the observed intent proposal API does not
//! yet exist. They define the expected behavior for the implementation.

use std::fs;
use std::path::Path;

/// Create a test component directory with IMPL.md only.
fn create_test_component(root: &Path) -> std::io::Result<()> {
    fs::create_dir_all(root.join("src"))?;
    fs::create_dir_all(root.join("tests"))?;

    // Write IMPL.md with observed behavior
    fs::write(
        root.join("IMPL.md"),
        "# Implementation Record\n\nFunction `add(a, b)` returns the sum of two integers.\nTested with positive, negative, and zero values.\n",
    )?;

    // Write a simple source file
    fs::write(
        root.join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    )?;

    Ok(())
}

/// Test: IMPL-only input should produce a candidate intent proposal.
#[test]
fn test_impl_only_input() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();
    create_test_component(root_path).expect("create component");

    // Expected API: derive_intent_proposal(impl_path, root)
    // This should produce a proposal with candidate requirement and contract
    let proposal = maerg::observed_intent::derive_intent_proposal(
        root_path.join("IMPL.md"),
        root_path,
    )
    .expect("derive proposal from IMPL.md");

    // Proposal should be derived from IMPL.md only
    assert_eq!(proposal.derived_from, "IMPL.md");
    assert!(proposal.candidate_requirement.is_some());
    assert!(proposal.candidate_contract.is_some());
}

/// Test: Proposal should not clobber existing draft requirements.
#[test]
fn test_no_clobber_draft_requirements() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();
    create_test_component(root_path).expect("create component");

    // Write existing draft requirements
    fs::write(
        root_path.join("REQUIREMENTS.md"),
        "# Requirements\n\nREQ-001: Existing requirement that should not be overwritten.\n",
    )
    .expect("write requirements");

    let proposal = maerg::observed_intent::derive_intent_proposal(
        root_path.join("IMPL.md"),
        root_path,
    )
    .expect("derive proposal");

    // Original requirements should be unchanged
    let requirements = fs::read_to_string(root_path.join("REQUIREMENTS.md")).expect("read requirements");
    assert!(requirements.contains("REQ-001"));
    assert!(requirements.contains("Existing requirement that should not be overwritten"));

    // Proposal should note the existing requirement
    assert!(proposal.existing_requirement.is_some());
}

/// Test: Design output should be separate from requirements.
#[test]
fn test_design_output_separate() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();
    create_test_component(root_path).expect("create component");

    let proposal = maerg::observed_intent::derive_intent_proposal(
        root_path.join("IMPL.md"),
        root_path,
    )
    .expect("derive proposal");

    // Proposal should have separate fields for requirement and contract
    assert!(proposal.candidate_requirement.is_some());
    assert!(proposal.candidate_contract.is_some());
    // Design should not be mixed in
    assert!(proposal.candidate_requirement.as_ref().unwrap().contains("requirement"));
    assert!(proposal.candidate_contract.as_ref().unwrap().contains("contract"));
}

/// Test: Proposal should include uncertainty markers.
#[test]
fn test_uncertainty_markers() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();
    create_test_component(root_path).expect("create component");

    let proposal = maerg::observed_intent::derive_intent_proposal(
        root_path.join("IMPL.md"),
        root_path,
    )
    .expect("derive proposal");

    // Proposal should indicate uncertainty
    assert!(!proposal.uncertainty_markers.is_empty());
}

/// Test: Comparison should be advisory, not normative.
#[test]
fn test_advisory_comparison() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();
    create_test_component(root_path).expect("create component");

    // Write existing requirements
    fs::write(
        root_path.join("REQUIREMENTS.md"),
        "# Requirements\n\nREQ-001: Different requirement than observed.\n",
    )
    .expect("write requirements");

    let proposal = maerg::observed_intent::derive_intent_proposal(
        root_path.join("IMPL.md"),
        root_path,
    )
    .expect("derive proposal");

    let comparison = maerg::observed_intent::compare_proposal(&proposal, root_path)
        .expect("compare proposal");

    // Comparison should be advisory
    assert!(comparison.advisory);
    assert!(!comparison.normative);
}

/// Test: Context exclusion - proposal should not include excluded artifacts.
#[test]
fn test_context_exclusion() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();
    create_test_component(root_path).expect("create component");

    // Write various context artifacts that should be excluded
    fs::write(root_path.join("VISION.md"), "# Vision\nProduct vision text.\n").expect("write vision");
    fs::write(root_path.join("ARCHITECTURE.md"), "# Architecture\nArchitecture text.\n").expect("write architecture");
    fs::write(root_path.join("CONTRACT.md"), "# Contract\nContract text.\n").expect("write contract");
    fs::write(root_path.join("DESIGN.md"), "# Design\nDesign text.\n").expect("write design");

    let proposal = maerg::observed_intent::derive_intent_proposal(
        root_path.join("IMPL.md"),
        root_path,
    )
    .expect("derive proposal");

    // Proposal should not contain excluded context
    assert!(!proposal.derived_from.contains("vision"));
    assert!(!proposal.derived_from.contains("architecture"));
    assert!(!proposal.derived_from.contains("contract"));
    assert!(!proposal.derived_from.contains("design"));
}

/// Test: Proposal should not emit compliance verdicts.
#[test]
fn test_no_compliance_verdicts() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();
    create_test_component(root_path).expect("create component");

    let proposal = maerg::observed_intent::derive_intent_proposal(
        root_path.join("IMPL.md"),
        root_path,
    )
    .expect("derive proposal");

    // Proposal should not claim compliance
    assert!(!proposal.compliant);
    assert!(proposal.compliance_status.is_none());
}

/// Test: Proposal should not emit normative contracts.
#[test]
fn test_no_normative_contracts() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();
    create_test_component(root_path).expect("create component");

    let proposal = maerg::observed_intent::derive_intent_proposal(
        root_path.join("IMPL.md"),
        root_path,
    )
    .expect("derive proposal");

    // Proposal should be advisory, not normative
    assert!(proposal.advisory);
    assert!(!proposal.normative);
}

/// Test: Proposal should preserve all existing artifacts.
#[test]
fn test_proposal_preserves_all_artifacts() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();
    create_test_component(root_path).expect("create component");

    // Write all intent artifacts
    fs::write(root_path.join("REQUIREMENTS.md"), "# Requirements\nREQ-001: Requirement.\n").expect("write req");
    fs::write(root_path.join("CONTRACT.md"), "# Contract\nProvides: add(a, b)\n").expect("write contract");
    fs::write(root_path.join("DESIGN.md"), "# Design\nSimple addition.\n").expect("write design");

    let proposal = maerg::observed_intent::derive_intent_proposal(
        root_path.join("IMPL.md"),
        root_path,
    )
    .expect("derive proposal");

    // All original artifacts should be unchanged
    let req = fs::read_to_string(root_path.join("REQUIREMENTS.md")).expect("read req");
    let contract = fs::read_to_string(root_path.join("CONTRACT.md")).expect("read contract");
    let design = fs::read_to_string(root_path.join("DESIGN.md")).expect("read design");

    assert!(req.contains("REQ-001"));
    assert!(contract.contains("Provides: add(a, b)"));
    assert!(design.contains("Simple addition."));
}

/// Test: Uncertainty from IMPL should be preserved in proposal.
#[test]
fn test_uncertainty_preserved_in_proposal() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();
    create_test_component(root_path).expect("create component");

    // Write IMPL.md with explicit uncertainty
    fs::write(
        root_path.join("IMPL.md"),
        "# Implementation Record\n\nFunction `add(a, b)` returns the sum.\nUncertainty: overflow behavior not tested.\n",
    )
    .expect("write impl");

    let proposal = maerg::observed_intent::derive_intent_proposal(
        root_path.join("IMPL.md"),
        root_path,
    )
    .expect("derive proposal");

    // Uncertainty should be carried into proposal
    assert!(proposal.uncertainty_markers.contains("overflow"));
}

/// Test: Proposal should be marked as advisory.
#[test]
fn test_proposal_is_advisory() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();
    create_test_component(root_path).expect("create component");

    let proposal = maerg::observed_intent::derive_intent_proposal(
        root_path.join("IMPL.md"),
        root_path,
    )
    .expect("derive proposal");

    assert!(proposal.advisory);
}
