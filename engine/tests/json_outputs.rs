use std::process::Command;
use tempfile::TempDir;

#[test]
fn global_json_flag_generates_structured_outputs() {
    let project = TempDir::new().expect("project");

    // 1. Test init --json
    let init_output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args([
            "--json",
            "init",
            project.path().to_str().expect("UTF-8 project path"),
        ])
        .output()
        .expect("run init --json");
    assert!(init_output.status.success());
    let init_stdout = String::from_utf8(init_output.stdout).expect("UTF-8 text output");
    assert!(init_stdout.contains("\"status\":\"success\""));
    assert!(init_stdout.contains("\"command\":\"init\""));
    assert!(init_stdout.contains("\"project_path\""));

    // 2. Test status --json
    let status_output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args([
            "--json",
            "status",
            project.path().to_str().expect("UTF-8 project path"),
        ])
        .output()
        .expect("run status --json");
    assert!(status_output.status.success());
    let status_stdout = String::from_utf8(status_output.stdout).expect("UTF-8 text output");
    assert!(status_stdout.contains("\"format_version\":1"));
    assert!(status_stdout.contains("\"project_state\":\"current\""));

    // 3. Test tree --json
    let tree_output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args([
            "--json",
            "tree",
            project.path().to_str().expect("UTF-8 project path"),
        ])
        .output()
        .expect("run tree --json");
    assert!(tree_output.status.success());
    let tree_stdout = String::from_utf8(tree_output.stdout).expect("UTF-8 text output");
    assert!(tree_stdout.contains("\"status\":\"success\""));
    assert!(tree_stdout.contains("\"command\":\"tree\""));
    assert!(tree_stdout.contains("\"components\""));

    // 4. Test component validate --json
    let validate_output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args([
            "--json",
            "component",
            "validate",
            project
                .path()
                .join("src")
                .to_str()
                .expect("UTF-8 component path"),
        ])
        .output()
        .expect("run component validate --json");
    assert!(validate_output.status.success());
    let validate_stdout = String::from_utf8(validate_output.stdout).expect("UTF-8 text output");
    assert!(validate_stdout.contains("\"status\":\"success\""));
    assert!(validate_stdout.contains("\"command\":\"component-validate\""));
    assert!(validate_stdout.contains("\"valid\":true"));
}

#[test]
fn invalid_component_documents_return_structured_json_failure() {
    let component = TempDir::new().expect("component");
    std::fs::write(component.path().join("REQUIREMENTS.md"), "# invalid\n")
        .expect("write invalid requirements");
    std::fs::write(component.path().join("CONTRACT.md"), "# invalid\n")
        .expect("write invalid contract");
    std::fs::write(component.path().join("DESIGN.md"), "# invalid\n")
        .expect("write invalid design");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args([
            "--json",
            "component",
            "validate",
            component.path().to_str().expect("UTF-8 component path"),
        ])
        .output()
        .expect("run component validation");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let response: serde_json::Value =
        serde_json::from_slice(&output.stderr).expect("structured JSON error");
    assert_eq!(response["status"], "error");
    assert_eq!(response["command"], "component-validate");
    assert_eq!(response["valid"], false);
    assert!(
        response["diagnostics"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
}
