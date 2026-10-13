use std::fs;

use kvist::{
    component_documents::{
        COMPONENT_CONTRACT_TEMPLATE, COMPONENT_DESIGN_TEMPLATE, COMPONENT_REQUIREMENTS_TEMPLATE,
        DocumentDiagnosticKind, DocumentKind, MAX_COMPONENT_DOCUMENT_BYTES, validate,
        validate_file,
    },
    init::initialize,
};
use tempfile::TempDir;

#[test]
fn generated_root_component_documents_are_valid() {
    let project = TempDir::new().expect("create temporary project");
    initialize(project.path()).expect("initialize project");

    for kind in [
        DocumentKind::Requirements,
        DocumentKind::Contract,
        DocumentKind::Design,
    ] {
        let contents = fs::read_to_string(project.path().join("src").join(kind.filename()))
            .expect("read root component document");
        let validation = validate(kind, &contents);
        assert!(validation.is_valid(), "{:?}", validation.diagnostics);
        assert_eq!(validation.template_version, Some(1));
    }
}

#[test]
fn checked_in_templates_cover_each_document_contract() {
    for (kind, template) in [
        (DocumentKind::Requirements, COMPONENT_REQUIREMENTS_TEMPLATE),
        (DocumentKind::Contract, COMPONENT_CONTRACT_TEMPLATE),
        (DocumentKind::Design, COMPONENT_DESIGN_TEMPLATE),
    ] {
        assert!(validate(kind, template).is_valid());
    }
    assert!(COMPONENT_CONTRACT_TEMPLATE.contains("OpenAPI, AsyncAPI"));
    assert!(COMPONENT_CONTRACT_TEMPLATE.contains("authoritative for semantics"));
}

#[test]
fn detects_missing_unsupported_out_of_order_and_empty_sections() {
    let missing = validate(DocumentKind::Requirements, "# Requirements\n");
    assert_eq!(
        missing.diagnostics[0].kind,
        DocumentDiagnosticKind::MissingTemplateVersion
    );

    let unsupported = validate(
        DocumentKind::Requirements,
        &COMPONENT_REQUIREMENTS_TEMPLATE.replacen(
            "kvist-requirements-version: 1",
            "kvist-requirements-version: 99",
            1,
        ),
    );
    assert!(unsupported.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DocumentDiagnosticKind::UnsupportedTemplateVersion {
                found: 99,
                supported: 1,
            }
    }));

    let out_of_order = COMPONENT_REQUIREMENTS_TEMPLATE
        .replace("## Purpose and scope", "## TEMP")
        .replace("## Functional requirements", "## Purpose and scope")
        .replace("## TEMP", "## Functional requirements");
    assert!(
        validate(DocumentKind::Requirements, &out_of_order)
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(
                diagnostic.kind,
                DocumentDiagnosticKind::InvalidSectionOrder { .. }
            ))
    );

    let empty = COMPONENT_DESIGN_TEMPLATE.replace(
        "## Verification strategy\n\n[Map requirements and contract clauses to unit, integration, contract,\nproperty, platform, security, and compliance checks. Identify required test\nfixtures and independent evidence.]",
        "## Verification strategy\n",
    );
    assert!(
        validate(DocumentKind::Design, &empty)
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(
                diagnostic.kind,
                DocumentDiagnosticKind::EmptySection { .. }
            ))
    );
}

#[test]
fn validates_bounded_regular_files_without_writing() {
    let directory = TempDir::new().expect("create temporary directory");
    let path = directory.path().join("CONTRACT.md");
    fs::write(&path, COMPONENT_CONTRACT_TEMPLATE).expect("write contract");

    let validation = validate_file(DocumentKind::Contract, &path).expect("validate contract");
    assert!(validation.is_valid());
    assert_eq!(
        fs::read_to_string(&path).expect("read contract after validation"),
        COMPONENT_CONTRACT_TEMPLATE
    );

    let oversized = directory.path().join("DESIGN.md");
    fs::write(
        &oversized,
        vec![b'x'; MAX_COMPONENT_DOCUMENT_BYTES as usize + 1],
    )
    .expect("write oversized design");
    assert!(
        validate_file(DocumentKind::Design, &oversized)
            .expect_err("reject oversized design")
            .to_string()
            .contains("exceeds the")
    );
}

#[cfg(unix)]
#[test]
fn rejects_symbolic_link_documents() {
    use std::os::unix::fs::symlink;

    let directory = TempDir::new().expect("create temporary directory");
    let target = directory.path().join("target.md");
    fs::write(&target, COMPONENT_REQUIREMENTS_TEMPLATE).expect("write target");
    let path = directory.path().join("REQUIREMENTS.md");
    symlink(&target, &path).expect("create document link");

    assert!(
        validate_file(DocumentKind::Requirements, &path)
            .expect_err("reject document link")
            .to_string()
            .contains("symbolic link")
    );
}
