//! Interactive agent model setup wizard.

use std::{
    fs,
    io::{BufRead, Write},
    path::{Path, PathBuf},
};

use toml_edit::{DocumentMut, Item, Table, Value, value};

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
        || trimmed.eq_ignore_ascii_case("c")
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
    run_wizard_inner(reader, writer, project_dir, None, force)
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

    let model = if let Some(profile_config) = profile_config {
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
        let setup_options = if let Ok(cfg) = config::load(project_dir) {
            agent_runtime::SetupOptions {
                force,
                ollama_url: cfg.agent.discovery.ollama_url,
                llama_server_url: cfg.agent.discovery.llama_server_url,
            }
        } else {
            agent_runtime::SetupOptions {
                force,
                ..Default::default()
            }
        };
        agent_runtime::collect_profile_with_options(reader, writer, project_dir, setup_options)?
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

    persist_model(&config_path, project_dir, project_local, &model)?;
    write_output(
        writer,
        &format!(
            "\nSuccessfully configured model `{}` in {}.\n\
             Next Step: Run 'kvist agent role set <ROLE> {}' to assign this model to a role.\n\
             ==================================================\n",
            model.name,
            config_path.display(),
            model.name
        ),
    )
}

fn persist_model(
    config_path: &Path,
    project_dir: &Path,
    project_local: bool,
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
    upsert_provider_and_profile(&mut document, model)?;
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

fn upsert_provider_and_profile(
    document: &mut DocumentMut,
    model: &agent_runtime::ModelProfile,
) -> Result<()> {
    let agent = ensure_table(&mut document["agent"], "agent")?;

    // 1. Ensure provider table
    let providers = ensure_table(&mut agent["providers"], "agent.providers")?;
    if !providers.contains_key(&model.provider) {
        let provider_table = ensure_table(
            &mut providers[&model.provider],
            &format!("agent.providers.{}", model.provider),
        )?;
        provider_table["type"] = value(&model.provider);
    }

    // 2. Ensure profile table
    let profiles = ensure_table(&mut agent["profiles"], "agent.profiles")?;
    let profile_table = ensure_table(
        &mut profiles[&model.name],
        &format!("agent.profiles.{}", model.name),
    )?;
    profile_table["provider"] = value(&model.provider);
    profile_table["model"] = value(&model.name);
    profile_table["command"] = value(&model.command);

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
    if let Some(agent) = document.get_mut("agent").and_then(Item::as_table_mut) {
        // 1. Remove from profiles table
        if let Some(profiles) = agent.get_mut("profiles").and_then(Item::as_table_mut) {
            if profiles.contains_key(model_name) {
                profiles.remove(model_name);
                removed_count += 1;
            }
            // Also check legacy tables
            for (_, profile_item) in profiles.iter_mut() {
                if let Some(profile_table) = profile_item.as_table_mut() {
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
                    if profile_table
                        .get("model")
                        .and_then(Item::as_value)
                        .and_then(Value::as_str)
                        == Some(model_name)
                    {
                        profile_table.remove("model");
                        removed_count += 1;
                    }
                    if profile_table
                        .get("default_model")
                        .and_then(Item::as_value)
                        .and_then(Value::as_str)
                        == Some(model_name)
                    {
                        profile_table.remove("default_model");
                        removed_count += 1;
                    }
                    if model_name == "default" && profile_table.contains_key("command_template") {
                        profile_table.remove("command_template");
                        removed_count += 1;
                    }
                }
            }
        }

        // 2. Clear from roles if assigned
        if let Some(roles) = agent.get_mut("roles").and_then(Item::as_table_mut) {
            for (_, role_item) in roles.iter_mut() {
                if let Some(role_table) = role_item.as_table_mut()
                    && role_table
                        .get("profile")
                        .and_then(Item::as_value)
                        .and_then(Value::as_str)
                        == Some(model_name)
                {
                    role_table.remove("profile");
                    removed_count += 1;
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

/// Removes all configured agent models and resets agent profile configuration.
pub fn remove_all_models(
    config_path: &Path,
    project_dir: &Path,
    project_local: bool,
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

    if let Some(agent) = document.get_mut("agent").and_then(Item::as_table_mut) {
        if agent.contains_key("profiles") {
            agent.remove("profiles");
        }
        if let Some(roles) = agent.get_mut("roles").and_then(Item::as_table_mut) {
            for (_, role_item) in roles.iter_mut() {
                if let Some(role_table) = role_item.as_table_mut() {
                    role_table.remove("profile");
                }
            }
        }
    }

    let contents = document.to_string();
    if project_local {
        config::validate_project_configuration_contents(config_path, project_dir, &contents)?;
    } else {
        config::validate_agent_configuration_contents(config_path, &contents)?;
    }
    replace_file_atomically(config_path, &contents)?;

    Ok(format!(
        "Successfully cleared all agent configuration from `{}`.",
        config_path.display()
    ))
}

/// Lists configured and available agent models with their role assignments.
pub fn list_models(project_dir: &Path) -> Result<String> {
    let mut output = String::new();
    output.push_str("╭── Agent Models & Profiles ───────────────────────────────────────\n");

    let config_path = project_dir.join("kvist.toml");
    let (document, existing_contents) =
        load_document(&config_path, true).unwrap_or((DocumentMut::new(), None));

    let mut project_models = Vec::new();
    if existing_contents.is_some()
        && let Some(agent) = document.get("agent").and_then(Item::as_table)
        && let Some(profiles) = agent.get("profiles").and_then(Item::as_table)
    {
        for (name, item) in profiles.iter() {
            if let Some(p_table) = item.as_table() {
                let provider = p_table
                    .get("provider")
                    .and_then(Item::as_value)
                    .and_then(Value::as_str)
                    .unwrap_or("custom");
                let model_id = p_table
                    .get("model")
                    .and_then(Item::as_value)
                    .and_then(Value::as_str);
                project_models.push((
                    name.to_owned(),
                    provider.to_owned(),
                    model_id.map(str::to_owned),
                ));
            }
        }
    }

    output.push_str("│  Configured Models in Project:\n");
    if project_models.is_empty() {
        output.push_str(
            "│    (no models configured; run 'kvist agent profile add' to configure a model)\n",
        );
    } else {
        for (name, provider, model_id) in &project_models {
            output.push_str(&format!("│    • {name}\n"));
            output.push_str(&format!("│      Provider: {provider}\n"));
            if let Some(mid) = model_id {
                output.push_str(&format!("│      Model ID: {mid}\n"));
            }
        }
    }

    // List user-global profiles if ~/.config/kvist/config.toml exists
    if let Some(user_config_path) = config::global_user_config_path()
        && let Ok((user_doc, Some(_))) = load_document(&user_config_path, false)
        && let Some(agent) = user_doc.get("agent").and_then(Item::as_table)
        && let Some(profiles) = agent.get("profiles").and_then(Item::as_table)
    {
        let mut user_models = Vec::new();
        for (name, item) in profiles.iter() {
            if let Some(p_table) = item.as_table() {
                let provider = p_table
                    .get("provider")
                    .and_then(Item::as_value)
                    .and_then(Value::as_str)
                    .unwrap_or("custom");
                let model_id = p_table
                    .get("model")
                    .and_then(Item::as_value)
                    .and_then(Value::as_str);
                user_models.push((
                    name.to_owned(),
                    provider.to_owned(),
                    model_id.map(str::to_owned),
                ));
            }
        }
        if !user_models.is_empty() {
            output.push_str(&format!(
                "│\n│  Global User Profiles ({}):\n",
                user_config_path.display()
            ));
            for (name, provider, model_id) in &user_models {
                output.push_str(&format!("│    • {name} (provider: {provider})\n"));
                if let Some(mid) = model_id {
                    output.push_str(&format!("│      Model ID: {mid}\n"));
                }
            }
        }
    }

    output.push_str("╰──────────────────────────────────────────────────────────────────");
    Ok(output)
}

/// Normalizes a role string to canonical form: "developer", "architect", or "security-reviewer".
pub fn normalize_role_name(role: &str) -> Result<String> {
    let lower = role.trim().to_ascii_lowercase();
    match lower.as_str() {
        "developer" | "dev" => Ok("developer".to_owned()),
        "architect" | "arch" => Ok("architect".to_owned()),
        "security-reviewer" | "security_reviewer" | "security" | "sec" => {
            Ok("security-reviewer".to_owned())
        }
        _ => Err(KvistError::AgentSetupFailed {
            reason: format!(
                "unknown role `{role}`; valid roles are: developer, architect, security-reviewer"
            ),
        }),
    }
}

/// Lists current role assignments and available model profiles for assignment.
pub fn list_roles(project_dir: &Path) -> Result<String> {
    let mut output = String::new();
    output.push_str("╭── Role Assignments ──────────────────────────────────────────────\n");

    let mut role_models: std::collections::BTreeMap<String, (Option<String>, Option<String>)> =
        std::collections::BTreeMap::new();
    role_models.insert("developer".to_owned(), (None, None));
    role_models.insert("architect".to_owned(), (None, None));
    role_models.insert("security-reviewer".to_owned(), (None, None));

    let config_path = project_dir.join("kvist.toml");
    let (document, existing_contents) =
        load_document(&config_path, true).unwrap_or((DocumentMut::new(), None));

    if existing_contents.is_some()
        && let Some(agent) = document.get("agent").and_then(Item::as_table)
    {
        if let Some(roles) = agent.get("roles").and_then(Item::as_table) {
            for (role_key, role_item) in roles.iter() {
                if let Some(role_table) = role_item.as_table() {
                    let active = role_table
                        .get("profile")
                        .or_else(|| role_table.get("model"))
                        .and_then(Item::as_value)
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let effort = role_table
                        .get("thinking_effort")
                        .and_then(Item::as_value)
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let norm_key = if role_key == "security_reviewer" {
                        "security-reviewer"
                    } else {
                        role_key
                    };
                    role_models.insert(norm_key.to_owned(), (active, effort));
                }
            }
        }
        if let Some(profiles) = agent.get("profiles").and_then(Item::as_table) {
            for (role_key, profile_item) in profiles.iter() {
                if let Some(profile_table) = profile_item.as_table() {
                    let active = profile_table
                        .get("model")
                        .and_then(Item::as_value)
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    if active.is_some() {
                        let norm_key = if role_key == "security_reviewer" {
                            "security-reviewer"
                        } else {
                            role_key
                        };
                        if role_models
                            .get(norm_key)
                            .map(|(a, _)| a.is_none())
                            .unwrap_or(true)
                        {
                            role_models.insert(norm_key.to_owned(), (active, None));
                        }
                    }
                }
            }
        }
    }

    output.push_str("│  Role                Assigned Model Profile\n");
    output.push_str("│  ────────────────────────────────────────────────────────────────\n");
    for (role_name, (assigned, effort)) in &role_models {
        let assigned_str = match (assigned, effort) {
            (Some(profile), Some(eff)) => format!("{profile} (thinking: {eff})"),
            (Some(profile), None) => profile.clone(),
            (None, _) => "(unassigned)".to_owned(),
        };
        output.push_str(&format!("│  {:<20} {}\n", role_name, assigned_str));
    }

    let merged_config = config::load(project_dir)
        .map(|c| c.agent)
        .unwrap_or_default();
    let mut available_models: Vec<String> = merged_config.profiles.keys().cloned().collect();

    if let Some(user_config_path) = config::global_user_config_path()
        && let Ok((user_doc, Some(_))) = load_document(&user_config_path, false)
        && let Some(agent) = user_doc.get("agent").and_then(Item::as_table)
        && let Some(profiles) = agent.get("profiles").and_then(Item::as_table)
    {
        for (name, _) in profiles.iter() {
            if !available_models.contains(&name.to_owned()) {
                available_models.push(name.to_owned());
            }
        }
    }

    if !available_models.is_empty() {
        output.push_str("│\n│  Available Model Profiles for Assignment:\n");
        for model in &available_models {
            output.push_str(&format!("│    • {}\n", model));
        }
    }

    output.push_str("│\n│  Commands:\n");
    output.push_str(
        "│    Assign role:   kvist agent role set <ROLE> <MODEL_NAME> [--thinking-effort <EFFORT>]\n",
    );
    output.push_str("│    Clear role:    kvist agent role clear <ROLE>\n");
    output.push_str("│    Add profile:   kvist agent profile add\n");
    output.push_str("╰──────────────────────────────────────────────────────────────────");
    Ok(output)
}

/// Sets the active model profile for a given role in project or global configuration.
pub fn set_role_model(
    config_path: &Path,
    project_dir: &Path,
    project_local: bool,
    role: &str,
    model_name: &str,
) -> Result<String> {
    set_role_model_with_effort(
        config_path,
        project_dir,
        project_local,
        role,
        model_name,
        None,
    )
}

/// Sets the active model profile and optional thinking effort for a given role.
pub fn set_role_model_with_effort(
    config_path: &Path,
    project_dir: &Path,
    project_local: bool,
    role: &str,
    model_name: &str,
    thinking_effort: Option<agent_runtime::ReasoningEffort>,
) -> Result<String> {
    let normalized_role = normalize_role_name(role)?;
    let (mut document, existing_contents) = load_document(config_path, project_local)?;

    let mut found = false;

    if let Some(agent) = document.get("agent").and_then(Item::as_table)
        && let Some(profiles) = agent.get("profiles").and_then(Item::as_table)
    {
        if profiles.contains_key(model_name) {
            found = true;
        } else {
            for (_, p_item) in profiles.iter() {
                if let Some(p_table) = p_item.as_table()
                    && let Some(models) = p_table.get("models").and_then(Item::as_array_of_tables)
                {
                    for m in models.iter() {
                        if m.get("name")
                            .and_then(Item::as_value)
                            .and_then(Value::as_str)
                            == Some(model_name)
                        {
                            found = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    if !found
        && let Ok(cfg) = config::load(project_dir)
        && cfg.agent.profiles.contains_key(model_name)
    {
        found = true;
    }

    if !found
        && let Some(user_config_path) = config::global_user_config_path()
        && let Ok((user_doc, Some(_))) = load_document(&user_config_path, false)
        && let Some(agent) = user_doc.get("agent").and_then(Item::as_table)
        && let Some(profiles) = agent.get("profiles").and_then(Item::as_table)
        && profiles.contains_key(model_name)
    {
        found = true;
    }

    if !found {
        return Err(KvistError::AgentSetupFailed {
            reason: format!(
                "model profile `{model_name}` is not configured in `{}`.\nRun 'kvist agent profile add' to configure it first.",
                config_path.display()
            ),
        });
    }

    let agent = ensure_table(&mut document["agent"], "agent")?;
    let roles = ensure_table(&mut agent["roles"], "agent.roles")?;
    let role_key = if normalized_role == "security-reviewer"
        && roles.contains_key("security_reviewer")
        && !roles.contains_key("security-reviewer")
    {
        "security_reviewer"
    } else {
        normalized_role.as_str()
    };

    let role_table = ensure_table(&mut roles[role_key], &format!("agent.roles.{role_key}"))?;
    role_table["profile"] = value(model_name);
    if let Some(effort) = thinking_effort {
        role_table["thinking_effort"] = value(effort.as_str());
    }

    let contents = document.to_string();
    if project_local {
        config::validate_project_configuration_contents(config_path, project_dir, &contents)?;
    } else {
        config::validate_agent_configuration_contents(config_path, &contents)?;
    }
    if existing_contents.is_some() {
        replace_file_atomically(config_path, &contents)?;
    } else {
        write_new_file_atomically(config_path, &contents)?;
    }

    Ok(format!(
        "Successfully assigned model profile `{model_name}` to role `{normalized_role}` in `{}`.",
        config_path.display()
    ))
}

/// Clears role assignment(s) in project or global configuration.
pub fn clear_role(
    config_path: &Path,
    project_dir: &Path,
    project_local: bool,
    role: Option<&str>,
    all: bool,
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

    let mut cleared = 0;
    if let Some(agent) = document.get_mut("agent").and_then(Item::as_table_mut) {
        if let Some(roles) = agent.get_mut("roles").and_then(Item::as_table_mut) {
            for (role_key, role_item) in roles.iter_mut() {
                let should_clear = if all {
                    true
                } else if let Some(target_role) = role {
                    let norm = normalize_role_name(target_role).unwrap_or(target_role.to_owned());
                    role_key == norm
                        || (norm == "security-reviewer" && role_key == "security_reviewer")
                } else {
                    false
                };

                if should_clear
                    && let Some(role_table) = role_item.as_table_mut()
                    && role_table.contains_key("profile")
                {
                    role_table.remove("profile");
                    cleared += 1;
                }
            }
        }

        // Also check legacy agent.profiles tables
        if let Some(profiles) = agent.get_mut("profiles").and_then(Item::as_table_mut) {
            for (role_key, profile_item) in profiles.iter_mut() {
                let should_clear = if all {
                    true
                } else if let Some(target_role) = role {
                    let norm = normalize_role_name(target_role).unwrap_or(target_role.to_owned());
                    role_key == norm
                        || (norm == "security-reviewer" && role_key == "security_reviewer")
                } else {
                    false
                };

                if should_clear
                    && let Some(profile_table) = profile_item.as_table_mut()
                    && profile_table.contains_key("model")
                {
                    profile_table.remove("model");
                    cleared += 1;
                }
            }
        }
    }

    if cleared == 0 {
        let target = if all {
            "any role".to_owned()
        } else {
            format!("role `{}`", role.unwrap_or(""))
        };
        return Err(KvistError::AgentSetupFailed {
            reason: format!("no active model assignment was found for {target}"),
        });
    }

    let contents = document.to_string();
    if project_local {
        config::validate_project_configuration_contents(config_path, project_dir, &contents)?;
    } else {
        config::validate_agent_configuration_contents(config_path, &contents)?;
    }
    replace_file_atomically(config_path, &contents)?;

    let target_desc = if all {
        "all roles".to_owned()
    } else {
        format!("role `{}`", role.unwrap_or(""))
    };
    Ok(format!(
        "Successfully cleared model assignment for {target_desc} in `{}`.",
        config_path.display()
    ))
}
