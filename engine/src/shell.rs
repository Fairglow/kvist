//! Kvist Interactive Workspace Shell implementation.

use std::path::Path;
use std::process::Command;
use clap::Parser;
use rustyline::DefaultEditor;
use crate::{Result, KvistError, config};

/// Launches and runs the persistent interactive workspace shell (REPL).
pub fn run_shell(project_dir: &Path) -> Result<()> {
    // 1. Load active project configuration to establish the status line context
    let project_config = match config::load(project_dir) {
        Ok(cfg) => Some(cfg),
        Err(_) => None,
    };

    let sandbox_backend = project_config
        .as_ref()
        .and_then(|cfg| cfg.sandbox.as_ref().map(|sb| sb.backend.clone()))
        .unwrap_or_else(|| "none".to_owned());

    let default_model = project_config
        .as_ref()
        .and_then(|cfg| {
            cfg.agent
                .developer
                .models
                .first()
                .map(|m| m.name.clone())
        })
        .unwrap_or_else(|| "none".to_owned());

    // 2. Initialize Rustyline Editor
    let mut rl = DefaultEditor::new().map_err(|e| KvistError::SandboxUnavailable {
        runner: "shell".to_owned(),
        reason: format!("failed to initialize line reader: {e}"),
    })?;

    println!("==================================================");
    println!(" Kvist Interactive Workspace Shell");
    println!(" Type 'exit' or 'quit' or press Ctrl+D to exit.");
    println!("==================================================");

    loop {
        // Resolve dynamic branch name on each prompt refresh
        let branch = get_git_branch(project_dir).unwrap_or_else(|| "detached".to_owned());

        // Construct status bar prompt
        let prompt_str = format!(
            "kvist ({}) [sandbox: {}] [model: {}] > ",
            branch, sandbox_backend, default_model
        );

        match rl.readline(&prompt_str) {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                if line == "exit" || line == "quit" {
                    break;
                }

                // Split user line respecting quotes using agent_runtime utility
                let split_result = agent_runtime::split_raw_command(line);
                match split_result {
                    Ok((program, arguments)) => {
                        if program == "shell" {
                            println!("You are already in an active Kvist shell.");
                            continue;
                        }

                        // Prepend "kvist" to feed arguments list into our existing parser
                        let mut full_args = vec!["kvist".to_owned()];
                        full_args.push(program);
                        full_args.extend(arguments);

                        // Try to parse command using our top-level clap Cli definitions
                        match crate::cli::Cli::try_parse_from(&full_args) {
                            Ok(parsed_cli) => {
                                // Prevent recursive shell command inside the shell
                                if matches!(parsed_cli.command, crate::cli::Command::Shell(_)) {
                                    println!("You are already in an active Kvist shell.");
                                    continue;
                                }

                                // Execute command via cli::execute
                                match crate::cli::execute(parsed_cli.command, parsed_cli.json) {
                                    Ok(output) => {
                                        if !output.is_empty() {
                                            println!("{output}");
                                        }
                                    }
                                    Err(err) => {
                                        let _ = err.print();
                                    }
                                }
                            }
                            Err(err) => {
                                // Print clap's validation/formatting errors directly to the terminal
                                eprintln!("{err}");
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("error: failed to parse command input: {err}");
                    }
                }
                
                // Add command to input history
                let _ = rl.add_history_entry(line);
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(err) => {
                eprintln!("error: line read failure: {err}");
                break;
            }
        }
    }

    Ok(())
}

fn get_git_branch(project_dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_dir)
        .output()
        .ok()?;
    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !branch.is_empty() {
            return Some(branch);
        }
    }
    None
}
