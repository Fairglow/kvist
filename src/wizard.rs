//! Interactive agent model setup wizard.

use std::{
    fs,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    process::Command,
};

use crate::{KvistError, Result, file_io::write_new_file_atomically};

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
    })?;
    Ok(())
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
    Ok(buffer)
}

/// Runs the interactive CLI wizard, prompting the user for provider, binary path/endpoints, and roles.
pub fn run_wizard<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    project_dir: &Path,
) -> Result<()> {
    write_output(
        writer,
        "==================================================\n",
    )?;
    write_output(
        writer,
        "          Kvist Agent Setup Wizard                \n",
    )?;
    write_output(
        writer,
        "==================================================\n",
    )?;
    write_output(
        writer,
        "This wizard guides you through configuring and testing local/remote LLM\n",
    )?;
    write_output(writer, "agent model profiles for Kvist.\n\n")?;

    // 1. Select Model Provider
    write_output(writer, "Select model provider:\n")?;
    write_output(writer, "  1) Local llama-cli (direct binary execution)\n")?;
    write_output(
        writer,
        "  2) Local llama-server (HTTP API - default localhost:8080)\n",
    )?;
    write_output(writer, "  3) Ollama (HTTP API - default localhost:11434)\n")?;
    write_output(writer, "  4) Copilot (CLI wrapper)\n")?;
    write_output(writer, "  5) Gemini (CLI wrapper)\n")?;
    write_output(
        writer,
        "  6) Custom Wrapper Script (e.g., ~/bin/llama-cli.sh)\n",
    )?;
    write_output(writer, "Choose (1-6) [3]: ")?;

    let choice_raw = read_input(reader)?;
    let choice = choice_raw.trim();
    let provider = match choice {
        "1" => "llama-cli",
        "2" => "llama-server",
        "4" => "copilot",
        "5" => "gemini-cli",
        "6" => "custom-script",
        _ => "ollama", // Default to Ollama
    };

    write_output(writer, &format!("\nConfiguring provider: {provider}\n"))?;

    // 2. Resolve Binary Path, Endpoint, or Custom Script
    let command_template: String;
    let model_name: String;

    match provider {
        "llama-cli" => {
            write_output(
                writer,
                "Enter absolute path to llama-cli binary [llama-cli]: ",
            )?;
            let bin_raw = read_input(reader)?;
            let mut binary_path = bin_raw.trim().to_owned();
            if binary_path.is_empty() {
                binary_path = "llama-cli".to_owned();
            }

            write_output(writer, "Enter path to GGUF model file: ")?;
            let model_raw = read_input(reader)?;
            let model_path = expand_home_path(model_raw.trim());

            command_template = format!(
                "{} --model {} --prompt '{{prompt}}' --context '{{context_files}}' --format json",
                binary_path,
                model_path.display()
            );
            model_name = "llama-cli".to_owned();
        }
        "llama-server" => {
            write_output(
                writer,
                "Enter llama-server base URL [http://localhost:8080]: ",
            )?;
            let url_raw = read_input(reader)?;
            let mut url = url_raw.trim().to_owned();
            if url.is_empty() {
                url = "http://localhost:8080".to_owned();
            }

            write_output(writer, &format!("Probing endpoint `{url}` using curl...\n"))?;
            let is_alive = probe_endpoint(&url);
            if is_alive {
                write_output(writer, "✔ Endpoint is responsive!\n")?;
            } else {
                write_output(
                    writer,
                    "⚠ Warning: Endpoint is unresponsive. Make sure the server is running.\n",
                )?;
            }

            command_template = format!(
                "curl -s -X POST {url}/v1/chat/completions -H 'Content-Type: application/json' -d '{{\"messages\": [{{\"role\": \"user\", \"content\": \"{{prompt}}\"}}]}}'"
            );
            model_name = "llama-server".to_owned();
        }
        "ollama" => {
            write_output(writer, "Enter Ollama base URL [http://localhost:11434]: ")?;
            let url_raw = read_input(reader)?;
            let mut url = url_raw.trim().to_owned();
            if url.is_empty() {
                url = "http://localhost:11434".to_owned();
            }

            write_output(
                writer,
                &format!("Probing Ollama endpoint `{url}/api/tags` using curl...\n"),
            )?;
            let is_alive = probe_endpoint(&format!("{url}/api/tags"));
            if is_alive {
                write_output(writer, "✔ Ollama is responsive!\n")?;
            } else {
                write_output(
                    writer,
                    "⚠ Warning: Ollama is unresponsive. Make sure Ollama daemon is running.\n",
                )?;
            }

            write_output(writer, "Enter Ollama model name [llama3.1:8b]: ")?;
            let model_raw = read_input(reader)?;
            let mut model = model_raw.trim().to_owned();
            if model.is_empty() {
                model = "llama3.1:8b".to_owned();
            }

            command_template = format!(
                "ollama run --stream=false {model} --prompt '{{prompt}}' --context '{{context_files}}'"
            );
            model_name = model;
        }
        "copilot" => {
            write_output(
                writer,
                "Enter Copilot CLI command template [copilot chat '{{prompt}}']: ",
            )?;
            let tmpl_raw = read_input(reader)?;
            let mut tmpl = tmpl_raw.trim().to_owned();
            if tmpl.is_empty() {
                tmpl = "copilot chat '{prompt}'".to_owned();
            }
            command_template = tmpl;
            model_name = "copilot".to_owned();
        }
        "gemini-cli" => {
            write_output(
                writer,
                "Enter Gemini-CLI command template [gemini-cli --prompt '{{prompt}}' --files {{context_files}}]: ",
            )?;
            let tmpl_raw = read_input(reader)?;
            let mut tmpl = tmpl_raw.trim().to_owned();
            if tmpl.is_empty() {
                tmpl = "gemini-cli --prompt '{prompt}' --files {context_files}".to_owned();
            }
            command_template = tmpl;
            model_name = "gemini".to_owned();
        }
        "custom-script" | _ => {
            write_output(
                writer,
                "Enter path to your custom wrapper script (e.g., ~/bin/llama-cli.sh): ",
            )?;
            let script_raw = read_input(reader)?;
            let script_path = expand_home_path(script_raw.trim());

            if script_path.exists() {
                write_output(writer, "✔ Script found!\n")?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let metadata = fs::metadata(&script_path).map_err(|source| KvistError::Io {
                        operation: "read script metadata",
                        path: script_path.clone(),
                        source,
                    })?;
                    if metadata.permissions().mode() & 0o111 == 0 {
                        write_output(
                            writer,
                            "⚠ Warning: Script file is not executable. Run `chmod +x` on it.\n",
                        )?;
                    }
                }
            } else {
                write_output(
                    writer,
                    "⚠ Warning: Script file does not exist. Ensure the path is correct.\n",
                )?;
            }

            command_template = format!(
                "{} --prompt '{{prompt}}' --context '{{context_files}}'",
                script_path.display().to_string().replace('\\', "/")
            );
            model_name = "custom-wrapper".to_owned();
        }
    }

    // 3. Select Role Assignment
    write_output(writer, "\nWhich roles should use this model?\n")?;
    write_output(writer, "  1) Developer (test writing & implementation)\n")?;
    write_output(writer, "  2) Architect (compliance reviews)\n")?;
    write_output(writer, "  3) Security Reviewer (security audits)\n")?;
    write_output(writer, "  4) All roles\n")?;
    write_output(writer, "Choose (1-4) [1]: ")?;

    let role_raw = read_input(reader)?;
    let role_choice = role_raw.trim();
    let roles = match role_choice {
        "2" => vec!["architect"],
        "3" => vec!["security_reviewer"],
        "4" => vec!["developer", "architect", "security_reviewer"],
        _ => vec!["developer"], // Default to developer
    };

    // 4. Save Location Choice
    write_output(writer, "\nWhere should this configuration be saved?\n")?;
    write_output(writer, "  1) Project-local configuration (kvist.toml)\n")?;
    write_output(
        writer,
        "  2) Global user-specific configuration (~/.config/kvist/config.toml)\n",
    )?;
    write_output(writer, "Choose (1-2) [1]: ")?;

    let save_raw = read_input(reader)?;
    let save_choice = save_raw.trim();
    let config_path = if save_choice == "2" {
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            KvistError::ImportFailed {
                reason: "cannot resolve user home directory".to_owned(),
            }
        })?;
        let config_dir = home.join(".config").join("kvist");
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).map_err(|source| KvistError::Io {
                operation: "create global config directory",
                path: config_dir.clone(),
                source,
            })?;
        }
        config_dir.join("config.toml")
    } else {
        project_dir.join("kvist.toml")
    };

    // 5. Build and Write TOML Config
    let mut toml_lines = Vec::new();
    if config_path.exists() {
        let existing = fs::read_to_string(&config_path).unwrap_or_default();
        toml_lines.push(existing);
    } else {
        toml_lines.push("schema_version = 1\ncomponent_root = \"src\"\n".to_owned());
    }

    toml_lines.push("\n[agent.profiles]".to_owned());
    for role in &roles {
        toml_lines.push(format!("\n[agent.profiles.{role}]"));
        toml_lines.push(format!("default_model = \"{model_name}\""));
        toml_lines.push(format!("model = \"{model_name}\""));
        toml_lines.push(format!("[[agent.profiles.{role}.models]]"));
        toml_lines.push(format!("name = \"{model_name}\""));
        toml_lines.push(format!(
            "command = \"{}\"",
            command_template.replace('"', "\\\"")
        ));
    }

    let final_toml = toml_lines.join("\n") + "\n";
    write_new_file_atomically(&config_path, &final_toml)?;

    write_output(
        writer,
        &format!(
            "\n✔ Successfully configured and saved model configuration to {}!\n",
            config_path.display()
        ),
    )?;
    write_output(
        writer,
        "==================================================\n",
    )?;

    Ok(())
}

fn expand_home_path(path_str: &str) -> PathBuf {
    if path_str.starts_with("~/") || path_str == "~" {
        if let Some(home_str) = std::env::var_os("HOME") {
            let home = Path::new(&home_str);
            if path_str == "~" {
                return home.to_path_buf();
            } else {
                return home.join(&path_str[2..]);
            }
        }
    }
    PathBuf::from(path_str)
}

fn probe_endpoint(url: &str) -> bool {
    let output = Command::new("curl")
        .args(["--silent", "--fail", "--max-time", "2", url])
        .output();
    match output {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}
