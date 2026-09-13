//! Interactive agent model setup wizard.

use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, Write},
    path::{Path, PathBuf},
};

use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value, value};

use crate::{
    KvistError, Result, config,
    file_io::{replace_file_atomically, write_new_file_atomically},
    filesystem::is_link_like,
};

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
    let bytes = reader
        .read_line(&mut buffer)
        .map_err(|source| KvistError::Io {
            operation: "read setup wizard input",
            path: PathBuf::from("stdin"),
            source,
        })?;
    if bytes == 0 {
        return Err(KvistError::AgentSetupCancelled);
    }
    let trimmed = buffer.trim();
    if trimmed == "\x1b"
        || trimmed.eq_ignore_ascii_case("cancel")
        || trimmed.eq_ignore_ascii_case("quit")
        || trimmed.eq_ignore_ascii_case("q")
    {
        return Err(KvistError::AgentSetupCancelled);
    }
    Ok(trimmed.to_owned())
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

/// Runs the interactive CLI wizard and safely creates or updates agent models.
pub fn run_wizard<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    project_dir: &Path,
) -> Result<()> {
    run_wizard_with_force(reader, writer, project_dir, false)
}

/// Runs the setup wizard with an explicit failed-qualification override.
pub fn run_wizard_with_force<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    project_dir: &Path,
    force: bool,
) -> Result<()> {
    let profile_config = agent_runtime::default_profile_config_path();
    run_wizard_inner(
        reader,
        writer,
        project_dir,
        profile_config.as_deref(),
        force,
    )
}

/// Runs setup with an explicit standalone profile store.
pub fn run_wizard_with_profile_config<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    project_dir: &Path,
    profile_config: &Path,
) -> Result<()> {
    run_wizard_inner(reader, writer, project_dir, Some(profile_config), false)
}

fn run_wizard_inner<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    project_dir: &Path,
    profile_config: Option<&Path>,
    force: bool,
) -> Result<()> {
    tracing::info!(
        project_dir = %project_dir.display(),
        "starting interactive agent setup wizard"
    );
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
        "Select profile source:\n\
           1) Configure a provider profile now\n\
           2) Use a saved agent-runtime profile\n\
         Choose (1-2) [1]: ",
    )?;
    let model = if read_input(reader)? == "2" {
        let profile_config = profile_config.ok_or_else(|| KvistError::AgentSetupFailed {
            reason: "cannot resolve standalone profile configuration; set HOME or XDG_CONFIG_HOME"
                .to_owned(),
        })?;
        let profiles = agent_runtime::load_profiles(profile_config)?;
        if profiles.is_empty() {
            return Err(KvistError::AgentSetupFailed {
                reason: format!(
                    "no standalone profiles exist in `{}`; run `agent-run setup` first",
                    profile_config.display()
                ),
            });
        }
        write_output(writer, "\nAvailable standalone profiles:\n")?;
        for profile in &profiles {
            write_output(
                writer,
                &format!("  {} ({})\n", profile.name, profile.provider),
            )?;
        }
        let name = prompt_required(reader, writer, "Profile name: ")?;
        profiles
            .into_iter()
            .find(|profile| profile.name == name)
            .ok_or_else(|| KvistError::AgentSetupFailed {
                reason: format!(
                    "profile `{name}` does not exist in `{}`",
                    profile_config.display()
                ),
            })?
    } else {
        agent_runtime::collect_profile_with_options(
            reader,
            writer,
            project_dir,
            agent_runtime::SetupOptions { force },
        )?
    };

    if let Ok(cfg) = config::load(project_dir) {
        write_output(writer, "\nCurrent role assignments:\n")?;
        let dev_model = cfg
            .agent
            .developer
            .model
            .as_deref()
            .unwrap_or(&cfg.agent.developer.default_model);
        let arch_model = cfg
            .agent
            .architect
            .model
            .as_deref()
            .unwrap_or(&cfg.agent.architect.default_model);
        let sec_model = cfg
            .agent
            .security_reviewer
            .model
            .as_deref()
            .unwrap_or(&cfg.agent.security_reviewer.default_model);
        write_output(writer, &format!("  Developer:         {}\n", dev_model))?;
        write_output(writer, &format!("  Architect:         {}\n", arch_model))?;
        write_output(writer, &format!("  Security Reviewer: {}\n", sec_model))?;
    }

    write_output(writer, "\nWhich roles should use this model?\n")?;
    write_output(
        writer,
        "  0) No roles (save as configured model, unassigned)\n",
    )?;
    write_output(writer, "  1) Developer (test writing & implementation)\n")?;
    write_output(writer, "  2) Architect (compliance reviews)\n")?;
    write_output(writer, "  3) Security Reviewer (security audits)\n")?;
    write_output(writer, "  4) All roles\n")?;
    write_output(writer, "  c) Cancel\n")?;
    write_output(writer, "Choose (0-4) [0]: ")?;
    let input = read_input(reader)?;
    let roles: Vec<&str> = match input.as_str() {
        "1" => vec!["developer"],
        "2" => vec!["architect"],
        "3" => vec!["security-reviewer"],
        "4" => vec!["developer", "architect", "security-reviewer"],
        "c" | "cancel" => return Err(KvistError::AgentSetupCancelled),
        _ => vec![], // default "0", empty, or other: No roles!
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

fn persist_model(
    config_path: &Path,
    project_dir: &Path,
    project_local: bool,
    roles: &[&str],
    model: &agent_runtime::ModelProfile,
) -> Result<()> {
    let (mut document, existing_contents) = load_document(config_path, project_local)?;
    if let Some(contents) = existing_contents.as_deref() {
        if project_local {
            config::validate_project_configuration_contents(config_path, project_dir, contents)?;
        } else {
            config::validate_agent_configuration_contents(config_path, contents)?;
        }
    }
    if roles.is_empty() {
        add_unassigned_model(&mut document, model)?;
    } else {
        for role in roles {
            upsert_role_model(&mut document, role, model)?;
        }
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

fn upsert_role_model(
    document: &mut DocumentMut,
    role: &str,
    model: &agent_runtime::ModelProfile,
) -> Result<()> {
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

fn upsert_model_item(item: &mut Item, model: &agent_runtime::ModelProfile) -> Result<()> {
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

fn upsert_inline_model(models: &mut Array, model: &agent_runtime::ModelProfile) -> Result<()> {
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

fn add_unassigned_model(
    document: &mut DocumentMut,
    model: &agent_runtime::ModelProfile,
) -> Result<()> {
    let agent = ensure_table(&mut document["agent"], "agent")?;
    let profiles = ensure_table(&mut agent["profiles"], "agent.profiles")?;
    let mut updated_any = false;
    for role_key in [
        "developer",
        "architect",
        "security_reviewer",
        "security-reviewer",
    ] {
        if profiles.contains_key(role_key) {
            let profile = ensure_table(
                &mut profiles[role_key],
                &format!("agent.profiles.{role_key}"),
            )?;
            upsert_model_item(&mut profile["models"], model)?;
            updated_any = true;
        }
    }
    if !updated_any {
        let profile = ensure_table(&mut profiles["developer"], "agent.profiles.developer")?;
        upsert_model_item(&mut profile["models"], model)?;
    }
    Ok(())
}

/// Removes a configured model profile from project or user configuration.
pub fn remove_model(
    config_path: &Path,
    project_dir: &Path,
    project_local: bool,
    model_name: &str,
) -> Result<String> {
    let (mut document, existing_contents) = load_document(config_path, project_local)?;
    if existing_contents.is_none() {
        return Err(KvistError::AgentSetupFailed {
            reason: format!(
                "configuration file `{}` does not exist",
                config_path.display()
            ),
        });
    }

    let mut removed_count = 0;
    if let Some(agent) = document.get_mut("agent").and_then(Item::as_table_mut)
        && let Some(profiles) = agent.get_mut("profiles").and_then(Item::as_table_mut)
    {
        for (_, profile_item) in profiles.iter_mut() {
            if let Some(profile_table) = profile_item.as_table_mut() {
                // Check models array of tables
                if let Some(models) = profile_table
                    .get_mut("models")
                    .and_then(Item::as_array_of_tables_mut)
                {
                    let prev_len = models.len();
                    models.retain(|table| {
                        table
                            .get("name")
                            .and_then(Item::as_value)
                            .and_then(Value::as_str)
                            != Some(model_name)
                    });
                    if models.len() < prev_len {
                        removed_count += prev_len - models.len();
                    }
                }
                // If model matches model_name, remove the active model override
                if profile_table
                    .get("model")
                    .and_then(Item::as_value)
                    .and_then(Value::as_str)
                    == Some(model_name)
                {
                    profile_table.remove("model");
                }
                // If default_model matches model_name, reset it
                if profile_table
                    .get("default_model")
                    .and_then(Item::as_value)
                    .and_then(Value::as_str)
                    == Some(model_name)
                {
                    profile_table["default_model"] = value("default");
                }
            }
        }
    }

    if removed_count == 0 {
        return Err(KvistError::AgentSetupFailed {
            reason: format!(
                "model `{model_name}` was not found in `{}`",
                config_path.display()
            ),
        });
    }

    let contents = document.to_string();
    if project_local {
        config::validate_project_configuration_contents(config_path, project_dir, &contents)?;
    } else {
        config::validate_agent_configuration_contents(config_path, &contents)?;
    }
    replace_file_atomically(config_path, &contents)?;

    Ok(format!(
        "Successfully removed model `{model_name}` from `{}`.",
        config_path.display()
    ))
}

/// Lists configured and available agent models with their role assignments.
pub fn list_models(project_dir: &Path) -> Result<String> {
    let mut output = String::new();
    output.push_str("╭── Agent Models ──────────────────────────────────────────────────\n");

    let cfg = config::load(project_dir)?;

    let mut configured_models: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    let roles = [
        ("Developer", &cfg.agent.developer),
        ("Architect", &cfg.agent.architect),
        ("Security Reviewer", &cfg.agent.security_reviewer),
    ];

    for (role_name, profile) in &roles {
        let active_model = profile.model.as_deref().unwrap_or(&profile.default_model);
        for m in &profile.models {
            let entry = configured_models
                .entry(m.name.clone())
                .or_insert_with(|| (m.command.clone(), Vec::new()));
            let role_desc = if m.name == active_model {
                format!("{role_name} (active)")
            } else {
                role_name.to_string()
            };
            if !entry.1.contains(&role_desc) {
                entry.1.push(role_desc);
            }
        }
    }

    output.push_str("│  Configured Models in Project:\n");
    if configured_models.is_empty() {
        output.push_str("│    (no custom models configured; using default templates)\n");
    } else {
        for (name, (command, assigned_roles)) in &configured_models {
            let roles_str = if assigned_roles.is_empty() {
                "unassigned (no roles)".to_owned()
            } else {
                assigned_roles.join(", ")
            };
            output.push_str(&format!("│    • {name}\n"));
            output.push_str(&format!("│      Roles:   {roles_str}\n"));
            output.push_str(&format!("│      Command: {command}\n"));
        }
    }

    // List standalone profiles from agent-runtime
    if let Some(profile_config) = agent_runtime::default_profile_config_path()
        && let Ok(profiles) = agent_runtime::load_profiles(&profile_config)
        && !profiles.is_empty()
    {
        output.push_str("│\n│  Available Standalone Agent-Runtime Profiles:\n");
        for p in &profiles {
            let is_configured = configured_models.contains_key(&p.name);
            let status_badge = if is_configured { " [configured]" } else { "" };
            output.push_str(&format!(
                "│    • {} ({}){}\n",
                p.name, p.provider, status_badge
            ));
        }
    }

    output.push_str("╰──────────────────────────────────────────────────────────────────");
    Ok(output)
}
