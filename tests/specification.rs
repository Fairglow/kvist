use std::fs;

use kvist::{
    artifacts::root_artifacts,
    init::initialize,
    specification::{
        COMPONENT_SPEC_TEMPLATE, MAX_SPECIFICATION_BYTES, SpecificationDiagnosticKind,
        SpecificationLayer, SpecificationSection, validate, validate_file,
    },
};
use tempfile::TempDir;

fn valid_specification() -> String {
    let project = TempDir::new().expect("create temporary project");
    initialize(project.path()).expect("initialize project");
    fs::read_to_string(project.path().join("src/SPEC.md")).expect("read root specification")
}

#[test]
fn generated_root_specification_is_valid() {
    let contents = valid_specification();

    let validation = validate(&contents);

    assert!(validation.is_valid(), "{:?}", validation.diagnostics);
    assert_eq!(validation.template_version, Some(1));
}

#[test]
fn detects_missing_template_version_at_the_first_line() {
    let validation = validate("# Root Component Specification\n");

    assert_eq!(
        validation.diagnostics[0],
        kvist::specification::SpecificationDiagnostic {
            kind: SpecificationDiagnosticKind::MissingTemplateVersion,
            line: 1,
            column: 1,
        }
    );
}

#[test]
fn detects_unsupported_template_versions() {
    let contents = valid_specification().replacen(
        "<!-- kvist-template-version: 1 -->",
        "<!-- kvist-template-version: 99 -->",
        1,
    );

    let validation = validate(&contents);

    assert!(validation.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == SpecificationDiagnosticKind::UnsupportedTemplateVersion {
                found: 99,
                supported: 1,
            }
    }));
}

#[test]
fn detects_missing_layers_and_reports_end_of_file_location() {
    let contents = "<!-- kvist-template-version: 1 -->\n# Root Component Specification\n";

    let validation = validate(contents);

    assert!(validation.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == SpecificationDiagnosticKind::MissingLayer {
                layer: SpecificationLayer::ExecutiveSummary,
            }
            && diagnostic.line == 3
            && diagnostic.column == 1
    }));
}

#[test]
fn detects_out_of_order_layers() {
    let contents = concat!(
        "<!-- kvist-template-version: 1 -->\n",
        "<details>\n",
        "<summary>Layer 2: Architectural guarantees</summary>\n",
        "## Constraints and invariants\ncontent\n</details>\n",
        "<details open>\n",
        "<summary>Layer 1: Executive summary and public contract</summary>\n",
        "## Purpose\ncontent\n## Public contract\ncontent\n</details>\n",
        "<details>\n",
        "<summary>Layer 3: Detailed strategy and algorithms</summary>\n",
        "## Design and failure paths\ncontent\n</details>\n",
    );

    let validation = validate(contents);

    assert!(validation.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == SpecificationDiagnosticKind::InvalidLayerOrder {
                layer: SpecificationLayer::ArchitecturalGuarantees,
            }
            && diagnostic.line == 3
    }));
}

#[test]
fn detects_empty_required_sections() {
    let contents = valid_specification().replace(
        "## Constraints and invariants\n\n[Describe performance bounds, concurrency invariants, memory limits, dependency\npolicy, and compatibility commitments.]",
        "## Constraints and invariants\n",
    );

    let validation = validate(&contents);

    assert!(validation.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == SpecificationDiagnosticKind::EmptySection {
                layer: SpecificationLayer::ArchitecturalGuarantees,
                section: SpecificationSection::ConstraintsAndInvariants,
            }
    }));
}

#[test]
fn does_not_treat_a_following_heading_as_required_section_content() {
    let contents = valid_specification().replace(
        "## Purpose\n\n[Describe the root component's purpose and why it exists.]",
        "## Purpose\n\n## Rationale\n\n[Describe the root component's rationale.]",
    );

    let validation = validate(&contents);

    assert!(validation.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == SpecificationDiagnosticKind::EmptySection {
                layer: SpecificationLayer::ExecutiveSummary,
                section: SpecificationSection::Purpose,
            }
    }));
}

#[test]
fn accepts_utf8_content_and_preserves_unvalidated_markdown() {
    let contents = valid_specification().replace(
        "[Describe the root component's purpose and why it exists.]",
        "Kvist supports Swedish users: räksmörgås.",
    );

    let validation = validate(&contents);

    assert!(validation.is_valid(), "{:?}", validation.diagnostics);
}

#[test]
fn validates_a_specification_file_without_writing_it() {
    let directory = TempDir::new().expect("create temporary directory");
    let path = directory.path().join("SPEC.md");
    let contents = valid_specification();
    fs::write(&path, &contents).expect("write specification fixture");

    let validation = validate_file(&path).expect("validate specification file");

    assert!(validation.is_valid());
    assert_eq!(
        fs::read_to_string(&path).expect("read specification after validation"),
        contents
    );
}

#[test]
fn rejects_specification_files_above_the_parsing_limit() {
    let directory = TempDir::new().expect("create temporary directory");
    let path = directory.path().join("SPEC.md");
    fs::write(&path, vec![b'x'; MAX_SPECIFICATION_BYTES as usize + 1])
        .expect("write oversized specification");

    let error = validate_file(&path).expect_err("reject oversized specification");

    assert!(error.to_string().contains("exceeds the"));
}

#[cfg(unix)]
#[test]
fn rejects_symbolic_link_specifications() {
    use std::os::unix::fs::symlink;

    let directory = TempDir::new().expect("create temporary directory");
    let target = directory.path().join("target.md");
    fs::write(&target, COMPONENT_SPEC_TEMPLATE).expect("write specification target");
    let path = directory.path().join("SPEC.md");
    symlink(&target, &path).expect("create specification link");

    let error = validate_file(&path).expect_err("reject specification link");

    assert!(error.to_string().contains("symbolic link"));
}

#[test]
fn root_artifact_template_stays_valid() {
    let specification = root_artifacts()
        .iter()
        .find(|artifact| artifact.relative_path == "src/SPEC.md")
        .expect("root specification template");

    assert!(validate(specification.contents).is_valid());
}
