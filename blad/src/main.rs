//! Standalone Markdown viewer with optional Skott session integration.

#[cfg(feature = "sessions")]
use blad::session;
use blad::{ColorDepth, DiagramCharset, MathMode, MermaidMode, RenderOptions, Theme, display};
use clap::{CommandFactory, Parser, ValueEnum};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

mod file_input;
mod theme_file;

#[derive(Parser)]
#[command(
    name = "blad",
    about = "Terminal Markdown viewer",
    version,
    propagate_version = true,
    subcommand_precedence_over_arg = true
)]
pub struct Cli {
    /// Directory containing skott session runs (default: .skott/runs)
    #[clap(long, global = true, value_name = "PATH")]
    #[cfg(feature = "sessions")]
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

    /// Load an explicit JSON theme/palette file (maximum 64 KiB)
    #[clap(long, global = true, value_name = "PATH")]
    theme_file: Option<PathBuf>,

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

    /// Files in tab order; '-' reads redirected stdin (also used with no files)
    #[arg(value_name = "FILE", num_args = 1..)]
    files: Vec<PathBuf>,

    #[cfg(feature = "sessions")]
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

fn render_options(cli: &Cli) -> std::io::Result<RenderOptions> {
    let file = cli
        .theme_file
        .as_deref()
        .map(theme_file::ThemeFile::load)
        .transpose()?;
    let colorfgbg = std::env::var("COLORFGBG").unwrap_or_default();
    let theme = match cli.theme {
        ThemeChoice::Auto => file
            .as_ref()
            .and_then(|file| file.theme)
            .unwrap_or_else(|| match colorfgbg.split(';').next_back() {
                Some("7" | "15") => Theme::Light,
                _ => Theme::Dark,
            }),
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
    Ok(RenderOptions {
        theme,
        color_depth,
        plain: cli.dump
            || ((!std::io::stdout().is_terminal() || !std::io::stdin().is_terminal())
                && !matches!(cli.color, ColorChoice::Always)),
        math: cli.math,
        mermaid: cli.mermaid,
        diagram_charset: cli.diagram_charset,
        palette: file
            .map(|file| file.palette(blad::Palette::for_theme(theme)))
            .transpose()?,
    })
}

#[cfg(feature = "sessions")]
#[derive(clap::Subcommand)]
pub enum Command {
    /// List recent skott sessions
    List {
        #[clap(long, default_value = "20")]
        limit: usize,
    },
    /// Read a session snapshot by ID, prefix or directory
    Session { session_id: String },
    /// Export a session to a combined markdown document
    Export {
        session_id: String,
        #[clap(long)]
        output: Option<PathBuf>,
    },
}

/// Report an I/O failure and exit with the contract's error code (1).
fn report_io(context: &str, result: std::io::Result<()>) {
    if let Err(error) = result.and_then(|()| std::io::stdout().flush()) {
        if error.kind() == std::io::ErrorKind::BrokenPipe {
            return;
        }
        eprintln!("Failed to {context}: {}", error.to_string().escape_debug());
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
    blad::logging::init_logging();
    let mut cli = Cli::parse();
    #[cfg(feature = "sessions")]
    if cli.command.is_some() && !cli.files.is_empty() {
        fail("Files and session commands cannot be combined; use -- before command-like filenames");
    }
    #[cfg(feature = "sessions")]
    let has_command = cli.command.is_some();
    #[cfg(not(feature = "sessions"))]
    let has_command = false;
    if cli.files.is_empty() && !has_command {
        if std::io::stdin().is_terminal() {
            report_io(
                "print help",
                Cli::command().write_help(&mut std::io::stderr()),
            );
            std::process::exit(2);
        }
        cli.files.push(PathBuf::from("-"));
    }
    let options = render_options(&cli).unwrap_or_else(|error| {
        fail(&format!(
            "Failed to load theme: {}",
            error.to_string().escape_debug()
        ))
    });

    #[cfg(feature = "sessions")]
    if let Some(command) = &cli.command {
        report_io("read session", run_session_command(command, &cli, &options));
        return;
    }
    report_io(
        "display markdown",
        display::view_markdown_files_with_options(&cli.files, cli.line_numbers, &options),
    );
}

#[cfg(feature = "sessions")]
fn run_session_command(
    command: &Command,
    cli: &Cli,
    options: &RenderOptions,
) -> std::io::Result<()> {
    let log_dir = cli
        .log_dir
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new(".skott/runs"));
    match command {
        Command::List { limit } => {
            let sessions = session::list_sessions(log_dir, *limit)?;
            if sessions.is_empty() {
                writeln!(
                    std::io::stdout().lock(),
                    "No sessions found in {}",
                    log_dir.display().to_string().escape_debug()
                )?;
                return Ok(());
            }
            display::list_sessions(&sessions)
        }
        Command::Session { session_id } => {
            let loaded = if std::path::Path::new(session_id).is_dir() {
                session::load_session_by_path(std::path::Path::new(session_id))
            } else {
                session::load_session(log_dir, session_id)
            }?;
            display::show_session_with_options(&loaded, options)
        }
        Command::Export { session_id, output } => {
            let loaded = session::load_session(log_dir, session_id)?;
            display::export_session(&loaded, output.as_deref())
        }
    }
}
