//! Blad: Skott session viewer and transcript browser.
//!
//! Reads skott's structured session transcripts from .skott/runs/ and displays
//! them with full markdown rendering. Can also view standalone .md files.
//!
//! Usage:
//!   blad list                     - list recent sessions
//!   blad show <session-id>        - view a completed session
//!   blad stream <session-id>      - follow a live session
//!   blad view <file.md>           - view any markdown file
//!   blad export <session-id>      - export session to combined markdown
//!   blad <session-id|file.md>     - smart shortcut (show or view)

use blad::{
    ColorDepth, DiagramCharset, MathMode, MermaidMode, RenderOptions, Theme, display, session,
};
use clap::{Parser, ValueEnum};
use std::io::IsTerminal;
use std::path::PathBuf;

const DEFAULT_LOG_DIR: &str = ".skott/runs";

#[derive(Parser)]
#[clap(name = "blad", about = "Skott session viewer and markdown browser")]
pub struct Cli {
    /// Directory containing skott session runs (default: .skott/runs)
    #[clap(long, global = true, value_name = "PATH")]
    log_dir: Option<PathBuf>,

    /// Output plain text without interactive viewer
    #[clap(long, global = true)]
    dump: bool,

    /// Show line numbers in interactive viewer
    #[clap(long, global = true)]
    line_numbers: bool,

    /// Terminal appearance; auto uses environment hints without querying stdin
    #[clap(long, global = true, value_enum, default_value = "auto")]
    theme: ThemeChoice,

    /// Color policy; plain dumps always override this setting
    #[clap(long, global = true, value_enum, default_value = "auto")]
    color: ColorChoice,

    /// Project supported mathematics into Unicode, or preserve its source
    #[clap(long, global = true, value_enum, default_value = "unicode")]
    math: MathMode,

    /// Render admitted Mermaid diagrams as terminal text, or show source
    #[clap(long, global = true, value_enum, default_value = "text")]
    mermaid: MermaidMode,

    /// Character set for text diagrams
    #[clap(long, global = true, value_enum, default_value = "unicode")]
    diagram_charset: DiagramCharset,

    /// Session ID or file path (smart shortcut for show/view)
    target: Option<String>,

    #[clap(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ThemeChoice {
    Auto,
    Dark,
    Light,
    Terminal,
}

#[derive(Clone, Copy, ValueEnum)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

fn render_options(cli: &Cli) -> RenderOptions {
    let colorfgbg = std::env::var("COLORFGBG").unwrap_or_default();
    let theme = match cli.theme {
        ThemeChoice::Auto => match colorfgbg.split(';').next_back() {
            Some("7" | "15") => Theme::Light,
            _ => Theme::Dark,
        },
        ThemeChoice::Dark => Theme::Dark,
        ThemeChoice::Light => Theme::Light,
        ThemeChoice::Terminal => Theme::Terminal,
    };
    let term = std::env::var("TERM").unwrap_or_default();
    let colorterm = std::env::var("COLORTERM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
    let color_depth = match cli.color {
        ColorChoice::Never => ColorDepth::None,
        ColorChoice::Auto if no_color || !std::io::stdout().is_terminal() || term == "dumb" => {
            ColorDepth::None
        }
        _ if colorterm == "truecolor" || colorterm == "24bit" => ColorDepth::TrueColor,
        _ if term.contains("256color") => ColorDepth::Ansi256,
        _ => ColorDepth::Ansi16,
    };
    RenderOptions {
        theme,
        color_depth,
        plain: cli.dump,
        math: cli.math,
        mermaid: cli.mermaid,
        diagram_charset: cli.diagram_charset,
        ..RenderOptions::default()
    }
}

fn display_file(path: &std::path::Path, content: &str, cli: &Cli, options: &RenderOptions) {
    let result = if cli.dump {
        blad::MarkdownRenderer::with_options(options.clone())
            .render_markdown(content, &mut std::io::stdout().lock())
    } else {
        display::view_markdown_file_with_options(Some(path), content, cli.line_numbers, options)
    };
    report_io("display markdown", result);
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// List recent skott sessions
    List {
        #[clap(long, default_value = "20")]
        limit: usize,
    },
    /// View a completed session transcript
    Show { session_id: String },
    /// Follow a live session as it streams
    Stream { session_id: String },
    /// View a standalone markdown file
    View { path: PathBuf },
    /// Export a session to a combined markdown document
    Export {
        session_id: String,
        #[clap(long)]
        output: Option<PathBuf>,
    },
}

fn resolve_log_dir(cli: &Cli) -> PathBuf {
    match &cli.log_dir {
        Some(path) => path.clone(),
        None => PathBuf::from(DEFAULT_LOG_DIR),
    }
}

/// Report an I/O failure and exit with the contract's error code (1).
fn report_io(context: &str, result: std::io::Result<()>) {
    if let Err(error) = result {
        eprintln!("Failed to {context}: {error}");
        std::process::exit(1);
    }
}

/// Print an error to stderr and exit with the contract's error code (1).
///
/// Every failed lookup (missing file, unknown session, unreadable path) is an
/// error per the contract, not a soft success: callers of the CLI must be able
/// to distinguish them from a successful, empty result by the exit status.
fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

fn main() {
    let cli = Cli::parse();
    let log_dir = resolve_log_dir(&cli);
    let options = render_options(&cli);

    // Smart shortcut: if no subcommand but a target was given, try show then view
    if cli.command.is_none() {
        if let Some(target) = &cli.target {
            if target.ends_with(".md") || target.ends_with(".markdown") {
                // Treat as a file path
                match std::fs::read_to_string(target) {
                    Ok(content) => {
                        display_file(std::path::Path::new(target), &content, &cli, &options);
                    }
                    Err(e) => fail(&format!("Error reading file: {e}")),
                }
                return;
            }

            // Try as a session directory path
            if PathBuf::from(target).is_dir() {
                match session::load_session_by_path(&PathBuf::from(target)) {
                    Some(s) => {
                        report_io(
                            "show session",
                            display::show_session_with_options(&s, &options),
                        );
                    }
                    None => fail(&format!("Error loading session from path: {target}")),
                }
                return;
            }

            // Try as a session ID
            match session::load_session(&log_dir, target) {
                Some(s) => {
                    report_io(
                        "show session",
                        display::show_session_with_options(&s, &options),
                    );
                }
                None => fail(&format!("Session not found: {target}")),
            }
        } else {
            fail("Usage: blad [OPTIONS] <command> [ARGS]");
        }
        return;
    }

    match &cli.command {
        Some(Command::List { limit }) => {
            let sessions = session::list_sessions(&log_dir, *limit);
            if sessions.is_empty() {
                println!("No sessions found in {}", log_dir.display());
                return;
            }
            report_io("list sessions", display::list_sessions(&sessions));
        }
        Some(Command::Show { session_id }) => {
            // Try as a path first, then as an ID
            if PathBuf::from(&session_id).is_dir() {
                match session::load_session_by_path(&PathBuf::from(&session_id)) {
                    Some(s) => {
                        report_io(
                            "show session",
                            display::show_session_with_options(&s, &options),
                        );
                    }
                    None => fail(&format!("Error loading session from path: {session_id}")),
                }
            } else {
                match session::load_session(&log_dir, session_id) {
                    Some(s) => {
                        report_io(
                            "show session",
                            display::show_session_with_options(&s, &options),
                        );
                    }
                    None => fail(&format!("Session not found: {session_id}")),
                }
            }
        }
        Some(Command::Stream { session_id }) => match session::load_session(&log_dir, session_id) {
            Some(s) => report_io(
                "stream session",
                display::stream_session_with_options(&s, &options),
            ),
            None => fail(&format!("Session not found: {session_id}")),
        },
        Some(Command::View { path }) => match std::fs::read_to_string(path) {
            Ok(content) => {
                display_file(path, &content, &cli, &options);
            }
            Err(e) => fail(&format!("Error reading file: {e}")),
        },
        Some(Command::Export { session_id, output }) => {
            match session::load_session(&log_dir, session_id) {
                Some(s) => report_io(
                    "export session",
                    display::export_session(&s, output.as_deref()),
                ),
                None => fail(&format!("Session not found: {session_id}")),
            }
        }
        None => {
            // Default: list recent sessions
            let sessions = session::list_sessions(&log_dir, 20);
            if sessions.is_empty() {
                println!("No sessions found in {}", log_dir.display());
            } else {
                report_io("list sessions", display::list_sessions(&sessions));
            }
        }
    }
}
