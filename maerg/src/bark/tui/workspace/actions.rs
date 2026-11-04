use std::io::{self, Write};
use std::path::Path;

use clap::Parser;
use crossterm::event::{KeyCode, KeyEvent};

use super::super::{editor::quote, process::Running, storage::Transcript};
use super::{MenuAction, Overlay, Screen, TerminalGuard, Workspace};
use crate::{bark, cli, presentation};

impl Workspace {
    pub(super) fn submit(&mut self, terminal: &mut Screen) -> io::Result<()> {
        if let Some(run) = &mut self.running {
            run.answer(&self.input.text)?;
            self.input.set("");
            return self
                .append("\n\n_Input supplied to the running command; answer not retained._\n\n");
        }
        let line = self.input.text.trim().to_owned();
        if line.is_empty() {
            return Ok(());
        }
        self.input.set("");
        self.history_index = None;
        self.prefill.begin_submission();
        if self.history.last() != Some(&line) {
            self.history
                .push(presentation::redact(&line, &self.redactions));
            while self.history.len() > 500
                || serde_json::to_string(&self.history)
                    .map_err(io::Error::other)?
                    .len()
                    > 1_048_576
            {
                self.history.remove(0);
            }
            self.transcript.save_history(&self.history)?;
        }
        self.append(&format!(
            "\n\n---\n\n## Command\n\n{}\n\n",
            presentation::fence(&line, "text")
        ))?;
        self.notice.clear();
        self.last_failed = false;
        self.follow = true;
        let (program, arguments) = sav::split_raw_command(&line).map_err(io::Error::other)?;
        match program.as_str() {
            "exit" | "quit" if arguments.is_empty() => self.quit = true,
            "shell" => self.append("Already in the Kvist workspace.\n")?,
            "settings" if arguments.is_empty() => self.menu("Settings"),
            "cd" => {
                let focus = match arguments.as_slice() {
                    [] => return self.append(&format!("**Component:** {}\n", presentation::literal(self.focus().as_deref().unwrap_or("(project)")))),
                    [component] if component == "-" => None,
                    [component] => {
                        let normalized = component.strip_prefix("./").unwrap_or(component);
                        if !self.state.components().iter().any(|known| known == normalized) {
                            return Err(io::Error::other(format!("unknown component `{component}`; use Tab to choose a discovered component")));
                        }
                        Some(normalized.to_owned())
                    }
                    _ => return Err(io::Error::other("usage: cd [COMPONENT_DIR|-]")),
                };
                self.change_focus(focus)?;
            }
            "tasks" => {
                let parsed = bark::parse_tasks_args(&arguments).map_err(io::Error::other)?;
                let component = parsed.component.or_else(|| self.focus()).unwrap_or_else(|| ".".into());
                let scope = self.state.scopes.get(&component).ok_or_else(|| io::Error::other(format!("no readable queue for `{component}`; run doctor")))?;
                let mut markdown = format!("# Tasks in {}\n\n| Task | State | Kind | Title |\n| --- | --- | --- | --- |\n", presentation::literal(&component));
                for task in scope.tasks.iter().filter(|task| parsed.status.is_none_or(|status| status == task.status)) {
                    markdown.push_str(&format!("| {} | {} | {} | {} |\n", presentation::literal(&task.id), bark::status_label(task.status), bark::kind_label(task.kind), presentation::literal(&task.title)));
                }
                for task in scope.tasks.iter().filter(|task| parsed.status.is_none_or(|status| status == task.status)) {
                    if let Some(reason) = &task.blocked_reason {
                        markdown.push_str(&format!("\n## {}\n\n{}\n\n", presentation::literal(&task.id), presentation::fence(reason, "text")));
                    }
                    if let Some(next) = bark::next_command_for(task, &scope.tasks, &component) {
                        markdown.push_str(&format!("\n**Next for {}:** {}\n", presentation::literal(&task.id), presentation::fence(&next, "text")));
                    }
                }
                self.append(&markdown)?;
            }
            "overview" if arguments.is_empty() => self.start(vec!["status".into()])?,
            "run" => {
                let mut args = vec!["task".into(), "run".into()];
                args.extend(arguments);
                self.dispatch(args, terminal)?;
            }
            "help" if arguments.is_empty() => {
                let markdown = "# Kvist workspace\n\n";
                let mut rows = String::new();
                for command in &self.tree.subcommands {
                    rows.push_str(&format!("| {} | {} |\n", presentation::literal(&command.name), presentation::literal(command.help.as_deref().unwrap_or(""))));
                }
                let header = "Commands use the same grammar as `kvist COMMAND`. Component focus is inserted into the editable buffer, never hidden for required arguments. `cd -` returns to the project.\n\n| Command | Purpose |\n| --- | --- |\n";
                self.append(&format!("{markdown}{header}{rows}\n\n{}", view_help()))?;
            }
            "history" => {
                let count = bark::parse_count("history", &arguments, 20).map_err(io::Error::other)?;
                let text = self.history.iter().rev().take(count).map(|line| format!("- {}\n", presentation::literal(line))).collect::<String>();
                self.append(&format!("# Command history\n\n{text}"))?;
            }
            "journal" if arguments.is_empty() => self.append(&format!(
                "# Transcript\n\n**Directory:** {}\n\n{} segments. PageUp/PageDown browse; Ctrl+Home/End reach the ends. Source Markdown is local diagnostic history, not workflow evidence.\n",
                presentation::literal(&self.transcript.directory.display().to_string()), self.transcript.latest() + 1
            ))?,
            "last" => {
                let count = bark::parse_count("last", &arguments, 10).map_err(io::Error::other)?;
                let runs = bark::runs::scan_recent_runs(&self.project, count);
                let mut text = "# Recent runs\n\n| Component | Task | Result | Tokens in/out |\n| --- | --- | --- | --- |\n".to_owned();
                for run in runs {
                    text.push_str(&format!("| {} | {} | {:?} | {:?}/{:?} |\n", presentation::literal(run.component.as_deref().unwrap_or(".")), presentation::literal(run.task_id.as_deref().unwrap_or("unknown")), run.success, run.tokens_input, run.tokens_output));
                }
                self.append(&text)?;
            }
            "locks" if arguments.is_empty() || arguments == ["clean"] => {
                let removed = if arguments.is_empty() { Vec::new() } else { bark::locks::clean_stale() };
                let entries = bark::locks::scan();
                let mut text = format!("# Task locks\n\n{} stale locks removed.\n\n| Path | Task | State |\n| --- | --- | --- |\n", removed.len());
                for entry in entries {
                    text.push_str(&format!("| {} | {} | {} |\n", presentation::literal(&entry.path.display().to_string()), presentation::literal(entry.task_id.as_deref().unwrap_or("unknown")), if entry.live { "live" } else { "stale" }));
                }
                self.append(&text)?;
                self.refresh();
            }
            "theme" => match arguments.as_slice() {
                _ if arguments.is_empty() || arguments == ["list"] => self.menu("Theme"),
                [set, value] if set == "set" => self.set_theme(value)?,
                _ => return Err(io::Error::other("usage: theme [list|set NAME|PATH]")),
            },
            "prompt" if arguments.len() == 1 && self.state.find_task(&arguments[0]).is_some() => {
                let (_, task) = self.state.find_task(&arguments[0]).ok_or_else(|| io::Error::other("task disappeared"))?;
                let seed = bark::build_prompt_seed(task);
                self.edit_prompt(&seed, vec!["prompt".into()], terminal)?;
            }
            _ => {
                let mut args = vec![program];
                args.extend(arguments);
                self.dispatch(args, terminal)?;
            }
        }
        Ok(())
    }

    fn dispatch(&mut self, mut arguments: Vec<String>, terminal: &mut Screen) -> io::Result<()> {
        let parsed =
            cli::Cli::try_parse_from(std::iter::once("kvist".to_owned()).chain(arguments.clone()));
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                self.last_failed = error.exit_code() != 0;
                return self.append(&presentation::document(
                    if self.last_failed {
                        "Command syntax"
                    } else {
                        "Command help"
                    },
                    &presentation::fence(&error.to_string(), "text"),
                ));
            }
        };
        let Some(command) = parsed.command else {
            return self.start(vec!["status".into()]);
        };
        let command = cli::normalize_task_positionals(command).map_err(io::Error::other)?;
        if matches!(command, cli::Command::Shell { .. }) {
            return self.append("Already in the Kvist workspace.\n");
        }
        if matches!(command, cli::Command::AuthoringApply { .. }) {
            return Err(io::Error::other(
                "internal effect application is not a workspace command",
            ));
        }
        if let Some(focus) = self.focus() {
            let omitted = match &command {
                cli::Command::Task {
                    command:
                        cli::TaskCommand::Next { component_dir }
                        | cli::TaskCommand::Log { component_dir, .. }
                        | cli::TaskCommand::Transition { component_dir, .. }
                        | cli::TaskCommand::Unlock { component_dir, .. }
                        | cli::TaskCommand::Recover { component_dir, .. }
                        | cli::TaskCommand::Finalize { component_dir, .. },
                }
                | cli::Command::Component {
                    command:
                        cli::ComponentCommand::Validate { component_dir }
                        | cli::ComponentCommand::Accept { component_dir, .. },
                } => component_dir.is_none(),
                _ => false,
            };
            if omitted
                && let Some(index) = arguments
                    .iter()
                    .position(|arg| arg == "task" || arg == "component")
            {
                arguments.insert(index + 2, focus);
            }
        }
        if matches!(
            command,
            cli::Command::Task {
                command: cli::TaskCommand::Run { .. }
            }
        ) && !arguments.iter().any(|arg| arg == "--stream")
        {
            arguments.push("--stream".into());
        }
        if let cli::Command::Prompt { editor: true, .. } = command {
            arguments.retain(|arg| arg != "--editor");
            return self.edit_prompt("", arguments, terminal);
        }
        if matches!(
            command,
            cli::Command::Prompt {
                allow_host_execution: true,
                ..
            }
        ) {
            return Err(io::Error::other(
                "host execution requires the standalone interactive `kvist prompt --allow-host-execution` acknowledgement; the embedded transcript is sandbox-only",
            ));
        }
        if let Some(prompt) = bark::destructive_gate(&command) {
            self.overlay = Some(Overlay::Confirm {
                prompt: prompt.into(),
                arguments,
            });
            return Ok(());
        }
        self.start(arguments)
    }

    fn edit_prompt(
        &mut self,
        seed: &str,
        arguments: Vec<String>,
        terminal: &mut Screen,
    ) -> io::Result<()> {
        TerminalGuard::restore()?;
        let edited = bark::edit_prompt_with_seed(seed);
        let guard = TerminalGuard::enter()?;
        std::mem::forget(guard);
        terminal.clear()?;
        let prompt = edited.map_err(io::Error::other)?;
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(prompt.as_bytes())?;
        file.flush()?;
        let mut arguments = arguments;
        arguments.extend(["--file".into(), file.path().display().to_string()]);
        self.prompt_file = Some(file);
        self.append(&presentation::document(
            "Authored prompt",
            &presentation::fence(&prompt, "markdown"),
        ))?;
        self.overlay = Some(Overlay::Confirm {
            prompt: "submit authored prompt".into(),
            arguments,
        });
        Ok(())
    }

    pub(super) fn start(&mut self, arguments: Vec<String>) -> io::Result<()> {
        if self.running.is_some() {
            return Err(io::Error::other(
                "wait for the running command before starting another command",
            ));
        }
        self.capture_failed = false;
        self.follow = true;
        self.streams = std::array::from_fn(|_| {
            super::super::process::Stream::with_redactions(self.redactions.clone())
        });
        self.running = Some(Running::spawn(&self.project, &arguments)?);
        self.notice =
            "Running; Ctrl+C cancels. Input answers the command (answers are not retained).".into();
        Ok(())
    }

    pub(super) fn change_focus(&mut self, focus: Option<String>) -> io::Result<()> {
        if self.running.is_some() {
            return Err(io::Error::other(
                "wait for the running command before switching component",
            ));
        }
        let scope = match &focus {
            Some(component) => {
                let project = crate::context::ProjectContext {
                    project_dir: self.project.clone(),
                };
                let component = project
                    .resolve_component(Some(Path::new(component)))
                    .map_err(io::Error::other)?;
                project
                    .component_path(&component)
                    .map_err(io::Error::other)?
            }
            None => self.project.clone(),
        };
        if scope.join(".kvist/transcript") != self.transcript.directory {
            let transcript = Transcript::open(&scope)?;
            let history = transcript.history()?;
            let segment = transcript.latest();
            let source = transcript.read(segment)?;
            self.transcript = transcript;
            self.history = history;
            self.segment = segment;
            self.source = source;
        }
        *self
            .focus
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = focus;
        self.prefill.begin_submission();
        self.input.set("");
        self.history_index = None;
        self.history_draft.clear();
        self.follow = true;
        self.scroll = 0;
        self.horizontal = 0;
        self.refresh();
        self.append(&format!(
            "\n\n# Focus: {}\n\n",
            presentation::literal(self.focus().as_deref().unwrap_or("(project)"))
        ))
    }

    pub(super) fn set_theme(&mut self, value: &str) -> io::Result<()> {
        let value = match bark::theme::match_preference(value) {
            bark::theme::Preference::Path(path) => {
                path.canonicalize()?.to_string_lossy().into_owned()
            }
            _ => value.to_owned(),
        };
        let theme = match bark::theme::match_preference(&value) {
            bark::theme::Preference::Dark => bark::Theme::dark(),
            bark::theme::Preference::Light => bark::Theme::light(),
            bark::theme::Preference::Named(id) => bark::Theme::by_id(id),
            bark::theme::Preference::Path(path) => {
                bark::Theme::by_id(bark::theme::load_theme_file(&path).map_err(io::Error::other)?)
            }
        };
        super::super::storage::private_directory(&self.project.join(".kvist"))?;
        bark::theme::save_preference(&self.project, &value).map_err(io::Error::other)?;
        self.theme = theme;
        self.dirty = true;
        self.append(&format!(
            "\n\n**Theme:** {}\n\n",
            presentation::literal(&theme.name())
        ))
    }

    pub(super) fn menu(&mut self, title: &'static str) {
        let choices = match title {
            "Component" => std::iter::once(("(project)".into(), MenuAction::Focus(None)))
                .chain(
                    self.state
                        .components()
                        .iter()
                        .map(|name| (name.clone(), MenuAction::Focus(Some(name.clone())))),
                )
                .collect(),
            "Theme" => bark::theme::registered()
                .into_iter()
                .map(|(name, _, _)| (name.clone(), MenuAction::Theme(name)))
                .collect(),
            "Model" => self
                .state
                .models()
                .iter()
                .map(|model| {
                    (
                        model.clone(),
                        MenuAction::Command(vec![
                            "agent".into(),
                            "role".into(),
                            "set".into(),
                            "developer".into(),
                            model.clone(),
                        ]),
                    )
                })
                .collect(),
            "Reasoning effort" => {
                let model = &self.status.default_model;
                ["none", "minimal", "low", "medium", "high", "xhigh", "max"]
                    .into_iter()
                    .map(|effort| {
                        (
                            effort.into(),
                            MenuAction::Command(vec![
                                "agent".into(),
                                "role".into(),
                                "set".into(),
                                "developer".into(),
                                model.clone(),
                                "--thinking-effort".into(),
                                effort.into(),
                            ]),
                        )
                    })
                    .collect()
            }
            "History" => {
                // A bounded selector; PageUp can still reach every older segment.
                let last = self.transcript.latest();
                (last.saturating_sub(99)..=last)
                    .rev()
                    .map(|sequence| {
                        (
                            format!("Transcript segment {}", sequence + 1),
                            MenuAction::Segment(sequence),
                        )
                    })
                    .collect()
            }
            _ => vec![
                ("Component".into(), MenuAction::Menu("Component")),
                (
                    "Model (developer role; policy reapproval may be required)".into(),
                    MenuAction::Menu("Model"),
                ),
                (
                    "Reasoning effort (developer role)".into(),
                    MenuAction::Menu("Reasoning effort"),
                ),
                ("Theme".into(), MenuAction::Menu("Theme")),
                ("Session history".into(), MenuAction::Menu("History")),
                ("Follow latest output".into(), MenuAction::Follow),
            ],
        };
        self.overlay = Some(Overlay::Menu {
            title: title.into(),
            choices,
            selected: 0,
        });
    }

    pub(super) fn overlay_key(&mut self, key: KeyEvent) -> io::Result<()> {
        let Some(overlay) = self.overlay.take() else {
            return Ok(());
        };
        match overlay {
            Overlay::Help { mut scroll } => {
                match key.code {
                    KeyCode::Esc | KeyCode::F(1) => return Ok(()),
                    KeyCode::Up => scroll = scroll.saturating_sub(1),
                    KeyCode::Down => scroll = scroll.saturating_add(1),
                    KeyCode::PageUp => scroll = scroll.saturating_sub(self.viewport),
                    KeyCode::PageDown => scroll = scroll.saturating_add(self.viewport),
                    _ => {}
                }
                self.overlay = Some(Overlay::Help { scroll });
            }
            Overlay::Confirm { prompt, arguments } => match key.code {
                KeyCode::Char('y' | 'Y') => self.start(arguments)?,
                KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                    self.prompt_file.take();
                    self.append("\n\nOperation cancelled.\n")?;
                }
                _ => self.overlay = Some(Overlay::Confirm { prompt, arguments }),
            },
            Overlay::Menu {
                title,
                mut choices,
                mut selected,
            } => {
                if key.code == KeyCode::Esc {
                    return Ok(());
                }
                if key.code == KeyCode::Up {
                    selected = selected.saturating_sub(1);
                }
                if key.code == KeyCode::Down {
                    selected = (selected + 1).min(choices.len().saturating_sub(1));
                }
                if key.code != KeyCode::Enter || choices.is_empty() {
                    self.overlay = Some(Overlay::Menu {
                        title,
                        choices,
                        selected,
                    });
                    return Ok(());
                }
                let (_, action) = choices.remove(selected);
                match action {
                    MenuAction::Menu(title) => self.menu(title),
                    MenuAction::Focus(focus) => self.change_focus(focus)?,
                    MenuAction::Theme(theme) => self.set_theme(&theme)?,
                    MenuAction::Command(arguments) => {
                        if self.running.is_some() {
                            return Err(io::Error::other(
                                "wait for the running command before changing model/effort",
                            ));
                        }
                        self.append(&format!(
                            "\n\n## Settings command\n\n{}\n\n",
                            presentation::fence(
                                &arguments
                                    .iter()
                                    .map(|argument| quote(argument))
                                    .collect::<Vec<_>>()
                                    .join(" "),
                                "text"
                            )
                        ))?;
                        self.start(arguments)?;
                    }
                    MenuAction::Segment(segment) => {
                        self.follow = false;
                        self.load_segment(segment, false)?;
                    }
                    MenuAction::Follow => {
                        self.follow = true;
                        self.load_segment(self.transcript.latest(), true)?;
                    }
                }
            }
        }
        Ok(())
    }
}

pub(super) fn view_help() -> &'static str {
    "## Keyboard\n\n| Key | Action |\n| --- | --- |\n| Tab / Shift+Tab | Complete / select candidate |\n| Enter | Accept candidate or submit command |\n| Up / Down | Command history |\n| Left / Right, Home / End | Edit input |\n| Ctrl+Left / Right | Move by word |\n| Ctrl+A / E / U / K / W / Z | Start / end / delete before / after / word / undo |\n| PageUp / PageDown, mouse wheel | Browse transcript across segments |\n| Ctrl+Home / End | Oldest / latest transcript |\n| Alt+Left / Right | Horizontal transcript scroll |\n| F1 / F2 | Help / settings |\n| Escape | Dismiss completion or overlay |\n| Ctrl+C | Clear input or cancel owned command |\n| Ctrl+D (empty input) | Send command EOF; exit when idle |\n| Ctrl+Q | Quit and reap any active command |\n| exit | Leave workspace when idle |\n\nWhile a command runs, input answers its prompts. Answers are masked and not retained. For `prompt --file -`, enter the prompt then Ctrl+D on empty input. Model/effort settings update the existing developer role; they never approve execution policy. Transcript history is local diagnostic state, not canonical evidence.\n"
}
