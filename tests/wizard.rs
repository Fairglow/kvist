use std::fs;
use std::io::Cursor;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

use kvist::config;
use kvist::wizard::run_wizard;

#[test]
fn test_wizard_ollama_local_config() {
    let project = TempDir::new().expect("create temp dir");
    let project_dir = project.path();

    // Mock inputs:
    // 1. Choose Ollama (Option 3)
    // 2. Enter Ollama base URL (Default)
    // 3. Enter Ollama model name (llama3.1:8b)
    // 4. Skip the optional model test
    // 5. Choose All Roles (Option 4)
    // 6. Choose Project-local configuration (Option 1)
    let mock_input = "3\n\nllama3.1:8b\n\nn\n4\n1\n";
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    let result = run_wizard(&mut reader, &mut writer, project_dir);
    assert!(result.is_ok());

    let output_str = String::from_utf8(writer).expect("valid utf-8 output");
    assert!(output_str.contains("Kvist Agent Setup Wizard"));
    assert!(output_str.contains("Successfully configured model"));

    // Verify written file
    let toml_path = project_dir.join("kvist.toml");
    assert!(toml_path.is_file());
    let toml_content = fs::read_to_string(&toml_path).expect("read toml");

    assert!(toml_content.contains("[agent.profiles.developer]"));
    assert!(toml_content.contains("[agent.profiles.architect]"));
    assert!(toml_content.contains("[agent.profiles.security-reviewer]"));
    assert!(toml_content.contains("ollama run llama3.1:8b"));
}

#[test]
fn test_wizard_custom_script_local_config() {
    let project = TempDir::new().expect("create temp dir");
    let project_dir = project.path();

    // Create a dummy wrapper script
    let script_path = project_dir.join("my-llama-cli.sh");
    fs::write(&script_path, "#!/bin/sh\necho 'mock'").expect("write script");
    #[cfg(unix)]
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make script executable");

    // Mock inputs:
    // 1. Choose Custom Wrapper Script (Option 6)
    // 2. Enter script path
    // 3. Name the model
    // 4. Keep the default command template
    // 5. Skip the optional model test
    // 6. Choose Developer Role (Option 1 / default)
    // 7. Choose Project-local configuration (Option 1)
    let mock_input = format!("6\n{}\nlocal-wrapper\n\nn\n1\n1\n", script_path.display());
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    let result = run_wizard(&mut reader, &mut writer, project_dir);
    assert!(result.is_ok());

    let toml_path = project_dir.join("kvist.toml");
    assert!(toml_path.is_file());
    let toml_content = fs::read_to_string(&toml_path).expect("read toml");

    assert!(toml_content.contains("[agent.profiles.developer]"));
    assert!(toml_content.contains("local-wrapper"));
    assert!(toml_content.contains("my-llama-cli.sh"));
}

#[test]
fn wizard_merges_a_model_into_existing_configuration() {
    let project = TempDir::new().expect("create temp dir");
    let project_dir = project.path();
    fs::write(
        project_dir.join("kvist.toml"),
        r#"# Keep this project configuration comment.
schema_version = 1
component_root = "components"
custom_setting = "preserved"

[agent]
profiles = { developer = { model = "existing", default_model = "existing", models = [{ name = "existing", command = "existing-agent '{prompt}'" }] } }
"#,
    )
    .expect("write existing configuration");
    let script_path = project_dir.join("provider.sh");
    fs::write(&script_path, "#!/bin/sh\nexit 0\n").expect("write provider");
    #[cfg(unix)]
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mock_input = format!("6\n{}\nnew-model\n\nn\n1\n1\n", script_path.display());
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    run_wizard(&mut reader, &mut writer, project_dir).expect("update configuration");

    let path = project_dir.join("kvist.toml");
    let contents = fs::read_to_string(&path).expect("read updated configuration");
    assert!(contents.contains("# Keep this project configuration comment."));
    assert!(contents.contains("custom_setting = \"preserved\""));
    assert!(contents.contains("name = \"existing\""));
    assert!(contents.contains("name = \"new-model\""));

    let parsed = config::load(project_dir).expect("load updated configuration");
    assert_eq!(
        parsed.component_root,
        std::path::PathBuf::from("components")
    );
    assert_eq!(parsed.agent.developer.model.as_deref(), Some("new-model"));
    assert_eq!(parsed.agent.developer.models.len(), 2);
}

#[cfg(unix)]
#[test]
fn wizard_tests_a_model_before_persisting_it() {
    let project = TempDir::new().expect("create temp dir");
    let script_path = project.path().join("provider.sh");
    fs::write(
        &script_path,
        "#!/bin/sh\nprintf 'verified model: %s\\n' \"$*\"\n",
    )
    .expect("write provider");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mock_input = format!(
        "6\n{}\nverified\n\ny\nConnection test\n1\n1\n",
        script_path.display()
    );
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    run_wizard(&mut reader, &mut writer, project.path()).expect("test and save model");

    let output = String::from_utf8(writer).expect("UTF-8 wizard output");
    assert!(output.contains("Model test succeeded"));
    assert!(project.path().join("kvist.toml").is_file());
}

#[cfg(unix)]
#[test]
fn wizard_does_not_persist_a_failed_model_test_without_confirmation() {
    let project = TempDir::new().expect("create temp dir");
    let script_path = project.path().join("provider.sh");
    fs::write(&script_path, "#!/bin/sh\nexit 7\n").expect("write provider");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mock_input = format!(
        "6\n{}\nfailing\n\ny\nConnection test\nn\n",
        script_path.display()
    );
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    let error = run_wizard(&mut reader, &mut writer, project.path())
        .expect_err("failed model test must stop setup");
    assert!(error.to_string().contains("model verification failed"));
    assert!(!project.path().join("kvist.toml").exists());
}

#[test]
fn wizard_rejects_invalid_existing_configuration_before_editing() {
    let project = TempDir::new().expect("create temp dir");
    let config_path = project.path().join("kvist.toml");
    let original = r#"schema_version = 1
component_root = "src"

[agent.profiles.developer]
models = []
"#;
    fs::write(&config_path, original).expect("write invalid configuration");
    let script_path = project.path().join("provider.sh");
    fs::write(&script_path, "#!/bin/sh\nexit 0\n").expect("write provider");
    #[cfg(unix)]
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mock_input = format!("6\n{}\nnew-model\n\nn\n1\n1\n", script_path.display());
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    run_wizard(&mut reader, &mut writer, project.path())
        .expect_err("invalid configuration must not be repaired");
    assert_eq!(
        fs::read_to_string(config_path).expect("read unchanged configuration"),
        original
    );
}
