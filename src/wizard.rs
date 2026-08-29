//! Interactive agent model setup wizard.

use std::{
    fs,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    process::Command,
};

use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value, value};

use crate::{
    KvistError, Result, agent, config,
    file_io::{replace_file_atomically, write_new_file_atomically},
    filesystem::is_link_like,
};

struct ModelSetup {
    name: String,
    command: String,
}

fn write_output<W: Write>(writer: &mut W, data: &str) -> Result<()> {
    writer
        .write_all(data.as_bytes())
        .map_err(|source| KvistError::Io {
            operation: "write setup wizard output",
            path: PathBuf::from("stdout"),
            source,
        })?;
    writer.flush().map_err(|source| KvistError::Io {
        operation: "flush setup wizard output",
        path: PathBuf::from("stdout"),
        source,
    })
}

fn read_input<R: BufRead>(reader: &mut R) -> Result<String> {
    let mut buffer = String::new();
    reader
        .read_line(&mut buffer)
        .map_err(|source| KvistError::Io {
            operation: "read setup wizard input",
            path: PathBuf::from("stdin"),
            source,
        })?;
    Ok(buffer.trim().to_owned())
}

fn prompt_with_default<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
    default: &str,
) -> Result<String> {
    write_output(writer, &format!("{prompt} [{default}]: "))?;
    let value = read_input(reader)?;
    Ok(if value.is_empty() {
        default.to_owned()
    } else {
        value
    })
}

fn prompt_required<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
) -> Result<String> {
    write_output(writer, prompt)?;
    let value = read_input(reader)?;
    if value.is_empty() {
        return Err(KvistError::AgentSetupFailed {
            reason: "required setup value must not be empty".to_owned(),
        });
    }
    Ok(value)
}

fn confirm<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
    default: bool,
) -> Result<bool> {
    write_output(
        writer,
        &format!("{prompt} [{}]: ", if default { "Y/n" } else { "y/N" }),
    )?;
    let answer = read_input(reader)?.to_ascii_lowercase();
    if answer.is_empty() {
        return Ok(default);
    }
    match answer.as_str() {
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => Err(KvistError::AgentSetupFailed {
            reason: format!("expected yes or no, received `{answer}`"),
        }),
    }
}

/// Runs the interactive CLI wizard and safely creates or updates agent models.
pub fn run_wizard<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    project_dir: &Path,
) -> Result<()> {
    write_output(
        writer,
        "==================================================\n\
                  Kvist Agent Setup Wizard                \n\
         ==================================================\n\
         This wizard guides you through configuring and testing local/remote LLM\n\
         agent model profiles for Kvist.\n\n",
    )?;
    write_output(
        writer,
        "Select model provider:\n\
           1) Local llama-cli (direct binary execution)\n\
           2) Local llama-server (HTTP API - default localhost:8080)\n\
           3) Ollama (HTTP API - default localhost:11434)\n\
           4) Copilot (CLI wrapper)\n\
           5) Gemini (CLI wrapper)\n\
           6) Custom Wrapper Script (e.g., ~/bin/llama-cli.sh)\n\
         Choose (1-6) [3]: ",
    )?;
    let provider = match read_input(reader)?.as_str() {
        "1" => "llama-cli",
        "2" => "llama-server",
        "4" => "copilot",
        "5" => "gemini-cli",
        "6" => "custom-script",
        _ => "ollama",
    };
    write_output(writer, &format!("\nConfiguring provider: {provider}\n"))?;

    let model = configure_model(provider, reader, writer)?;
    if confirm(reader, writer, "\nTest this model before saving?", true)? {
        let test_prompt = prompt_with_default(
            reader,
            writer,
            "Test prompt",
            "Reply with: Kvist model ready",
        )?;
        match verify_model(&model, &test_prompt, project_dir, writer) {
            Ok(()) => write_output(writer, "Model test succeeded.\n")?,
            Err(error) => {
                write_output(writer, &format!("Model test failed: {error}\n"))?;
                if !confirm(reader, writer, "Save configuration anyway?", false)? {
                    return Err(KvistError::AgentSetupFailed {
                        reason: "model verification failed; configuration was not changed"
                            .to_owned(),
                    });
                }
            }
        }
    }

    write_output(writer, "\nWhich roles should use this model?\n")?;
    write_output(writer, "  1) Developer (test writing & implementation)\n")?;
    write_output(writer, "  2) Architect (compliance reviews)\n")?;
    write_output(writer, "  3) Security Reviewer (security audits)\n")?;
    write_output(writer, "  4) All roles\n")?;
    write_output(writer, "Choose (1-4) [1]: ")?;
    let roles = match read_input(reader)?.as_str() {
        "2" => vec!["architect"],
        "3" => vec!["security-reviewer"],
        "4" => vec!["developer", "architect", "security-reviewer"],
        _ => vec!["developer"],
    };

    write_output(writer, "\nWhere should this configuration be saved?\n")?;
    write_output(writer, "  1) Project-local configuration (kvist.toml)\n")?;
    write_output(
        writer,
        "  2) Global user-specific configuration (~/.config/kvist/config.toml)\n",
    )?;
    write_output(writer, "Choose (1-2) [1]: ")?;
    let project_local = read_input(reader)? != "2";
    let config_path = if project_local {
        project_dir.join("kvist.toml")
    } else {
        config::global_user_config_path().ok_or_else(|| KvistError::AgentSetupFailed {
            reason: "cannot resolve the user configuration directory".to_owned(),
        })?
    };

    persist_model(&config_path, project_dir, project_local, &roles, &model)?;
    write_output(
        writer,
        &format!(
            "\nSuccessfully configured model `{}` in {}.\n\
             ==================================================\n",
            model.name,
            config_path.display()
        ),
    )
}

fn configure_model<R: BufRead, W: Write>(
    provider: &str,
    reader: &mut R,
    writer: &mut W,
) -> Result<ModelSetup> {
    match provider {
        "llama-cli" => {
            let binary =
                prompt_with_default(reader, writer, "Path to llama-cli executable", "llama-cli")?;
            let model_path = prompt_required(reader, writer, "Path to GGUF model file: ")?;
            let name =
                prompt_with_default(reader, writer, "Model configuration name", "llama-cli")?;
            let default = format!(
                "{} --model {} --prompt '{{prompt}}' --context '{{context_files}}' --format json",
                command_argument(&binary),
                command_argument(&expand_home_path(&model_path).to_string_lossy())
            );
            let command = prompt_with_default(reader, writer, "Command template", &default)?;
            Ok(ModelSetup { name, command })
        }
        "llama-server" => {
            let url = prompt_with_default(
                reader,
                writer,
                "llama-server base URL",
                "http://localhost:8080",
            )?;
            probe_and_report(&url, writer)?;
            let name = prompt_with_default(reader, writer, "Model name", "default")?;
            let default = format!(
                "curl --silent --fail --request POST {url}/v1/chat/completions --json \
                 '{{\"model\":\"{name}\",\"messages\":[{{\"role\":\"user\",\"content\":\"{{prompt}}\"}}]}}'"
            );
            let command = prompt_with_default(reader, writer, "Command template", &default)?;
            Ok(ModelSetup { name, command })
        }
        "ollama" => {
            let url =
                prompt_with_default(reader, writer, "Ollama base URL", "http://localhost:11434")?;
            probe_and_report(&format!("{url}/api/tags"), writer)?;
            let name = prompt_with_default(reader, writer, "Ollama model name", "llama3.1:8b")?;
            let default = format!("ollama run {name} '{{prompt}}'");
            let command = prompt_with_default(reader, writer, "Command template", &default)?;
            Ok(ModelSetup { name, command })
        }
        "copilot" => {
            let name = prompt_with_default(reader, writer, "Model configuration name", "copilot")?;
            let command = prompt_with_default(
                reader,
                writer,
                "Command template",
                "copilot chat '{prompt}'",
            )?;
            Ok(ModelSetup { name, command })
        }
        "gemini-cli" => {
            let name = prompt_with_default(reader, writer, "Model configuration name", "gemini")?;
            let command = prompt_with_default(
                reader,
                writer,
                "Command template",
                "gemini-cli --prompt '{prompt}' --files {context_files}",
            )?;
            Ok(ModelSetup { name, command })
        }
        _ => {
            let script = prompt_required(
                reader,
                writer,
                "Path to custom wrapper executable (e.g., ~/bin/llama-cli.sh): ",
            )?;
            let script_path = expand_home_path(&script);
            validate_custom_executable(&script_path)?;
            write_output(writer, "Custom wrapper found and executable.\n")?;
            let name =
                prompt_with_default(reader, writer, "Model configuration name", "custom-wrapper")?;
            let default = format!(
                "{} --prompt '{{prompt}}' --context '{{context_files}}'",
                command_argument(&script_path.to_string_lossy())
            );
            let command = prompt_with_default(reader, writer, "Command template", &default)?;
            Ok(ModelSetup { name, command })
        }
    }
}

fn command_argument(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn validate_custom_executable(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|source| KvistError::Io {
        operation: "inspect custom wrapper executable",
        path: path.to_path_buf(),
        source,
    })?;
    if is_link_like(&metadata) || !metadata.file_type().is_file() {
        return Err(KvistError::AgentSetupFailed {
            reason: format!(
                "custom wrapper `{}` must be a regular non-link file",
                path.display()
            ),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(KvistError::AgentSetupFailed {
                reason: format!("custom wrapper `{}` is not executable", path.display()),
            });
        }
    }
    Ok(())
}

fn probe_and_report<W: Write>(url: &str, writer: &mut W) -> Result<()> {
    write_output(writer, &format!("Probing `{url}`...\n"))?;
    let responsive = Command::new("curl")
        .args(["--silent", "--fail", "--max-time", "2", url])
        .status()
        .is_ok_and(|status| status.success());
    write_output(
        writer,
        if responsive {
            "Endpoint is responsive.\n"
        } else {
            "Warning: endpoint probe failed; the model test can verify the full command.\n"
        },
    )
}

fn verify_model<W: Write>(
    model: &ModelSetup,
    prompt: &str,
    project_dir: &Path,
    writer: &mut W,
) -> Result<()> {
    write_output(
        writer,
        &format!(
            "Executing model `{}` with the generated command...\n",
            model.name
        ),
    )?;
    let (program, arguments) = agent::split_command(&model.command, prompt, &[], project_dir)?;
    crate::prompt_supervisor::run_supervised_prompt(&program, &arguments, 30, true, 0)
}

fn persist_model(
    config_path: &Path,
    project_dir: &Path,
    project_local: bool,
    roles: &[&str],
    model: &ModelSetup,
) -> Result<()> {
    let (mut document, existing_contents) = load_document(config_path, project_local)?;
    if let Some(contents) = existing_contents.as_deref() {
        if project_local {
            config::validate_project_configuration_contents(config_path, project_dir, contents)?;
        } else {
            config::validate_agent_configuration_contents(config_path, contents)?;
        }
    }
    for role in roles {
        upsert_role_model(&mut document, role, model)?;
    }
    let contents = document.to_string();
    if contents.len() as u64 > config::MAX_CONFIGURATION_BYTES {
        return Err(KvistError::ProjectConfigurationTooLarge {
            path: config_path.to_path_buf(),
            max_bytes: config::MAX_CONFIGURATION_BYTES,
        });
    }
    if project_local {
        config::validate_project_configuration_contents(config_path, project_dir, &contents)?;
    } else {
        config::validate_agent_configuration_contents(config_path, &contents)?;
    }

    let parent = config_path
        .parent()
        .ok_or_else(|| KvistError::AgentSetupFailed {
            reason: format!(
                "configuration path `{}` has no parent",
                config_path.display()
            ),
        })?;
    fs::create_dir_all(parent).map_err(|source| KvistError::Io {
        operation: "create configuration directory",
        path: parent.to_path_buf(),
        source,
    })?;
    if existing_contents.is_some() {
        replace_file_atomically(config_path, &contents)
    } else {
        write_new_file_atomically(config_path, &contents)
    }
}

fn load_document(config_path: &Path, project_local: bool) -> Result<(DocumentMut, Option<String>)> {
    let existing_contents = match fs::symlink_metadata(config_path) {
        Ok(metadata) => {
            if is_link_like(&metadata) || !metadata.file_type().is_file() {
                return Err(KvistError::ProjectConfigurationNotFile {
                    path: config_path.to_path_buf(),
                });
            }
            if metadata.len() > config::MAX_CONFIGURATION_BYTES {
                return Err(KvistError::ProjectConfigurationTooLarge {
                    path: config_path.to_path_buf(),
                    max_bytes: config::MAX_CONFIGURATION_BYTES,
                });
            }
            Some(
                fs::read_to_string(config_path).map_err(|source| KvistError::Io {
                    operation: "read agent configuration",
                    path: config_path.to_path_buf(),
                    source,
                })?,
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect agent configuration",
                path: config_path.to_path_buf(),
                source,
            });
        }
    };
    let contents = existing_contents.as_deref().unwrap_or(if project_local {
        "schema_version = 1\ncomponent_root = \"src\"\n"
    } else {
        ""
    });
    let document = contents.parse::<DocumentMut>().map_err(|error| {
        KvistError::InvalidProjectConfiguration {
            path: config_path.to_path_buf(),
            reason: error.to_string(),
        }
    })?;
    Ok((document, existing_contents))
}

fn ensure_table<'a>(item: &'a mut Item, config_path: &str) -> Result<&'a mut Table> {
    if item.is_none() {
        *item = Item::Table(Table::new());
    }
    if matches!(item, Item::Value(Value::InlineTable(_))) {
        let previous = std::mem::take(item);
        *item =
            previous
                .into_table()
                .map(Item::Table)
                .map_err(|_| KvistError::AgentSetupFailed {
                    reason: format!("`{config_path}` must be a TOML table"),
                })?;
    }
    item.as_table_mut()
        .ok_or_else(|| KvistError::AgentSetupFailed {
            reason: format!("`{config_path}` must be a TOML table"),
        })
}

fn upsert_role_model(document: &mut DocumentMut, role: &str, model: &ModelSetup) -> Result<()> {
    let agent = ensure_table(&mut document["agent"], "agent")?;
    let profiles = ensure_table(&mut agent["profiles"], "agent.profiles")?;
    let role_key = if role == "security-reviewer"
        && profiles.contains_key("security_reviewer")
        && !profiles.contains_key("security-reviewer")
    {
        "security_reviewer"
    } else {
        role
    };
    let profile = ensure_table(
        &mut profiles[role_key],
        &format!("agent.profiles.{role_key}"),
    )?;
    profile["model"] = value(&model.name);
    profile["default_model"] = value(&model.name);
    upsert_model_item(&mut profile["models"], model)
}

fn upsert_model_item(item: &mut Item, model: &ModelSetup) -> Result<()> {
    if item.is_none() {
        *item = Item::ArrayOfTables(ArrayOfTables::new());
    }
    match item {
        Item::ArrayOfTables(models) => {
            if let Some(existing) = models.iter_mut().find(|table| {
                table
                    .get("name")
                    .and_then(Item::as_value)
                    .and_then(Value::as_str)
                    == Some(model.name.as_str())
            }) {
                existing["command"] = value(&model.command);
            } else {
                let mut table = Table::new();
                table["name"] = value(&model.name);
                table["command"] = value(&model.command);
                models.push(table);
            }
            Ok(())
        }
        Item::Value(Value::Array(models)) => upsert_inline_model(models, model),
        _ => Err(KvistError::AgentSetupFailed {
            reason: "`agent profile models` must be an array of tables".to_owned(),
        }),
    }
}

fn upsert_inline_model(models: &mut Array, model: &ModelSetup) -> Result<()> {
    let existing_index = models.iter().position(|value| {
        value
            .as_inline_table()
            .and_then(|table| table.get("name"))
            .and_then(Value::as_str)
            == Some(model.name.as_str())
    });
    if let Some(index) = existing_index {
        let existing = models
            .get_mut(index)
            .and_then(Value::as_inline_table_mut)
            .ok_or_else(|| KvistError::AgentSetupFailed {
                reason: "`agent profile models` must contain only tables".to_owned(),
            })?;
        existing.insert("command", Value::from(&model.command));
    } else {
        if models.iter().any(|value| !value.is_inline_table()) {
            return Err(KvistError::AgentSetupFailed {
                reason: "`agent profile models` must contain only tables".to_owned(),
            });
        }
        let mut table = InlineTable::new();
        table.insert("name", Value::from(&model.name));
        table.insert("command", Value::from(&model.command));
        models.push(table);
    }
    Ok(())
}

fn expand_home_path(path: &str) -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        if let Some(remainder) = path.strip_prefix("~/") {
            return PathBuf::from(home).join(remainder);
        }
        if path == "~" {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(path)
}
