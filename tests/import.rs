use std::fs;
use std::process::Command;
use tempfile::TempDir;

use kvist::component_documents::{
    COMPONENT_CONTRACT_TEMPLATE, COMPONENT_DESIGN_TEMPLATE, COMPONENT_REQUIREMENTS_TEMPLATE,
};
use kvist::import::{ImportOutcome, import};

fn create_local_git_repo(with_artifacts: bool, is_rust_project: bool) -> (TempDir, String) {
    let repo_dir = TempDir::new().expect("create local repo dir");

    // Set git config in the repo for testing commits
    let run_git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo_dir.path())
            .status()
            .expect("run git command");
        assert!(status.success(), "git command failed: {:?}", args);
    };

    run_git(&["init", "-b", "main"]);
    run_git(&["config", "user.name", "Test User"]);
    run_git(&["config", "user.email", "test@example.invalid"]);

    if is_rust_project {
        fs::create_dir_all(repo_dir.path().join("src")).expect("create src");
        fs::write(repo_dir.path().join("src/lib.rs"), "pub fn hello() {}").expect("write lib.rs");
        fs::write(
            repo_dir.path().join("Cargo.toml"),
            r#"[package]
name = "imported-project"
version = "0.1.0"
"#,
        )
        .expect("write Cargo.toml");
    }

    if with_artifacts {
        let todos_content = r#"schema_version: 1
component:
  requirements_revision: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  contract_revision: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  design_revision: "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
  parent_contract: null
  revalidation:
    state: current
    checked_at: "2026-08-21T20:59:05Z"
    stale_since: null
    causes: []
tasks: []
"#;
        let impl_content = "<!-- kvist-implementation-version: 1 -->\n# Implementation Record\n";

        fs::write(
            repo_dir.path().join("REQUIREMENTS.md"),
            COMPONENT_REQUIREMENTS_TEMPLATE,
        )
        .expect("write REQUIREMENTS.md");
        fs::write(
            repo_dir.path().join("CONTRACT.md"),
            COMPONENT_CONTRACT_TEMPLATE,
        )
        .expect("write CONTRACT.md");
        fs::write(repo_dir.path().join("DESIGN.md"), COMPONENT_DESIGN_TEMPLATE)
            .expect("write DESIGN.md");
        fs::write(repo_dir.path().join("TODOS.yaml"), todos_content).expect("write TODOS.yaml");
        fs::write(repo_dir.path().join("IMPL.md"), impl_content).expect("write IMPL.md");
    } else {
        // Create at least one dummy file to commit if not a Rust project
        if !is_rust_project {
            fs::write(repo_dir.path().join("dummy.txt"), "hello").expect("write dummy.txt");
        }
    }

    run_git(&["add", "."]);
    run_git(&["commit", "-m", "Initial commit"]);

    let repo_url = repo_dir.path().to_string_lossy().into_owned();
    (repo_dir, repo_url)
}

#[test]
fn test_import_with_existing_artifacts() {
    let (_repo_dir, repo_url) = create_local_git_repo(true, true);
    let dest_dir = TempDir::new().expect("create dest dir");

    let outcome =
        import(&repo_url, "main", None, dest_dir.path()).expect("import existing artifacts");
    assert_eq!(
        outcome,
        ImportOutcome::ImportedWithExistingArtifacts {
            dest_dir: dest_dir.path().to_path_buf()
        }
    );

    assert!(dest_dir.path().join("REQUIREMENTS.md").is_file());
    assert!(dest_dir.path().join("CONTRACT.md").is_file());
    assert!(dest_dir.path().join("DESIGN.md").is_file());
    assert!(dest_dir.path().join("TODOS.yaml").is_file());
    assert!(dest_dir.path().join("IMPL.md").is_file());
}

#[test]
fn test_import_without_artifacts_rust_project() {
    let (_repo_dir, repo_url) = create_local_git_repo(false, true);
    let dest_dir = TempDir::new().expect("create dest dir");

    let outcome = import(&repo_url, "main", None, dest_dir.path()).expect("import rust project");
    assert_eq!(
        outcome,
        ImportOutcome::ImportedAndConverted {
            dest_dir: dest_dir.path().to_path_buf()
        }
    );

    assert!(dest_dir.path().join(".kvist/REQUIREMENTS.md").is_file());
    assert!(dest_dir.path().join(".kvist/CONTRACT.md").is_file());
    assert!(dest_dir.path().join(".kvist/DESIGN.md").is_file());
    assert!(dest_dir.path().join(".kvist/TODOS.yaml").is_file());
    assert!(dest_dir.path().join(".kvist/IMPL.md").is_file());
}

#[test]
fn test_import_without_artifacts_generic_project() {
    let (_repo_dir, repo_url) = create_local_git_repo(false, false);
    let dest_dir = TempDir::new().expect("create dest dir");

    let outcome = import(&repo_url, "main", None, dest_dir.path()).expect("import generic project");
    assert_eq!(
        outcome,
        ImportOutcome::ImportedAndInitialized {
            dest_dir: dest_dir.path().to_path_buf()
        }
    );

    assert!(dest_dir.path().join("src/REQUIREMENTS.md").is_file());
    assert!(dest_dir.path().join("src/CONTRACT.md").is_file());
    assert!(dest_dir.path().join("src/DESIGN.md").is_file());
    assert!(dest_dir.path().join("src/TODOS.yaml").is_file());
    assert!(dest_dir.path().join("src/IMPL.md").is_file());
}
