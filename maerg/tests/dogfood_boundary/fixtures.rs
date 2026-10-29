use std::{fs, path::Path, process::Command};

use kvist::{artifacts::root_artifacts, task_queue};

use super::support::*;

const COMPONENT_CONTEXT: [&str; 5] = [
    "REQUIREMENTS.md",
    "CONTRACT.md",
    "DESIGN.md",
    "TODOS.yaml",
    "IMPL.md",
];

fn template(path: &str) -> &'static str {
    root_artifacts()
        .iter()
        .find(|artifact| artifact.relative_path == path)
        .expect("native artifact template")
        .contents
}

#[test]
fn workflow_fixture_context_is_bounded_native_content_under_unchanged_profile_limits() {
    let project = create_target_project("pending");
    for component in [".", "sav", "galla"] {
        let directory = project.path().join("maerg").join(component);
        let mut paths: Vec<_> = COMPONENT_CONTEXT
            .iter()
            .map(|name| directory.join(name))
            .collect();
        paths.push(project.path().join("ROOT_CONTRACT.md"));
        if component != "." {
            paths.push(project.path().join("maerg/CONTRACT.md"));
        }
        let bytes: usize = paths.iter().map(|path| fs::read(path).unwrap().len()).sum();
        assert!(
            bytes <= 16 * 1024,
            "{component} fixture context is {bytes} bytes, above the 16 KiB bound"
        );
        for name in ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md", "IMPL.md"] {
            let contents = fs::read_to_string(directory.join(name)).unwrap();
            let native = template(&format!("src/{name}"));
            if name == "REQUIREMENTS.md" {
                assert!(contents.starts_with(native));
                assert!(contents.contains("### REQ-SUPERVISED-EXECUTION\n"));
            } else {
                assert_eq!(contents, native, "{component}/{name}");
            }
        }
        let queue =
            task_queue::parse(&fs::read_to_string(directory.join("TODOS.yaml")).unwrap()).unwrap();
        assert_eq!(
            queue.component.requirements_revision,
            sha256_file(&directory.join("REQUIREMENTS.md"))
        );
        assert_eq!(
            queue.component.contract_revision,
            sha256_file(&directory.join("CONTRACT.md"))
        );
        assert_eq!(
            queue.component.design_revision,
            sha256_file(&directory.join("DESIGN.md"))
        );
        if component != "." {
            let parent = queue
                .component
                .parent_contract
                .expect("child parent binding");
            assert_eq!(parent.path, "../CONTRACT.md");
            assert_eq!(
                parent.revision,
                sha256_file(&project.path().join("maerg/CONTRACT.md"))
            );
        }
    }
    for name in ["VISION.md", "ARCHITECTURE.md", "ROOT_CONTRACT.md"] {
        assert_eq!(
            fs::read_to_string(project.path().join(name)).unwrap(),
            template(name)
        );
    }
    let profiles: toml::Value =
        toml::from_str(&fs::read_to_string(project.path().join("providers.toml")).unwrap())
            .unwrap();
    assert_eq!(
        profiles["profiles"]["fixture"]["backend"]["context_limit"].as_integer(),
        Some(65_536)
    );
    assert_eq!(
        profiles["profiles"]["fixture"]["backend"]["response_reserve"].as_integer(),
        Some(1_024)
    );
}

#[test]
fn workflow_fixture_creation_does_not_read_repository_authority() {
    if std::env::var("KVIST_DOGFOOD_TEMPLATE_CHILD").as_deref() == Ok("1") {
        let manifest = std::env::var_os("CARGO_MANIFEST_DIR").unwrap();
        assert!(!Path::new(&manifest).join("VISION.md").exists());
        assert!(
            !Path::new(&manifest)
                .parent()
                .unwrap()
                .join("VISION.md")
                .exists()
        );
        let project = create_target_project("pending");
        assert_success(
            &run_kvist(project.path(), &["component", "validate", "."]),
            "validate fixture without repository authority",
        );
        let status = run_kvist(project.path(), &["status", "--format", "json"]);
        assert_success(&status, "inspect isolated fixture");
        let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
        assert_eq!(status["project_state"], "current");
        let components = status["components"].as_array().unwrap();
        assert_eq!(components.len(), 3);
        for component in components {
            assert_eq!(component["state"], "current");
            assert!(
                component["artifacts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|artifact| artifact["state"] == "valid")
            );
        }
        return;
    }
    let unavailable = temporary_directory("dogfood-no-authority-");
    let manifest = unavailable.path().join("maerg");
    fs::create_dir(&manifest).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "fixtures::workflow_fixture_creation_does_not_read_repository_authority",
            "--nocapture",
        ])
        .env("KVIST_DOGFOOD_TEMPLATE_CHILD", "1")
        .env("CARGO_MANIFEST_DIR", manifest)
        .env_remove("KVIST_TEST_MODEL_URL")
        .output()
        .unwrap();
    assert_success(&output, "create workflow fixture with empty manifest root");
}
