use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use serde_json::Value;

use crate::{
    CommandSpec, Error, ModelProfile, Result, SupervisionPolicy, command::json_string,
    profile::is_valid_profile_name, render_command, run_supervised, run_supervised_capture,
    upsert_profile,
};

const LLAMA_SERVER_DEFAULT_URL: &str = "http://127.0.0.1:9931";
const MAX_DISCOVERY_BYTES: usize = 64 * 1024;
const MAX_DISCOVERED_MODELS: usize = 128;
const MAX_MODEL_ID_BYTES: usize = 256;
const SETUP_TEST_PROMPT: &str = "Reply with exactly: OK";

/// Controls the non-interactive qualification decision made by setup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SetupOptions {
    /// Persist a profile even when its mandatory live qualification fails.
    pub force: bool,
}

/// Collects and optionally verifies one reusable provider profile.
pub fn collect_profile<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    working_directory: &Path,
) -> Result<ModelProfile> {
    collect_profile_with_options(reader, writer, working_directory, SetupOptions::default())
}

/// Collects and qualifies one profile with explicit setup options.
pub fn collect_profile_with_options<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    working_directory: &Path,
    options: SetupOptions,
) -> Result<ModelProfile> {
    write_output(
        writer,
        "Select model provider:\n\
           1) Local llama-cli (direct binary execution)\n\
           2) Local llama-server (HTTP API - default 127.0.0.1:9931)\n\
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

    let profile = configure_profile(provider, reader, writer)?;
    write_output(
        writer,
        &format!("Generated command template: {}\n", profile.command),
    )?;
    write_output(
        writer,
        "\nTesting the generated command with current host permissions and the fixed \
         prompt `Reply with exactly: OK`.\n",
    )?;
    match verify_profile(&profile, SETUP_TEST_PROMPT, working_directory, true) {
        Ok(()) => write_output(writer, "Model test succeeded.\n")?,
        Err(error @ Error::Cancelled) => return Err(error),
        Err(error) if options.force => {
            write_output(
                writer,
                &format!(
                    "Model test failed: {error}\n\
                     Warning: --force permits this failed profile to be saved.\n"
                ),
            )?;
        }
        Err(error) => {
            return Err(Error::ProfileSetup {
                reason: format!(
                    "model verification failed; profile was not saved: {error}; \
                     rerun setup with --force to persist it explicitly"
                ),
            });
        }
    }
    Ok(profile)
}

/// Runs the reusable setup interaction and persists the resulting profile.
pub fn run_setup_wizard<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    working_directory: &Path,
    config_path: &Path,
) -> Result<ModelProfile> {
    run_setup_wizard_with_options(
        reader,
        writer,
        working_directory,
        config_path,
        SetupOptions::default(),
    )
}

/// Runs setup with explicit qualification and persistence options.
pub fn run_setup_wizard_with_options<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    working_directory: &Path,
    config_path: &Path,
    options: SetupOptions,
) -> Result<ModelProfile> {
    write_output(
        writer,
        "==================================================\n\
                 Agent Runtime Setup Wizard              \n\
         ==================================================\n\
         Configure a reusable provider profile for bounded host execution.\n\n",
    )?;
    let profile = collect_profile_with_options(reader, writer, working_directory, options)?;
    upsert_profile(config_path, &profile)?;
    write_output(
        writer,
        &format!(
            "\nSaved profile `{}` in {}.\n\
             ==================================================\n",
            profile.name,
            config_path.display()
        ),
    )?;
    Ok(profile)
}

/// Executes a profile's exact rendered command under bounded supervision.
pub fn verify_profile(
    profile: &ModelProfile,
    prompt: &str,
    working_directory: &Path,
    host_execution_acknowledged: bool,
) -> Result<()> {
    if !host_execution_acknowledged {
        return Err(Error::HostExecutionNotAcknowledged);
    }
    let (program, arguments) = render_command(&profile.command, prompt, &[], working_directory)?;
    let policy = SupervisionPolicy {
        idle_timeout: Duration::from_secs(30),
        attempt_timeout: Some(Duration::from_secs(300)),
        detect_loops: true,
        max_retries: 0,
        max_output_bytes: 64 * 1024,
    };
    run_supervised_capture(&policy, |_| {
        Ok(CommandSpec::new(program.clone(), arguments.clone())
            .in_directory(working_directory.to_path_buf()))
    })?;
    Ok(())
}

fn configure_profile<R: BufRead, W: Write>(
    provider: &str,
    reader: &mut R,
    writer: &mut W,
) -> Result<ModelProfile> {
    let (name, command) = match provider {
        "llama-cli" => {
            let binary = select_cli_executable(
                reader,
                writer,
                "llama-cli",
                "Path to llama-cli or a compatible wrapper executable: ",
            )?;
            let model_path = prompt_required(reader, writer, "Path to GGUF model file: ")?;
            let model_path = resolve_setup_file(&expand_home_path(&model_path), "GGUF model")?;
            let name = prompt_with_default(reader, writer, "Profile name", "llama-cli")?;
            let default = format!(
                "{} --model {} --prompt '{{prompt}}' --single-turn --simple-io \
                 --no-display-prompt --predict 4096",
                command_argument(&binary),
                command_argument(&model_path.to_string_lossy())
            );
            write_output(
                writer,
                "Note: llama-cli performs inference only. It cannot read context files, edit \
                 files, or invoke tools unless your wrapper implements those capabilities.\n",
            )?;
            let command = prompt_with_default(reader, writer, "Command template", &default)?;
            (name, command)
        }
        "llama-server" => {
            let url = prompt_with_default(
                reader,
                writer,
                "llama-server base URL",
                LLAMA_SERVER_DEFAULT_URL,
            )?;
            validate_probe_url(&url)?;
            let url = url.trim_end_matches('/');
            probe_and_report(&format!("{url}/health"), writer)?;
            let model = select_llama_server_model(reader, writer, url)?;
            let profile_default = if is_valid_profile_name(&model) {
                model.as_str()
            } else {
                "llama-server"
            };
            let name = prompt_with_default(reader, writer, "Profile name", profile_default)?;
            let request = format!(
                "{{\"model\":{},\"messages\":[{{\"role\":\"user\",\"content\":{{prompt_json}}}}]}}",
                json_string(&model)
            );
            let endpoint = command_argument(&format!("{url}/v1/chat/completions"));
            let default = format!(
                "curl --disable --silent --show-error --fail-with-body --request POST --json {} -- \
                 {endpoint}",
                command_argument(&request),
            );
            let command = prompt_with_default(reader, writer, "Command template", &default)?;
            (name, command)
        }
        "ollama" => {
            let url =
                prompt_with_default(reader, writer, "Ollama base URL", "http://localhost:11434")?;
            validate_probe_url(&url)?;
            probe_and_report(&format!("{url}/api/tags"), writer)?;
            let name = prompt_with_default(
                reader,
                writer,
                "Ollama model and profile name",
                "llama3.1:8b",
            )?;
            let default = format!(
                "env {} ollama run {} '{{prompt}}'",
                command_argument(&format!("OLLAMA_HOST={url}")),
                command_argument(&name)
            );
            let command = prompt_with_default(reader, writer, "Command template", &default)?;
            (name, command)
        }
        "copilot" => {
            let binary = select_cli_executable(
                reader,
                writer,
                "copilot",
                "Path to the Copilot CLI or a compatible wrapper executable: ",
            )?;
            let model =
                prompt_optional(reader, writer, "Copilot model (blank uses CLI default): ")?;
            let name = prompt_with_default(reader, writer, "Profile name", "copilot")?;
            let mut default = format!(
                "{} --prompt '{{prompt}}' --silent --allow-all-tools --no-ask-user \
                 --reasoning-effort '{{reasoning_effort}}'",
                command_argument(&binary)
            );
            if let Some(model) = model {
                default.push_str(" --model ");
                default.push_str(&command_argument(&model));
            }
            let command = prompt_with_default(reader, writer, "Command template", &default)?;
            (name, command)
        }
        "gemini-cli" => {
            let binary = select_cli_executable(
                reader,
                writer,
                "gemini",
                "Path to the Gemini CLI or a compatible wrapper executable: ",
            )?;
            let model = prompt_optional(reader, writer, "Gemini model (blank uses CLI default): ")?;
            let name = prompt_with_default(reader, writer, "Profile name", "gemini")?;
            let mut default = format!(
                "{} --prompt '{{prompt}}' --output-format text --approval-mode yolo --skip-trust",
                command_argument(&binary)
            );
            if let Some(model) = model {
                default.push_str(" --model ");
                default.push_str(&command_argument(&model));
            }
            let command = prompt_with_default(reader, writer, "Command template", &default)?;
            (name, command)
        }
        _ => {
            let script = prompt_required(
                reader,
                writer,
                "Path to custom wrapper executable (e.g., ~/bin/llama-cli.sh): ",
            )?;
            let script_path = resolve_setup_file(&expand_home_path(&script), "custom wrapper")?;
            validate_custom_executable(&script_path)?;
            write_output(writer, "Custom wrapper found and executable.\n")?;
            let name = prompt_with_default(reader, writer, "Profile name", "custom-wrapper")?;
            let default = format!(
                "{} --prompt '{{prompt}}' --context '{{context_files}}'",
                command_argument(&script_path.to_string_lossy())
            );
            let command = prompt_with_default(reader, writer, "Command template", &default)?;
            (name, command)
        }
    };
    let profile = ModelProfile {
        name,
        provider: provider.to_owned(),
        command,
    };
    crate::profile::validate_profile(&profile)?;
    Ok(profile)
}

fn validate_custom_executable(path: &Path) -> Result<()> {
    validate_regular_file(path, "custom wrapper")?;
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect custom wrapper executable",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(Error::ProfileSetup {
            reason: format!("custom wrapper `{}` is not executable", path.display()),
        });
    }
    Ok(())
}

fn validate_regular_file(path: &Path, description: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect setup file",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(Error::ProfileSetup {
            reason: format!(
                "{description} `{}` must be a regular non-link file",
                path.display()
            ),
        });
    }
    Ok(())
}

fn select_cli_executable<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    conventional_name: &str,
    fallback_prompt: &str,
) -> Result<String> {
    write_output(
        writer,
        &format!("Checking `{conventional_name} --version`...\n"),
    )?;
    match probe_cli_version(conventional_name) {
        Ok(()) => {
            write_output(
                writer,
                &format!("Provider executable `{conventional_name}` is accessible.\n"),
            )?;
            Ok(conventional_name.to_owned())
        }
        Err(error @ Error::Cancelled) => Err(error),
        Err(error) => {
            write_output(
                writer,
                &format!(
                    "`{conventional_name}` is not accessible through PATH: {error}\n\
                     Supply a direct executable or a compatible wrapper instead.\n"
                ),
            )?;
            let path = resolve_setup_file(
                &expand_home_path(&prompt_required(reader, writer, fallback_prompt)?),
                "provider executable",
            )?;
            validate_custom_executable(&path)?;
            let executable = path.to_string_lossy().into_owned();
            match probe_cli_version(&executable) {
                Ok(()) => {}
                Err(error @ Error::Cancelled) => return Err(error),
                Err(error) => {
                    return Err(Error::ProfileSetup {
                        reason: format!(
                            "{conventional_name} version probe failed for `{}`: {error}",
                            path.display()
                        ),
                    });
                }
            }
            write_output(
                writer,
                &format!("Provider executable `{}` is accessible.\n", path.display()),
            )?;
            Ok(executable)
        }
    }
}

fn probe_cli_version(executable: &str) -> Result<()> {
    let policy = SupervisionPolicy {
        idle_timeout: Duration::from_secs(10),
        attempt_timeout: Some(Duration::from_secs(10)),
        detect_loops: false,
        max_retries: 0,
        max_output_bytes: 64 * 1024,
    };
    run_supervised(&policy, |_| Ok(CommandSpec::new(executable, ["--version"])))?;
    Ok(())
}

fn resolve_setup_file(path: &Path, description: &str) -> Result<PathBuf> {
    validate_regular_file(path, description)?;
    let resolved = fs::canonicalize(path).map_err(|source| Error::Io {
        operation: "resolve setup file",
        path: path.to_path_buf(),
        source,
    })?;
    validate_regular_file(&resolved, description)?;
    Ok(resolved)
}

fn probe_and_report<W: Write>(url: &str, writer: &mut W) -> Result<()> {
    write_output(writer, &format!("Probing `{url}`...\n"))?;
    let responsive = Command::new("curl")
        .args([
            "--disable",
            "--silent",
            "--fail",
            "--max-time",
            "2",
            "--",
            url,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
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

fn select_llama_server_model<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    base_url: &str,
) -> Result<String> {
    let models = match discover_llama_server_models(base_url) {
        Ok(models) => models,
        Err(reason) => {
            write_output(
                writer,
                &format!(
                    "Warning: could not list llama-server models ({reason}); \
                     enter a model ID manually.\n"
                ),
            )?;
            Vec::new()
        }
    };

    let model = if models.is_empty() {
        prompt_with_default(reader, writer, "Model ID", "default")?
    } else {
        write_output(writer, "Available llama-server models:\n")?;
        for (index, model) in models.iter().enumerate() {
            write_output(writer, &format!("  {}) {model}\n", index + 1))?;
        }
        let selection = prompt_with_default(
            reader,
            writer,
            "Select model by number, #number, or enter an exact model ID",
            "default",
        )?;
        if models.iter().any(|model| model == &selection) {
            selection
        } else if let Some(index) = selection
            .strip_prefix('#')
            .and_then(|value| value.parse::<usize>().ok())
        {
            if (1..=models.len()).contains(&index) {
                models[index - 1].clone()
            } else {
                return Err(invalid_model_selection(models.len()));
            }
        } else {
            match selection.parse::<usize>() {
                Ok(index) if (1..=models.len()).contains(&index) => models[index - 1].clone(),
                Ok(_) => return Err(invalid_model_selection(models.len())),
                Err(_) => selection,
            }
        }
    };
    validate_model_id(&model)?;
    Ok(model)
}

fn invalid_model_selection(model_count: usize) -> Error {
    Error::ProfileSetup {
        reason: format!(
            "model selection must be an advertised exact ID, a number from 1 to {model_count}, \
             # followed by such a number, or another exact model ID"
        ),
    }
}

fn discover_llama_server_models(base_url: &str) -> std::result::Result<Vec<String>, &'static str> {
    let url = format!("{base_url}/v1/models");
    let output = Command::new("curl")
        .args([
            "--disable",
            "--silent",
            "--fail",
            "--max-time",
            "5",
            "--max-filesize",
            &MAX_DISCOVERY_BYTES.to_string(),
            "--",
            &url,
        ])
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "curl could not be executed")?;
    if !output.status.success() {
        return Err("the model-list endpoint returned a failure");
    }
    if output.stdout.len() > MAX_DISCOVERY_BYTES {
        return Err("the model-list response exceeded 65536 bytes");
    }

    let root = serde_json::from_slice::<Value>(&output.stdout)
        .map_err(|_| "the model-list response was not valid JSON")?;
    let data = root
        .get("data")
        .and_then(Value::as_array)
        .ok_or("the model-list response had no data array")?;
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for entry in data {
        let model = entry
            .get("id")
            .and_then(Value::as_str)
            .ok_or("a model-list entry had no string id")?;
        validate_discovered_model_id(model)?;
        if seen.insert(model.to_owned()) {
            if models.len() >= MAX_DISCOVERED_MODELS {
                return Err("the model-list response exceeded 128 unique models");
            }
            models.push(model.to_owned());
        }
    }
    Ok(models)
}

fn validate_discovered_model_id(model: &str) -> std::result::Result<(), &'static str> {
    if model.is_empty()
        || model.len() > MAX_MODEL_ID_BYTES
        || !model
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'{' | b'}'))
    {
        return Err("a model ID was not 1-256 printable ASCII bytes without braces");
    }
    Ok(())
}

fn validate_model_id(model: &str) -> Result<()> {
    validate_discovered_model_id(model).map_err(|reason| Error::ProfileSetup {
        reason: format!("invalid llama-server model ID: {reason}"),
    })
}

fn validate_probe_url(url: &str) -> Result<()> {
    if url.len() > 2_048
        || !(url.starts_with("http://") || url.starts_with("https://"))
        || url.chars().any(char::is_whitespace)
        || url.chars().any(char::is_control)
    {
        return Err(Error::ProfileSetup {
            reason: "provider base URL must be a bounded HTTP or HTTPS URL without whitespace"
                .to_owned(),
        });
    }
    Ok(())
}

fn write_output<W: Write>(writer: &mut W, data: &str) -> Result<()> {
    writer
        .write_all(data.as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|source| Error::Io {
            operation: "write setup output",
            path: PathBuf::from("setup output"),
            source,
        })
}

fn read_input<R: BufRead>(reader: &mut R) -> Result<String> {
    let mut buffer = String::new();
    reader.read_line(&mut buffer).map_err(|source| Error::Io {
        operation: "read setup input",
        path: PathBuf::from("setup input"),
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
        return Err(Error::ProfileSetup {
            reason: "required setup value must not be empty".to_owned(),
        });
    }
    Ok(value)
}

fn prompt_optional<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
) -> Result<Option<String>> {
    write_output(writer, prompt)?;
    let value = read_input(reader)?;
    Ok((!value.is_empty()).then_some(value))
}

fn command_argument(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
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
