use std::fs;
use std::io::Cursor;
use tempfile::TempDir;

use kvist::wizard::run_wizard;

#[test]
fn test_wizard_ollama_local_config() {
    let project = TempDir::new().expect("create temp dir");
    let project_dir = project.path();

    // Mock inputs:
    // 1. Choose Ollama (Option 3)
    // 2. Enter Ollama base URL (Default)
    // 3. Enter Ollama model name (llama3.1:8b)
    // 4. Choose All Roles (Option 4)
    // 5. Choose Project-local configuration (Option 1)
    let mock_input = "3\n\nllama3.1:8b\n4\n1\n";
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    let result = run_wizard(&mut reader, &mut writer, project_dir);
    assert!(result.is_ok());

    let output_str = String::from_utf8(writer).expect("valid utf-8 output");
    assert!(output_str.contains("Kvist Agent Setup Wizard"));
    assert!(output_str.contains("Successfully configured and saved model configuration"));

    // Verify written file
    let toml_path = project_dir.join("kvist.toml");
    assert!(toml_path.is_file());
    let toml_content = fs::read_to_string(&toml_path).expect("read toml");

    assert!(toml_content.contains("[agent.profiles.developer]"));
    assert!(toml_content.contains("[agent.profiles.architect]"));
    assert!(toml_content.contains("[agent.profiles.security_reviewer]"));
    assert!(toml_content.contains("ollama run --stream=false llama3.1:8b"));
}

#[test]
fn test_wizard_custom_script_local_config() {
    let project = TempDir::new().expect("create temp dir");
    let project_dir = project.path();

    // Create a dummy wrapper script
    let script_path = project_dir.join("my-llama-cli.sh");
    fs::write(&script_path, "#!/bin/sh\necho 'mock'").expect("write script");

    // Mock inputs:
    // 1. Choose Custom Wrapper Script (Option 6)
    // 2. Enter script path
    // 3. Choose Developer Role (Option 1 / default)
    // 4. Choose Project-local configuration (Option 1)
    let mock_input = format!("6\n{}\n1\n1\n", script_path.display());
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    let result = run_wizard(&mut reader, &mut writer, project_dir);
    assert!(result.is_ok());

    let toml_path = project_dir.join("kvist.toml");
    assert!(toml_path.is_file());
    let toml_content = fs::read_to_string(&toml_path).expect("read toml");

    assert!(toml_content.contains("[agent.profiles.developer]"));
    assert!(toml_content.contains("custom-wrapper"));
    assert!(toml_content.contains("my-llama-cli.sh"));
}
