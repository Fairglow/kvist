use std::fs;
use tempfile::TempDir;

use kvist::{
    component_documents::{self, DocumentKind},
    reverse_discovery::{ReverseDiscoverOutcome, reverse_discover},
    task_queue,
};

#[test]
fn test_reverse_discover_simple_rust_crate() {
    let project = TempDir::new().expect("create temp dir");
    let path = project.path().to_path_buf();

    // Create a simple Rust project structure
    fs::create_dir_all(path.join("src")).expect("create src dir");
    fs::write(
        path.join("src/lib.rs"),
        r#"
pub struct MyDiscoveredStruct {
    pub value: u32,
}

pub fn public_discovered_fn() -> bool {
    true
}

#[test]
fn test_discovered_behaviour() {
    assert!(public_discovered_fn());
}
"#,
    )
    .expect("write lib.rs");

    let outcome = reverse_discover(&path).expect("run reverse discover");
    assert_eq!(
        outcome,
        ReverseDiscoverOutcome::Discovered { path: path.clone() }
    );

    let metadata_dir = path.join(".kvist");
    assert!(metadata_dir.join("REQUIREMENTS.md").is_file());
    assert!(metadata_dir.join("CONTRACT.md").is_file());
    assert!(metadata_dir.join("DESIGN.md").is_file());
    assert!(metadata_dir.join("TODOS.yaml").is_file());
    assert!(metadata_dir.join("IMPL.md").is_file());

    let contract = fs::read_to_string(metadata_dir.join("CONTRACT.md")).expect("read contract");
    assert!(contract.contains("MyDiscoveredStruct"));
    assert!(contract.contains("public_discovered_fn"));
    let requirements =
        fs::read_to_string(metadata_dir.join("REQUIREMENTS.md")).expect("read requirements");
    assert!(requirements.contains("test_discovered_behaviour"));
    for kind in [
        DocumentKind::Requirements,
        DocumentKind::Contract,
        DocumentKind::Design,
    ] {
        assert!(
            component_documents::validate_file(kind, &metadata_dir.join(kind.filename()))
                .expect("validate document")
                .is_valid()
        );
    }

    // Validate generated TODOS queue
    let queue_content = fs::read_to_string(metadata_dir.join("TODOS.yaml")).expect("read todos");
    task_queue::parse(&queue_content).expect("parse generated todos");
}

#[test]
fn test_reverse_discover_python_project() {
    let project = TempDir::new().expect("create temp dir");
    let path = project.path().to_path_buf();

    fs::write(
        path.join("main.py"),
        r#"
class PyDiscoveredClass:
    def __init__(self):
        pass

def py_discovered_fn():
    return True

def test_py_discovered():
    assert py_discovered_fn()
"#,
    )
    .expect("write main.py");

    let outcome = reverse_discover(&path).expect("run reverse discover on python");
    assert_eq!(
        outcome,
        ReverseDiscoverOutcome::Discovered { path: path.clone() }
    );

    let metadata_dir = path.join(".kvist");
    assert!(metadata_dir.join("REQUIREMENTS.md").is_file());

    let contract = fs::read_to_string(metadata_dir.join("CONTRACT.md")).expect("read contract");
    assert!(contract.contains("PyDiscoveredClass"));
    assert!(contract.contains("py_discovered_fn"));
    let requirements =
        fs::read_to_string(metadata_dir.join("REQUIREMENTS.md")).expect("read requirements");
    assert!(requirements.contains("test_py_discovered"));
}

#[test]
fn test_reverse_discover_refuses_to_overwrite_existing() {
    let project = TempDir::new().expect("create temp dir");
    let path = project.path().to_path_buf();

    let metadata_dir = path.join(".kvist");
    fs::create_dir_all(&metadata_dir).expect("create metadata dir");
    fs::write(
        metadata_dir.join("REQUIREMENTS.md"),
        "existing requirements",
    )
    .expect("write requirements");

    let err = reverse_discover(&path).expect_err("should refuse to overwrite existing spec");
    assert!(
        err.to_string()
            .contains("refusing to overwrite existing component document")
    );
}

#[test]
#[cfg(unix)]
fn reverse_discover_rejects_a_symlinked_metadata_directory() {
    use std::os::unix::fs::symlink;

    let project = TempDir::new().expect("project");
    let outside = TempDir::new().expect("outside directory");
    fs::write(project.path().join("lib.rs"), "pub fn exposed() {}\n").expect("write source");
    symlink(outside.path(), project.path().join(".kvist")).expect("link metadata directory");

    let error = reverse_discover(project.path()).expect_err("reject linked metadata directory");

    assert!(error.to_string().contains("link-like"));
    assert!(!outside.path().join("REQUIREMENTS.md").exists());
    assert!(!outside.path().join("CONTRACT.md").exists());
    assert!(!outside.path().join("DESIGN.md").exists());
}
