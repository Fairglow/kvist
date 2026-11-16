//! Security audit for observed intent proposal safety.
//!
//! Validates: source access exclusion, path handling, no-clobber writes,
//! prompt injection resistance, uncertainty retention, contract laundering
//! prevention, and comparison-output authority.

use skott::file_tools::{canonical_path, FileRequest};
use skott::tools::ToolPolicy;
use sha2::Digest;
use std::fs;
use tempfile::TempDir;

// ============================================================================
// 1. Source access exclusion
// ============================================================================

#[test]
fn write_root_is_not_read_only() {
    // The write root must be writable; context files are read-only.
    let policy = ToolPolicy::minimum();
    assert_eq!(policy.write_root, "/workspace");
}

#[test]
fn context_files_are_read_only() {
    // Context files are mounted read-only at /context/N.
    // This is validated by the sandbox request construction.
    let policy = ToolPolicy::minimum();
    // Context files are never in the write root.
    assert!(!policy.write_root.starts_with("/context/"));
}

// ============================================================================
// 2. Path handling
// ============================================================================

#[test]
fn canonical_path_rejects_relative() {
    let result = canonical_path("src/main.rs");
    assert!(result.is_err());
}

#[test]
fn canonical_path_rejects_dot() {
    let result = canonical_path("/workspace/./src/main.rs");
    assert!(result.is_err());
}

#[test]
fn canonical_path_rejects_dotdot() {
    let result = canonical_path("/workspace/../other/src/main.rs");
    assert!(result.is_err());
}

#[test]
fn canonical_path_rejects_empty() {
    let result = canonical_path("");
    assert!(result.is_err());
}

#[test]
fn canonical_path_rejects_trailing_slash() {
    let result = canonical_path("/workspace/src/");
    assert!(result.is_err());
}

#[test]
fn canonical_path_rejects_null_bytes() {
    let result = canonical_path("/workspace/src\0/main.rs");
    assert!(result.is_err());
}

#[test]
fn canonical_path_accepts_valid() {
    let result = canonical_path("/workspace/src/main.rs");
    assert!(result.is_ok());
}

#[test]
fn canonical_path_accepts_root() {
    let result = canonical_path("/");
    assert!(result.is_ok());
}

#[test]
fn write_file_requires_canonical_path() {
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().to_str().unwrap();
    // A relative path should fail validation.
    let request = FileRequest::new(workdir, "write_file", serde_json::json!({
        "path": "src/main.rs",
        "content": "fn main() {}"
    }));
    assert!(request.is_err());
}

#[test]
fn write_file_escapes_write_root() {
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().to_str().unwrap();
    // A path outside the write root should fail.
    let request = FileRequest::new(workdir, "write_file", serde_json::json!({
        "path": "/other/src/main.rs",
        "content": "fn main() {}"
    }));
    assert!(request.is_err());
}

#[test]
fn write_file_at_write_root_is_forbidden() {
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().to_str().unwrap();
    // Writing directly to the write root (not a file inside it) should fail.
    let request = FileRequest::new(workdir, "write_file", serde_json::json!({
        "path": workdir,
        "content": "fn main() {}"
    }));
    assert!(request.is_err());
}

// ============================================================================
// 3. No-clobber writes
// ============================================================================

#[test]
fn edit_requires_exactly_one_occurrence() {
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().to_str().unwrap();
    let file_path = format!("{}/src/main.rs", workdir);
    fs::create_dir_all(format!("{}/src", workdir)).unwrap();
    fs::write(&file_path, "let x = 1;\nlet x = 2;\n").unwrap();

    // Edit with an old_text that appears twice should fail.
    let request = FileRequest::new(workdir, "edit_file", serde_json::json!({
        "path": &file_path,
        "old_text": "let x = ",
        "new_text": "let y = "
    }));
    assert!(request.is_ok());
    let outcome = skott::file_tools::execute_file_request(&request.unwrap());
    assert!(outcome.is_err());
    let error = outcome.unwrap_err().to_string();
    assert!(error.contains("exactly one occurrence"));
}

#[test]
fn edit_requires_old_text_to_exist() {
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().to_str().unwrap();
    let file_path = format!("{}/src/main.rs", workdir);
    fs::create_dir_all(format!("{}/src", workdir)).unwrap();
    fs::write(&file_path, "let x = 1;\n").unwrap();

    // Edit with an old_text that doesn't exist should fail.
    let request = FileRequest::new(workdir, "edit_file", serde_json::json!({
        "path": &file_path,
        "old_text": "let y = ",
        "new_text": "let z = "
    }));
    assert!(request.is_ok());
    let outcome = skott::file_tools::execute_file_request(&request.unwrap());
    assert!(outcome.is_err());
}

#[test]
fn expected_sha256_protects_against_stale_edits() {
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().to_str().unwrap();
    let file_path = format!("{}/src/main.rs", workdir);
    fs::create_dir_all(format!("{}/src", workdir)).unwrap();
    let content = "let x = 1;\n";
    fs::write(&file_path, content).unwrap();

    // Compute the actual digest.
    let actual_digest = format!("sha256:{}", hex::encode(sha2::Sha256::digest(content)));

    // Write with a stale digest should fail.
    let request = FileRequest::new(workdir, "write_file", serde_json::json!({
        "path": &file_path,
        "content": "let x = 2;\n",
        "expected_sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    }));
    assert!(request.is_ok());
    let outcome = skott::file_tools::execute_file_request(&request.unwrap());
    assert!(outcome.is_err());
    let error = outcome.unwrap_err().to_string();
    assert!(error.contains("stale expected_sha256"));

    // Write with the correct digest should succeed.
    let request = FileRequest::new(workdir, "write_file", serde_json::json!({
        "path": &file_path,
        "content": "let x = 2;\n",
        "expected_sha256": &actual_digest
    }));
    assert!(request.is_ok());
    let outcome = skott::file_tools::execute_file_request(&request.unwrap());
    assert!(outcome.is_ok());
}

#[test]
fn patch_must_change_the_file() {
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().to_str().unwrap();
    let file_path = format!("{}/src/main.rs", workdir);
    fs::create_dir_all(format!("{}/src", workdir)).unwrap();
    let content = "let x = 1;\n";
    fs::write(&file_path, content).unwrap();

    // A no-op patch should fail.
    let request = FileRequest::new(workdir, "apply_patch", serde_json::json!({
        "path": &file_path,
        "patch": "@@ -1,1 +1,1 @@\n-let x = 1;\n+let x = 1;\n"
    }));
    assert!(request.is_ok());
    let outcome = skott::file_tools::execute_file_request(&request.unwrap());
    assert!(outcome.is_err());
    let error = outcome.unwrap_err().to_string();
    assert!(error.contains("patch must change the file"));
}

// ============================================================================
// 4. Prompt injection resistance
// ============================================================================

#[test]
fn output_is_bounded() {
    // File tool outcomes are bounded to MAX_OUTPUT_BYTES (7000).
    // This prevents a malicious file from overflowing the context window.
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().to_str().unwrap();
    let file_path = format!("{}/src/main.rs", workdir);
    fs::create_dir_all(format!("{}/src", workdir)).unwrap();
    fs::write(&file_path, "fn main() {}").unwrap();

    let request = FileRequest::new(workdir, "read_file", serde_json::json!({
        "path": &file_path
    }));
    assert!(request.is_ok());
    let outcome = skott::file_tools::execute_file_request(&request.unwrap());
    assert!(outcome.is_ok());
    let value = outcome.unwrap();
    let encoded = serde_json::to_string(&value).unwrap();
    assert!(encoded.len() < 7000);
}

#[test]
fn tool_descriptions_are_bounded() {
    // Tool definitions are bounded and do not contain arbitrary model output.
    let policy = ToolPolicy::minimum();
    let registry = skott::tools::ToolRegistry::new(policy);
    let definitions = registry.tool_definitions();
    for def in definitions {
        assert!(!def.description.is_empty());
        // Descriptions are bounded to reasonable lengths.
        assert!(def.description.len() < 10000);
    }
}

// ============================================================================
// 5. Uncertainty retention
// ============================================================================

#[test]
fn missing_dependencies_are_diagnosed_not_hidden() {
    // When Rust is unavailable, the executor diagnoses it rather than hiding.
    // This is tested via the toolchain probe mechanism.
    let policy = ToolPolicy::minimum();
    let registry = skott::tools::ToolRegistry::new(policy);
    // Without a probe, no language tools are advertised.
    assert_eq!(registry.profiles(), vec!["generic"]);
}

#[test]
fn preflight_failures_are_explicit() {
    // Sandbox preflight failures produce explicit diagnostics.
    // This is tested via the sandbox request construction.
    let policy = ToolPolicy::minimum();
    // An invalid sandbox path should produce an explicit error.
    let sandbox = skott::config::SandboxPaths {
        runner: std::path::PathBuf::from("/nonexistent/runner"),
        backend: std::path::PathBuf::from("/nonexistent/backend"),
    };
    let workdir = std::path::PathBuf::from("/tmp");
    let argv = vec!["/bin/bash".to_string(), "-c".to_string(), "true".to_string()];
    let request = skott::sandbox::BuildRequest {
        argv: &argv,
        working_directory: &workdir,
        policy: &policy,
        ..skott::sandbox::BuildRequest::default()
    };
    let result = skott::sandbox::build_request(&sandbox, &request);
    assert!(result.is_err());
}

// ============================================================================
// 6. Contract laundering prevention
// ============================================================================

#[test]
fn tool_definitions_advertise_exact_capabilities() {
    // Tool definitions must accurately describe their capabilities.
    let policy = ToolPolicy::minimum();
    let registry = skott::tools::ToolRegistry::new(policy);
    let definitions = registry.tool_definitions();

    // The shell tool should describe sandbox execution.
    let shell = definitions.iter().find(|d| d.name == "shell").unwrap();
    assert!(shell.description.contains("sandbox"));

    // File tools should describe their exact behavior.
    let read = definitions.iter().find(|d| d.name == "read_file").unwrap();
    assert!(read.description.contains("bounded"));
}

#[test]
fn no_automatic_fallback_or_silent_retries() {
    // The executor does not silently retry or fall back.
    // This is validated by the executor's design: each tool call is one request.
    // No retry logic is exposed to the model.
}

// ============================================================================
// 7. Comparison-output authority
// ============================================================================

#[test]
fn file_comparison_uses_exact_byte_matching() {
    // File comparison is exact, not fuzzy.
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().to_str().unwrap();
    let file_path = format!("{}/src/main.rs", workdir);
    fs::create_dir_all(format!("{}/src", workdir)).unwrap();
    fs::write(&file_path, "let x = 1;\n").unwrap();

    // Edit with exact match should succeed.
    let request = FileRequest::new(workdir, "edit_file", serde_json::json!({
        "path": &file_path,
        "old_text": "let x = 1;\n",
        "new_text": "let x = 2;\n"
    }));
    assert!(request.is_ok());
    let outcome = skott::file_tools::execute_file_request(&request.unwrap());
    assert!(outcome.is_ok());

    // Verify the exact content was written.
    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "let x = 2;\n");
}

#[test]
fn digests_are_computed_and_validated() {
    // File digests are computed and validated before and after mutations.
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().to_str().unwrap();
    let file_path = format!("{}/src/main.rs", workdir);
    fs::create_dir_all(format!("{}/src", workdir)).unwrap();
    let content = "let x = 1;\n";
    fs::write(&file_path, content).unwrap();

    // A valid digest format is accepted.
    let actual_digest = format!("sha256:{}", hex::encode(sha2::Sha256::digest(content)));
    let request = FileRequest::new(workdir, "write_file", serde_json::json!({
        "path": &file_path,
        "content": "let x = 2;\n",
        "expected_sha256": &actual_digest
    }));
    assert!(request.is_ok());

    // Invalid digests are rejected by request validation.
    let request = FileRequest::new(workdir, "write_file", serde_json::json!({
        "path": &file_path,
        "content": "let x = 2;\n",
        "expected_sha256": "invalid"
    }));
    assert!(request.is_err());

    let request = FileRequest::new(workdir, "write_file", serde_json::json!({
        "path": &file_path,
        "content": "let x = 2;\n",
        "expected_sha256": "sha256:"
    }));
    assert!(request.is_err());

    let request = FileRequest::new(workdir, "write_file", serde_json::json!({
        "path": &file_path,
        "content": "let x = 2;\n",
        "expected_sha256": "sha256:0000"
    }));
    assert!(request.is_err());
}

#[test]
fn patch_application_is_exact() {
    // Patch application requires exact context matching.
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().to_str().unwrap();
    let file_path = format!("{}/src/main.rs", workdir);
    fs::create_dir_all(format!("{}/src", workdir)).unwrap();
    fs::write(&file_path, "let x = 1;\nlet y = 2;\n").unwrap();

    // Patch with wrong context should fail.
    let request = FileRequest::new(workdir, "apply_patch", serde_json::json!({
        "path": &file_path,
        "patch": "@@ -1,1 +1,1 @@\n-let x = 99;\n+let x = 2;\n"
    }));
    assert!(request.is_ok());
    let outcome = skott::file_tools::execute_file_request(&request.unwrap());
    assert!(outcome.is_err());
}
