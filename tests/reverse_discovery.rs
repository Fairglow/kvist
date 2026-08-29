use std::fs;
use tempfile::TempDir;

use kvist::{
    reverse_discovery::{ReverseDiscoverOutcome, reverse_discover},
    specification, task_queue,
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
    assert!(metadata_dir.join("SPEC.md").is_file());
    assert!(metadata_dir.join("TODOS.yaml").is_file());
    assert!(metadata_dir.join("IMPL.md").is_file());

    // Validate generated specification
    let spec_content = fs::read_to_string(metadata_dir.join("SPEC.md")).expect("read spec");
    assert!(spec_content.contains("MyDiscoveredStruct"));
    assert!(spec_content.contains("public_discovered_fn"));
    assert!(spec_content.contains("test_discovered_behaviour"));

    let validation =
        specification::validate_file(&metadata_dir.join("SPEC.md")).expect("validate spec file");
    assert!(validation.is_valid());

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
    assert!(metadata_dir.join("SPEC.md").is_file());

    let spec_content = fs::read_to_string(metadata_dir.join("SPEC.md")).expect("read spec");
    assert!(spec_content.contains("PyDiscoveredClass"));
    assert!(spec_content.contains("py_discovered_fn"));
    assert!(spec_content.contains("test_py_discovered"));
}

#[test]
fn test_reverse_discover_refuses_to_overwrite_existing() {
    let project = TempDir::new().expect("create temp dir");
    let path = project.path().to_path_buf();

    let metadata_dir = path.join(".kvist");
    fs::create_dir_all(&metadata_dir).expect("create metadata dir");
    fs::write(metadata_dir.join("SPEC.md"), "existing spec").expect("write spec");

    let err = reverse_discover(&path).expect_err("should refuse to overwrite existing spec");
    assert!(
        err.to_string()
            .contains("refusing to overwrite existing specification")
    );
}
