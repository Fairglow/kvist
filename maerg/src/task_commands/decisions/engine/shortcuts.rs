use super::*;

const REGISTRY: &str = "shortcuts.json";
const MAX_ACTIONS: usize = 2048;

/// A host-rendered response; its number refers to exact stored arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResponseAction {
    pub number: u32,
    pub description: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(in crate::task_commands::decisions) enum Command {
    OptionPlan {
        task: String,
        catalog_digest: String,
        option: String,
        title: String,
    },
    ApplyPlan {
        task: String,
        plan_id: String,
        approval: String,
        continue_run: bool,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: u32,
    project: PathBuf,
    component: PathBuf,
    next_number: u32,
    actions: BTreeMap<u32, Command>,
    authentication_tag: String,
}

fn read(context: &TaskContext) -> Result<Option<(Registry, String)>> {
    let path = context.component_dir.join(DIRECTORY).join(REGISTRY);
    if !exists(&path)? {
        return Ok(None);
    }
    let text = read_text(&path)?;
    let registry: Registry = serde_json::from_str(&text)
        .map_err(|error| fail(&path, format!("invalid shortcut JSON: {error}")))?;
    if registry.schema_version != 1
        || registry.project != absolute(&context.project_dir)?
        || registry.component != absolute(&context.component_dir)?
        || registry.next_number == 0
        || registry.actions.len() > MAX_ACTIONS
        || registry
            .actions
            .keys()
            .any(|number| *number == 0 || *number >= registry.next_number)
        || !equal(
            &registry.authentication_tag,
            &authenticated(&registry, &existing_secret()?)?,
        )
    {
        return Err(fail(
            &path,
            "shortcut authentication, numbering or component identity failed; no response was executed",
        ));
    }
    Ok(Some((registry, text)))
}

fn expansion(component: &Path, command: &Command) -> String {
    let component = component_argument(component);
    match command {
        Command::OptionPlan {
            task,
            option,
            catalog_digest,
            ..
        } => format!(
            "kvist decision {component} plan --task {} --option {} --catalog {}",
            quote(task),
            quote(option),
            quote(catalog_digest),
        ),
        Command::ApplyPlan {
            plan_id,
            approval,
            continue_run,
            ..
        } => format!(
            "kvist decision {component} apply {} --approve {}{}",
            quote(plan_id),
            quote(approval),
            if *continue_run { " --continue" } else { "" },
        ),
    }
}

fn response(context: &TaskContext, number: u32, command: &Command) -> ResponseAction {
    let description = match command {
        Command::OptionPlan { task, title, .. } => {
            format!("Prepare {title} ({task}); no controlled edits")
        }
        Command::ApplyPlan {
            task, continue_run, ..
        } => format!(
            "Approve exact plan for {task}{}",
            if *continue_run {
                " and continue ordinary execution"
            } else {
                "; stop after application"
            },
        ),
    };
    ResponseAction {
        number,
        description,
        command: expansion(&context.component_path, command),
    }
}

pub(in crate::task_commands::decisions) fn list(
    context: &TaskContext,
) -> Result<Vec<ResponseAction>> {
    Ok(read(context)?
        .map(|(registry, _)| {
            registry
                .actions
                .iter()
                .map(|(number, command)| response(context, *number, command))
                .collect()
        })
        .unwrap_or_default())
}

fn register(context: &TaskContext, commands: Vec<Command>) -> Result<Vec<ResponseAction>> {
    safe_path(&context.component_dir, false)?;
    let lock = TaskLock::for_context(context, "decision-shortcuts", &now()?)?;
    let result = (|| {
        lock.revalidate()?;
        ensure_no_pending(&context.component_dir)?;
        let (previous, before) = read(context)?
            .map(|(registry, text)| (Some(registry), Some(text)))
            .unwrap_or((None, None));
        let mut registry = previous.unwrap_or(Registry {
            schema_version: 1,
            project: absolute(&context.project_dir)?,
            component: absolute(&context.component_dir)?,
            next_number: 1,
            actions: BTreeMap::new(),
            authentication_tag: String::new(),
        });
        let mut responses = Vec::new();
        for command in commands {
            let number = match registry
                .actions
                .iter()
                .find(|(_, existing)| *existing == &command)
            {
                Some((number, _)) => *number,
                None => {
                    if registry.actions.len() >= MAX_ACTIONS {
                        return Err(fail(
                            &context.component_dir,
                            "shortcut registry reached 2048 retained actions; use the full exact command; numbers were not recycled",
                        ));
                    }
                    let number = registry.next_number;
                    registry.next_number = number.checked_add(1).ok_or_else(|| {
                        fail(
                            &context.component_dir,
                            "shortcut number space exhausted; use the full command",
                        )
                    })?;
                    registry.actions.insert(number, command.clone());
                    number
                }
            };
            responses.push(response(context, number, &command));
        }
        registry.authentication_tag = authenticated(&registry, &secret(context)?)?;
        let path = context.component_dir.join(DIRECTORY).join(REGISTRY);
        storage::directory(
            path.parent()
                .ok_or_else(|| fail(&path, "missing shortcut directory"))?,
        )?;
        let text = encode(&registry)?;
        bounded(&text, &path)?;
        lock.revalidate()?;
        ensure_no_pending(&context.component_dir)?;
        if let Some(before) = before {
            if before != text {
                replace_controlled(&path, &before, &text, &lock)?;
            }
        } else {
            write_new(&path, &text)?;
        }
        Ok(responses)
    })();
    let released = lock.release();
    result.and_then(|responses| released.map(|()| responses))
}

pub(in crate::task_commands::decisions) fn options(
    context: &TaskContext,
    catalog: &Catalog,
    text: &str,
) -> Result<Vec<ResponseAction>> {
    register(
        context,
        catalog
            .alternatives
            .options
            .iter()
            .map(|option| Command::OptionPlan {
                task: catalog.task_id.clone(),
                catalog_digest: digest(text.as_bytes()),
                option: option.id.clone(),
                title: option.title.clone(),
            })
            .collect(),
    )
}

pub(in crate::task_commands::decisions) fn plan(
    context: &TaskContext,
    plan: &Plan,
) -> Result<Vec<ResponseAction>> {
    register(
        context,
        [false, true]
            .into_iter()
            .map(|continue_run| Command::ApplyPlan {
                task: plan.input.task.id.clone(),
                plan_id: plan.id.clone(),
                approval: plan.approval.clone(),
                continue_run,
            })
            .collect(),
    )
}

pub(in crate::task_commands::decisions) fn discover(
    context: &TaskContext,
) -> Result<Vec<ResponseAction>> {
    let directory = context.component_dir.join(DIRECTORY);
    if !exists(&directory)? {
        return Ok(Vec::new());
    }
    // Exact recovery uses already retained responses; discovery must not publish while fenced.
    if exists(&directory.join("pending.json"))? {
        return list(context);
    }
    safe_path(&directory, false)?;
    let mut ids = Vec::new();
    for (index, entry) in fs::read_dir(&directory)
        .map_err(|error| fail(&directory, error.to_string()))?
        .enumerate()
    {
        if index >= 4096 {
            return Err(fail(&directory, "shortcut discovery exceeds 4096 entries"));
        }
        let path = entry
            .map_err(|error| fail(&directory, error.to_string()))?
            .path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        if let Some(id) = path.file_stem().and_then(|stem| stem.to_str())
            && id.len() == 64
            && id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && !exists(&directory.join(format!("{id}.applied.json")))?
        {
            ids.push(id.to_owned());
        }
    }
    ids.sort();
    let mut commands = Vec::new();
    let mut bytes = 0;
    for id in ids {
        let plan = load_plan(&context.project_dir, &context.component_path, &id)?;
        bytes += encode(&plan)?.len();
        if bytes > MAX_TOTAL {
            return Err(fail(&directory, "shortcut plan discovery exceeds 8 MiB"));
        }
        for continue_run in [false, true] {
            commands.push(Command::ApplyPlan {
                task: plan.input.task.id.clone(),
                plan_id: plan.id.clone(),
                approval: plan.approval.clone(),
                continue_run,
            });
        }
    }
    if !commands.is_empty() {
        register(context, commands)?;
    }
    list(context)
}

pub(in crate::task_commands::decisions) fn resolve(
    context: &TaskContext,
    number: u32,
) -> Result<(Command, ResponseAction)> {
    read(context)?    .and_then(|(registry, _)| registry.actions.get(&number).cloned())
        .map(|command| {
            let response = response(context, number, &command);
            (command, response)
        })
        .ok_or_else(|| fail(&context.component_dir, format!(
            "unknown response {number}; run `kvist decision {} actions`; no latest-plan fallback",
            component_argument(&context.component_path))))
}

pub(in crate::task_commands::decisions) fn markdown(
    component: &Path,
    actions: &[ResponseAction],
) -> String {
    if actions.is_empty() {
        return "\n\nNo numbered responses yet. Request options or prepare a plan.\n".into();
    }
    let mut text = "\n\n## Numbered responses\n\nNumbers invoke exactly the commands below. Choice prepares only; approval accepts the full frozen plan and review/exception. Historical responses still validate their original inputs; no newer plan is substituted. Tab completes response numbers.\n\n".to_owned();
    for action in actions {
        text.push_str(&format!(
            "- **{}**: {}. Run `kvist decision {} do {}`.\n\n  Exact command: `{}`\n\n",
            action.number,
            action.description,
            component_argument(component),
            action.number,
            action.command,
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, TaskContext) {
        let directory = tempfile::tempdir().unwrap();
        let component_dir = directory.path().join("component");
        fs::create_dir(&component_dir).unwrap();
        let context = TaskContext {
            project_dir: directory.path().to_path_buf(),
            component_dir,
            component_path: PathBuf::from("component"),
        };
        (directory, context)
    }

    fn command(id: &str) -> Command {
        Command::ApplyPlan {
            task: "paused-task".into(),
            plan_id: id.into(),
            approval: format!("sha256:{id}"),
            continue_run: false,
        }
    }

    #[test]
    fn numbered_commands_are_immutable_deduplicated_and_component_bound() {
        let (_directory, context) = fixture();
        let first = register(&context, vec![command("first")]).unwrap();
        let mut continuing = command("first");
        if let Command::ApplyPlan { continue_run, .. } = &mut continuing {
            *continue_run = true;
        }
        let second = register(
            &context,
            vec![command("first"), continuing, command("later")],
        )
        .unwrap();
        assert_eq!(first[0].number, 1);
        assert_eq!(
            second
                .iter()
                .map(|response| response.number)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(resolve(&context, 1).unwrap().0, command("first"));
        assert!(resolve(&context, 0).is_err());
        assert!(resolve(&context, 4).is_err());
        assert!(second[1].command.ends_with("--continue"));
        assert!(!second[0].command.contains("--continue"));
        let other = context.project_dir.join("other");
        fs::create_dir_all(other.join(DIRECTORY)).unwrap();
        fs::copy(
            context.component_dir.join(DIRECTORY).join(REGISTRY),
            other.join(DIRECTORY).join(REGISTRY),
        )
        .unwrap();
        let other = TaskContext {
            component_dir: other,
            component_path: "other".into(),
            ..context
        };
        assert!(list(&other).is_err());
    }

    #[test]
    fn changed_registry_unknown_fields_and_links_do_not_authorize_commands() {
        let (_directory, context) = fixture();
        register(&context, vec![command("original")]).unwrap();
        let path = context.component_dir.join(DIRECTORY).join(REGISTRY);
        let original = fs::read_to_string(&path).unwrap();
        fs::write(&path, original.replace("original", "retargeted")).unwrap();
        assert!(resolve(&context, 1).is_err());
        assert!(register(&context, vec![command("new")]).is_err());
        let mut value: Value = serde_json::from_str(&original).unwrap();
        value["shell"] = Value::String("arbitrary command".into());
        fs::write(&path, value.to_string()).unwrap();
        assert!(list(&context).is_err());
        fs::write(&path, &original).unwrap();
        let linked = context
            .component_dir
            .join(DIRECTORY)
            .join("hardlinked.json");
        fs::hard_link(&path, &linked).unwrap();
        assert!(resolve(&context, 1).is_err());
        fs::remove_file(&linked).unwrap();
        #[cfg(unix)]
        {
            let target = context.project_dir.join("outside.json");
            fs::rename(&path, &target).unwrap();
            std::os::unix::fs::symlink(&target, &path).unwrap();
            assert!(resolve(&context, 1).is_err());
        }
    }

    #[test]
    fn pending_transactions_and_live_locks_prevent_shortcut_publication() {
        let (_directory, context) = fixture();
        register(&context, vec![command("original")]).unwrap();
        let path = context.component_dir.join(DIRECTORY).join(REGISTRY);
        let original = fs::read(&path).unwrap();
        let pending = context.component_dir.join(DIRECTORY).join("pending.json");
        fs::write(&pending, "{}").unwrap();
        assert!(register(&context, vec![command("new")]).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(discover(&context).unwrap().len(), 1);
        assert!(
            resolve(&context, 1).is_ok(),
            "identical recovery may still resolve"
        );
        fs::remove_file(pending).unwrap();
        let lock = TaskLock::for_context(&context, "other-writer", &now().unwrap()).unwrap();
        assert!(register(&context, vec![command("new")]).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        lock.release().unwrap();
    }

    #[test]
    fn bounded_shortcut_registry_never_recycles_full_or_oversized_entries() {
        let (_directory, context) = fixture();
        let responses = register(
            &context,
            (0..MAX_ACTIONS)
                .map(|number| command(&number.to_string()))
                .collect(),
        )
        .unwrap();
        assert_eq!(responses.len(), MAX_ACTIONS);
        let path = context.component_dir.join(DIRECTORY).join(REGISTRY);
        let before = fs::read(&path).unwrap();
        assert!(register(&context, vec![command("over-limit")]).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        let (_other_directory, other) = fixture();
        assert!(register(&other, vec![command(&"x".repeat(MAX_FILE as usize))]).is_err());
        assert!(!other.component_dir.join(DIRECTORY).join(REGISTRY).exists());
    }
}
