//! Theme definitions, resolution, and user customization.
//!
//! A [`Theme`] is a cheap, copyable handle into a process-wide theme
//! registry. Two built-in themes are always present — `dark` (the default;
//! high-contrast light text on the terminal's background, with the agent
//! result rendered on a true black surface) and `light` (dark text with a
//! white agent-result surface). Users customize the palette through a TOML
//! spec (see [`parse_theme_spec`]) placed at `~/.config/kvist/theme.toml`
//! (or `XDG_CONFIG_HOME/kvist/theme.toml`), referenced by name through the
//! `KVIST_THEME` environment variable, `.kvist/theme`, or
//! `~/.config/kvist/theme`, or by path directly.
//!
//! Resolution honors the established color-contract: `NO_COLOR` (any value),
//! `CLICOLOR=0`, `TERM=dumb`, and a non-terminal stdout all degrade to plain
//! text; `CLICOLOR_FORCE` (any value but `0`) keeps styling. When no explicit
//! preference is set, the terminal's default background is probed with an
//! OSC 11 query and the matching surface is chosen (dark default).

use std::{
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use crate::KvistError;

/// Maximum accepted size of a user theme spec.
pub const MAX_THEME_SPEC_BYTES: u64 = 8 * 192;

/// Registry index of the built-in dark theme.
pub const DARK_ID: u16 = 0;
/// Registry index of the built-in light theme.
pub const LIGHT_ID: u16 = 1;

/// One semantic SGR palette: every style the shell emits is a named role,
/// so a theme is a single lookup table and a renderer never hard-codes a
/// color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    /// Prompt verb (`kvist`).
    pub prompt: String,
    /// Component context segment (`maerg/`).
    pub component: String,
    /// Failure marker after a failed command.
    pub failure: String,
    /// The prompt indicator (`❯`).
    pub indicator: String,
    /// Box rules and the prompt separator line.
    pub border: String,
    /// Secondary text (status badge, timestamps, hints).
    pub dim: String,
    /// Errors, failures, stale state.
    pub red: String,
    /// Success, live state.
    pub green: String,
    /// Caution, the next-ready marker.
    pub yellow: String,
    /// Informational primary text.
    pub blue: String,
    /// Component and secondary context.
    pub magenta: String,
    /// In-progress state.
    pub cyan: String,
    /// Background of the agent result surface.
    pub agent_background: String,
    /// Foreground of the agent result surface.
    pub agent_foreground: String,
}

impl Palette {
    /// The default dark palette: bright text on the terminal background,
    /// agent result on true black.
    pub fn dark() -> Self {
        Self {
            prompt: "1;34".to_owned(),
            component: "1;35".to_owned(),
            failure: "1;31".to_owned(),
            indicator: "1;32".to_owned(),
            border: "2".to_owned(),
            dim: "2".to_owned(),
            red: "31".to_owned(),
            green: "32".to_owned(),
            yellow: "33".to_owned(),
            blue: "34".to_owned(),
            magenta: "35".to_owned(),
            cyan: "36".to_owned(),
            agent_background: "40".to_owned(),
            agent_foreground: "37".to_owned(),
        }
    }

    /// The light palette: dark text, agent result on white.
    pub fn light() -> Self {
        Self {
            agent_background: "47".to_owned(),
            agent_foreground: "30".to_owned(),
            ..Self::dark()
        }
    }

    /// SGR code combining the agent foreground and background.
    pub fn agent_result_code(&self) -> String {
        format!("{};{}", self.agent_foreground, self.agent_background)
    }
}

/// One registered theme definition.
#[derive(Debug, Clone)]
pub struct ThemeDef {
    name: String,
    palette: Palette,
    builtin: bool,
}

fn registry() -> &'static Mutex<Vec<ThemeDef>> {
    static REGISTRY: OnceLock<Mutex<Vec<ThemeDef>>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        Mutex::new(vec![
            ThemeDef {
                name: "dark".to_owned(),
                palette: Palette::dark(),
                builtin: true,
            },
            ThemeDef {
                name: "light".to_owned(),
                palette: Palette::light(),
                builtin: true,
            },
        ])
    })
}

/// Registers a custom theme definition, replacing an existing entry with the
/// same name. Returns the registry id of the theme.
pub fn register(def: ThemeDef) -> u16 {
    let mut guard = registry().lock().expect("theme registry poisoned");
    if let Some(existing) = guard.iter_mut().find(|d| d.name == def.name) {
        existing.name = def.name.clone();
        existing.palette = def.palette.clone();
        existing.builtin = def.builtin;
        return guard.iter().position(|d| d.name == def.name).unwrap_or(0) as u16;
    }
    guard.push(def);
    (guard.len() - 1) as u16
}

/// The resolved palette for a theme id, degrading to the dark palette.
pub fn palette_for(id: u16) -> Palette {
    let guard = registry().lock().expect("theme registry poisoned");
    guard
        .get(id as usize)
        .map(|def| def.palette.clone())
        .unwrap_or_else(Palette::dark)
}

/// The display name of a theme id, degrading to `dark`.
pub fn name_for(id: u16) -> String {
    let guard = registry().lock().expect("theme registry poisoned");
    guard
        .get(id as usize)
        .map(|def| def.name.clone())
        .unwrap_or_else(|| "dark".to_owned())
}

/// A snapshot of the registry: (name, id, built-in) in registration order.
pub fn registered() -> Vec<(String, u16, bool)> {
    let guard = registry().lock().expect("theme registry poisoned");
    guard
        .iter()
        .enumerate()
        .map(|(i, def)| (def.name.clone(), i as u16, def.builtin))
        .collect()
}

/// A resolved terminal theme: a copyable handle into the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Whether ANSI styling is emitted.
    enabled: bool,
    /// Registry id of the active palette.
    id: u16,
}

impl Default for Theme {
    fn default() -> Self {
        Self::plain()
    }
}

impl Theme {
    /// A plain-text theme: every style helper returns its input unchanged.
    pub const fn plain() -> Self {
        Self {
            enabled: false,
            id: DARK_ID,
        }
    }

    /// The built-in dark theme with styling enabled.
    pub const fn dark() -> Self {
        Self {
            enabled: true,
            id: DARK_ID,
        }
    }

    /// The built-in light theme with styling enabled.
    pub const fn light() -> Self {
        Self {
            enabled: true,
            id: LIGHT_ID,
        }
    }

    /// A styled theme at an explicit registry id (a custom user theme).
    pub const fn by_id(id: u16) -> Self {
        Self { enabled: true, id }
    }

    /// The dark theme with styling enabled. Retained as an alias so the
    /// existing plain/enabled pair keeps working unchanged.
    pub const fn enabled() -> Self {
        Self::dark()
    }

    /// Whether ANSI styling is emitted.
    pub const fn is_enabled(self) -> bool {
        self.enabled
    }

    /// The display name of the theme's palette.
    pub fn name(self) -> String {
        name_for(self.id)
    }

    /// The resolved palette for this theme.
    pub fn palette(self) -> Palette {
        palette_for(self.id)
    }

    /// Resolves the session theme for one project directory.
    ///
    /// Plain-text degradation (see the module docs) wins first. Otherwise the
    /// preference chain is: `KVIST_THEME` environment variable, the
    /// project-local `.kvist/theme`, the user preference at
    /// `~/.config/kvist/theme`, an OSC 11 terminal-background probe, and the
    /// dark default. A preference that names a path loads that spec; a name
    /// resolves against the built-ins plus the user theme file, which is
    /// loaded (once) from `~/.config/kvist/theme.toml`.
    pub fn resolve(project_dir: &Path) -> Self {
        if env_disables_color() {
            return Self::plain();
        }
        let forced = std::env::var_os("CLICOLOR_FORCE").is_some_and(|value| value != "0");
        let enabled = forced || io::stdout().is_terminal();

        load_user_theme_file();
        let preference = std::env::var_os("KVIST_THEME")
            .map(|v| v.to_string_lossy().into_owned())
            .filter(|v| !v.is_empty())
            .or_else(|| read_preference_file(project_dir.join(".kvist").join("theme")))
            .or_else(|| read_preference_file(user_preference_path()));
        let id = match preference.as_deref() {
            Some(value) => match match_preference(value) {
                Preference::Dark => DARK_ID,
                Preference::Light => LIGHT_ID,
                Preference::Named(id) => id,
                Preference::Path(path) => match load_theme_file(&path) {
                    Ok(id) => id,
                    Err(error) => {
                        eprintln!("warning: {error}; falling back to the detected theme");
                        detect_id()
                    }
                },
            },
            None => detect_id(),
        };
        Self { enabled, id }
    }

    /// Applies one SGR code, or returns the text unchanged when styling is
    /// disabled.
    pub fn style(self, code: &str, text: &str) -> String {
        if !self.enabled {
            return text.to_owned();
        }
        format!("\x1b[{code}m{text}\x1b[0m")
    }

    /// Bold text.
    pub fn bold(self, text: &str) -> String {
        self.style("1", text)
    }

    /// Faint (dimmed) text, in the theme's dim role.
    pub fn dim(self, text: &str) -> String {
        self.style(&self.palette().dim, text)
    }

    /// Prompt verb (`kvist`).
    pub fn prompt(self, text: &str) -> String {
        self.style(&self.palette().prompt, text)
    }

    /// Component context segment.
    pub fn component(self, text: &str) -> String {
        self.style(&self.palette().component, text)
    }

    /// Failure marker.
    pub fn failure(self, text: &str) -> String {
        self.style(&self.palette().failure, text)
    }

    /// Prompt indicator (`❯`).
    pub fn indicator(self, text: &str) -> String {
        self.style(&self.palette().indicator, text)
    }

    /// Red text (errors, failures, stale state).
    pub fn red(self, text: &str) -> String {
        self.style(&self.palette().red, text)
    }

    /// Green text (success, live state).
    pub fn green(self, text: &str) -> String {
        self.style(&self.palette().green, text)
    }

    /// Yellow text (caution, the next-ready marker).
    pub fn yellow(self, text: &str) -> String {
        self.style(&self.palette().yellow, text)
    }

    /// Blue text (informational primary).
    pub fn blue(self, text: &str) -> String {
        self.style(&self.palette().blue, text)
    }

    /// Magenta text (secondary context).
    pub fn magenta(self, text: &str) -> String {
        self.style(&self.palette().magenta, text)
    }

    /// Cyan text (in-progress state).
    pub fn cyan(self, text: &str) -> String {
        self.style(&self.palette().cyan, text)
    }

    /// One line of the agent result surface (foreground on background).
    pub fn agent_result(self, text: &str) -> String {
        self.style(&self.palette().agent_result_code(), text)
    }
}

/// Whether the standard color contract disables styling.
pub fn env_disables_color() -> bool {
    std::env::var_os("NO_COLOR").is_some()
        || std::env::var_os("CLICOLOR").is_some_and(|value| value == "0")
        || std::env::var_os("TERM").is_some_and(|value| value == "dumb")
}

/// The kind of theme preference a preference value expresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preference {
    /// The built-in dark theme.
    Dark,
    /// The built-in light theme.
    Light,
    /// A registered theme by name.
    Named(u16),
    /// An explicit path to a theme spec file.
    Path(PathBuf),
}

/// Classifies one preference value. A value containing a path separator or
/// ending in `.toml` is treated as a file path; `dark`/`light` (any case)
/// select the built-ins; anything else is a theme name.
pub fn match_preference(value: &str) -> Preference {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Preference::Dark;
    }
    let looks_like_path = trimmed.contains('/')
        || trimmed.contains(std::path::MAIN_SEPARATOR)
        || trimmed.to_ascii_lowercase().ends_with(".toml");
    if looks_like_path {
        return Preference::Path(PathBuf::from(trimmed));
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "dark" => Preference::Dark,
        "light" => Preference::Light,
        _ => match registered()
            .into_iter()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(trimmed))
        {
            Some((_, id, _)) => Preference::Named(id),
            None => Preference::Path(PathBuf::from(trimmed)),
        },
    }
}

/// The user theme spec path: `$XDG_CONFIG_HOME/kvist/theme.toml`, defaulting
/// to `~/.config/kvist/theme.toml`.
pub fn user_theme_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .map(|home| home.join(".config"));
    match base {
        Some(base) => base.join("kvist").join("theme.toml"),
        None => PathBuf::from("theme.toml"),
    }
}

/// The user theme-preference file path (`~/.config/kvist/theme`).
pub fn user_preference_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .map(|home| home.join(".config"));
    match base {
        Some(base) => base.join("kvist").join("theme"),
        None => PathBuf::from("theme"),
    }
}

/// Reads a preference file (one line: a name or a path), tolerating absence.
fn read_preference_file(path: PathBuf) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let line = contents.lines().next()?.trim().to_owned();
    (!line.is_empty()).then_some(line)
}

/// Persists the project-local theme preference (validated: a built-in name,
/// a registered name, or a path).
pub fn save_preference(project_dir: &Path, value: &str) -> Result<(), KvistError> {
    let matched = match_preference(value);
    let valid = matches!(
        matched,
        Preference::Dark | Preference::Light | Preference::Named(_)
    ) || matches!(&matched, Preference::Path(p) if p.is_absolute());
    if !valid {
        return Err(KvistError::ThemePreferenceInvalid {
            value: value.to_owned(),
        });
    }
    let kvist_dir = project_dir.join(".kvist");
    std::fs::create_dir_all(&kvist_dir).map_err(|source| KvistError::Io {
        operation: "create `.kvist` directory for the theme preference",
        path: kvist_dir.clone(),
        source,
    })?;
    let path = kvist_dir.join("theme");
    let tmp = kvist_dir.join(format!(".theme.tmp.{}", std::process::id()));
    std::fs::write(&tmp, format!("{value}\n")).map_err(|source| KvistError::Io {
        operation: "write the theme preference",
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, &path).map_err(|source| KvistError::Io {
        operation: "persist the theme preference",
        path,
        source,
    })
}

/// Loads and registers the user theme spec, once per process. A missing or
/// unreadable file is silently ignored (it is optional); an invalid spec is
/// reported once so a broken file does not silently vanish.
fn load_user_theme_file() {
    static LOADED: OnceLock<()> = OnceLock::new();
    if LOADED.get().is_some() {
        return;
    }
    if let Err(error) = load_theme_file(&user_theme_path()) {
        // Only report a file that exists: a missing user theme is normal.
        if user_theme_path().exists() {
            eprintln!("warning: {error}");
        }
    }
    let _ = LOADED.set(());
}

/// Loads one theme spec file, registers it, and returns its id.
pub fn load_theme_file(path: &Path) -> Result<u16, KvistError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|source| KvistError::Io {
        operation: "inspect theme spec",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_THEME_SPEC_BYTES {
        return Err(KvistError::ThemeSpecTooLarge {
            path: path.to_path_buf(),
            max_bytes: MAX_THEME_SPEC_BYTES,
        });
    }
    let contents = std::fs::read_to_string(path).map_err(|source| KvistError::Io {
        operation: "read theme spec",
        path: path.to_path_buf(),
        source,
    })?;
    let spec = parse_theme_spec(&contents)?;
    let id = register(ThemeDef {
        name: spec.name.clone(),
        palette: palette_for_spec(&spec),
        builtin: false,
    });
    Ok(id)
}

/// Parsed fields of one theme spec document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeSpec {
    /// Theme name: 1..=32 chars of `[a-z0-9-]` (lower-cased on parse).
    pub name: String,
    /// Color overrides per role; missing roles keep the mode's palette.
    pub colors: std::collections::BTreeMap<String, String>,
    /// Base mode: `dark` (default) or `light`.
    pub mode: String,
}

/// Parses and validates a theme spec document.
///
/// Format (TOML):
/// ```toml
/// name = "midnight"
/// mode = "dark"                # "dark" (default) or "light"
/// [colors]                     # every role optional
/// prompt = "#4f9ef7"           # #rrggbb hex
/// border = "gray"              # named ANSI color
/// agent_background = "0"       # 256-color index
/// ```
pub fn parse_theme_spec(contents: &str) -> Result<ThemeSpec, KvistError> {
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        name: String,
        #[serde(default)]
        mode: String,
        #[serde(default)]
        colors: std::collections::BTreeMap<String, RawColor>,
    }
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum RawColor {
        Named(String),
        Index(u8),
    }

    let raw: Raw = toml::from_str(contents).map_err(|error| KvistError::ThemeSpecInvalid {
        reason: format!("invalid TOML: {error}"),
    })?;
    let name = raw.name.trim().to_ascii_lowercase();
    if name.is_empty()
        || name.len() > 32
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(KvistError::ThemeSpecInvalid {
            reason: "`name` must be 1-32 characters of [a-z0-9-]".to_owned(),
        });
    }
    let mode = raw.mode.trim().to_ascii_lowercase();
    let mode = if mode.is_empty() {
        "dark"
    } else {
        mode.as_str()
    };
    if mode != "dark" && mode != "light" {
        return Err(KvistError::ThemeSpecInvalid {
            reason: "`mode` must be \"dark\" or \"light\" (default dark)".to_owned(),
        });
    }
    let mode = mode.to_owned();
    let mut colors = std::collections::BTreeMap::new();
    for (role, value) in raw.colors {
        let role = role.trim().to_ascii_lowercase();
        if !KNOWN_ROLES.contains(&role.as_str()) {
            return Err(KvistError::ThemeSpecInvalid {
                reason: format!(
                    "unknown color role `{role}`; known: {}",
                    KNOWN_ROLES.join(", ")
                ),
            });
        }
        let raw_value = match value {
            RawColor::Named(v) => v,
            RawColor::Index(i) => i.to_string(),
        };
        let parsed = parse_color(&raw_value).map_err(|reason| KvistError::ThemeSpecInvalid {
            reason: format!("`{role}`: {reason}"),
        })?;
        let _ = parsed;
        colors.insert(role, raw_value);
    }
    Ok(ThemeSpec { name, colors, mode })
}

/// The color roles a theme spec may override.
pub const KNOWN_ROLES: [&str; 14] = [
    "prompt",
    "component",
    "failure",
    "indicator",
    "border",
    "dim",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "agent_background",
    "agent_foreground",
];

/// One parsed color: its foreground and background SGR encodings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorSpec {
    /// Foreground SGR (e.g. `31`, `38;5;208`, `38;2;255;0;0`).
    pub foreground: String,
    /// Background SGR (e.g. `41`, `48;5;237`, `48;2;255;0;0`).
    pub background: String,
}

/// Parses one color value: `#rrggbb`/`#rgb` hex, a named ANSI color, or a
/// 0-255 index.
pub fn parse_color(value: &str) -> Result<ColorSpec, String> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let expanded = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 => hex.to_owned(),
            other => {
                return Err(format!(
                    "hex color must be #rgb or #rrggbb (got {other} digits)"
                ));
            }
        };
        let r = u8::from_str_radix(&expanded[0..2], 16).map_err(|_| "invalid hex".to_owned())?;
        let g = u8::from_str_radix(&expanded[2..4], 16).map_err(|_| "invalid hex".to_owned())?;
        let b = u8::from_str_radix(&expanded[4..6], 16).map_err(|_| "invalid hex".to_owned())?;
        return Ok(ColorSpec {
            foreground: format!("38;2;{r};{g};{b}"),
            background: format!("48;2;{r};{g};{b}"),
        });
    }
    let lower = value.to_ascii_lowercase();
    let named = [
        ("black", "30", "40"),
        ("red", "31", "41"),
        ("green", "32", "42"),
        ("yellow", "33", "43"),
        ("blue", "34", "44"),
        ("magenta", "35", "45"),
        ("cyan", "36", "46"),
        ("white", "37", "47"),
        ("gray", "90", "100"),
        ("grey", "90", "100"),
        ("bright-black", "90", "100"),
        ("bright-red", "91", "41"),
        ("bright-green", "92", "42"),
        ("bright-yellow", "93", "43"),
        ("bright-blue", "94", "44"),
        ("bright-magenta", "95", "45"),
        ("bright-cyan", "96", "46"),
        ("bright-white", "97", "100"),
    ];
    if let Some((_, fg, bg)) = named.iter().find(|(n, _, _)| *n == lower) {
        return Ok(ColorSpec {
            foreground: (*fg).to_owned(),
            background: (*bg).to_owned(),
        });
    }
    if let Ok(index) = value.parse::<u8>() {
        return Ok(ColorSpec {
            foreground: format!("38;5;{index}"),
            background: format!("48;5;{index}"),
        });
    }
    Err("expected a #rrggbb hex color, a named ANSI color, or a 0-255 index".to_owned())
}

/// Builds the effective palette for one parsed spec: the mode's base palette
/// with each overridden role replaced by the parsed color. Foreground roles
/// take the foreground encoding; `agent_background` takes the background
/// encoding; `agent_foreground` takes the foreground encoding.
pub fn palette_for_spec(spec: &ThemeSpec) -> Palette {
    let mut palette = if spec.mode == "light" {
        Palette::light()
    } else {
        Palette::dark()
    };
    for (role, value) in &spec.colors {
        let Ok(color) = parse_color(value) else {
            continue;
        };
        match role.as_str() {
            "prompt" => palette.prompt = color.foreground,
            "component" => palette.component = color.foreground,
            "failure" => palette.failure = color.foreground,
            "indicator" => palette.indicator = color.foreground,
            "border" => palette.border = color.foreground,
            "dim" => palette.dim = color.foreground,
            "red" => palette.red = color.foreground,
            "green" => palette.green = color.foreground,
            "yellow" => palette.yellow = color.foreground,
            "blue" => palette.blue = color.foreground,
            "magenta" => palette.magenta = color.foreground,
            "cyan" => palette.cyan = color.foreground,
            "agent_background" => palette.agent_background = color.background,
            "agent_foreground" => palette.agent_foreground = color.foreground,
            _ => {}
        }
    }
    palette
}

/// The default of `detect_id`: probes the terminal background (OSC 11),
/// falling back to dark.
fn detect_id() -> u16 {
    match probe_terminal_background() {
        Some(rgb) if !is_dark_surface(rgb) => LIGHT_ID,
        _ => DARK_ID,
    }
}

/// Whether an (r, g, b) surface is dark by relative luminance.
pub fn is_dark_surface(rgb: (u8, u8, u8)) -> bool {
    let (r, g, b) = rgb;
    let luminance = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
    luminance < 128.0
}

/// Renders a live preview of one theme: the prompt, the separator rule, the
/// agent result surface, and the status badge, drawn in that theme.
pub fn render_theme_preview(theme: Theme, width: usize) -> String {
    let inner = width.saturating_sub(4).max(20);
    let mut rows: Vec<String> = Vec::new();
    rows.push(format!(
        "{} {} {} task run",
        theme.prompt("kvist"),
        theme.dim("(main)"),
        theme.indicator("❯")
    ));
    rows.push(super::style::separator_rule(theme, width));
    rows.push(theme.agent_result(&super::style::pad_right(
        "Agent result on the theme's black surface",
        inner,
    )));
    rows.push(theme.dim("[bubblewrap · 🔒 1 · ollama]"));
    let titled = super::style::titled_box(theme, &preview_title(theme), &rows, Some(width));
    format!(
        "{}\n{}\n{}",
        titled.top,
        titled.rows.join("\n"),
        titled.bottom
    )
}

/// The preview box title for one theme.
fn preview_title(theme: Theme) -> String {
    let source = if theme.id() == DARK_ID || theme.id() == LIGHT_ID {
        "built-in".to_owned()
    } else {
        "user".to_owned()
    };
    format!("{} — {}", theme.name(), source)
}

/// Renders the overview shown by the `theme` builtin: the current theme
/// first, every available theme as a live preview, and the customization
/// pointers.
pub fn render_theme_overview(current: Theme, width: usize) -> String {
    let mut out = format!(
        "{}\n",
        current.bold(&format!("Current theme: {}", current.name()))
    );
    let entries = registered();
    for (name, id, builtin) in &entries {
        let theme = Theme::by_id(*id);
        let marker = if *id == current.id() {
            " (current)"
        } else {
            ""
        };
        out.push_str(&format!("\n{}\n", theme.bold(&format!("{name}{marker}"))));
        let source = if *builtin { "built-in" } else { "user" };
        out.push_str(&format!("  {}\n", theme.dim(source)));
        out.push_str(&render_theme_preview(theme, width));
    }
    out.push_str(&format!(
        "\n{}\n",
        current.dim(&format!(
            "Switch: `theme set NAME` (persisted to .kvist/theme). Customize: {}",
            user_theme_path().display()
        ))
    ));
    out
}

impl Theme {
    /// The registry id of this theme (for equality with registered themes).
    pub const fn id(self) -> u16 {
        self.id
    }
}

/// Queries the terminal for its default background color (OSC 11).
///
/// The query is only issued when stdin and stdout are the same interactive
/// terminal; a non-responding terminal (or a timeout of 150 ms) yields
/// `None` and the dark default applies.
pub fn probe_terminal_background() -> Option<(u8, u8, u8)> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return None;
    }
    let raw = RawModeGuard::enter().ok()?;
    let mut stdout = io::stdout();
    if stdout.write_all(b"\x1b]11;?\x1b\\").is_err() || stdout.flush().is_err() {
        let _ = raw;
        return None;
    }
    let mut data: Vec<u8> = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(150);
    let mut byte = [0u8; 1];
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    while Instant::now() < deadline {
        match handle.read(&mut byte) {
            Ok(1) => {
                data.push(byte[0]);
                if data.len() > 256 {
                    return None;
                }
                if let Some(rgb) = parse_osc11(&data) {
                    return Some(rgb);
                }
            }
            // EOF, a partial multi-byte read, or an IO error: stop probing.
            Ok(_) => break,
            Err(_) => break,
        }
    }
    None
}

/// Restores the captured terminal state on drop.
struct RawModeGuard(Option<nix::sys::termios::Termios>);

impl RawModeGuard {
    /// Enters raw mode on the controlling terminal, capturing the previous
    /// state for restore. `/dev/tty` is used (via its `AsFd` impl) so no
    /// raw-fd handling is needed.
    fn enter() -> io::Result<Self> {
        use nix::sys::termios::{self, SetArg};
        let tty = std::fs::File::open("/dev/tty")?;
        let original = termios::tcgetattr(&tty)?;
        let mut raw = original.clone();
        termios::cfmakeraw(&mut raw);
        termios::tcsetattr(&tty, SetArg::TCSANOW, &raw)?;
        Ok(Self(Some(original)))
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if let (Some(original), Ok(tty)) = (self.0.take(), std::fs::File::open("/dev/tty")) {
            let _ =
                nix::sys::termios::tcsetattr(&tty, nix::sys::termios::SetArg::TCSANOW, &original);
        }
    }
}

/// Parses an OSC 11 background response out of the accumulated bytes.
///
/// Accepted forms: `ESC]11;rgb:RR/GG/BB ST` (ST = ESC\) and the BEL-
/// terminated variant; each channel is one to four hex digits. Returns
/// `None` while the response has not terminated yet.
pub fn parse_osc11(data: &[u8]) -> Option<(u8, u8, u8)> {
    let text = std::str::from_utf8(data).ok()?;
    let marker = "\x1b]11;rgb:";
    let start = text.find(marker)? + marker.len();
    let rest = &text[start..];
    // The response terminates with a BEL or a string terminator (ESC\").
    let (body, terminated) = if let Some(i) = rest.find('\x07') {
        (&rest[..i], true)
    } else if let Some(i) = rest.strip_suffix("\\").and_then(|s| s.rfind('\x1b')) {
        (&rest[..i], true)
    } else {
        (rest, false)
    };
    if !terminated {
        return None;
    }
    let mut channels = body.split('/');
    let r = u8::from_str_radix(channels.next()?.trim(), 16).ok()?;
    let g = u8::from_str_radix(channels.next()?.trim(), 16).ok()?;
    let b = u8::from_str_radix(channels.next()?.trim(), 16).ok()?;
    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_and_light_palettes_differ_on_the_agent_surface() {
        let dark = Palette::dark();
        let light = Palette::light();
        assert_eq!(dark.agent_background, "40");
        assert_eq!(dark.agent_foreground, "37");
        assert_eq!(light.agent_background, "47");
        assert_eq!(light.agent_foreground, "30");
        assert_eq!(dark.prompt, light.prompt);
    }

    #[test]
    fn theme_handles_resolve_palettes_and_names() {
        let dark = Theme::dark();
        assert_eq!(dark.name(), "dark");
        assert!(dark.is_enabled());
        let plain = Theme::plain();
        assert!(!plain.is_enabled());
        assert_eq!(plain.bold("x"), "x");
        assert_eq!(Theme::enabled(), Theme::dark());
    }

    #[test]
    fn match_preference_classifies_builtins_names_and_paths() {
        assert_eq!(match_preference("dark"), Preference::Dark);
        assert_eq!(match_preference("LIGHT"), Preference::Light);
        assert_eq!(match_preference("dark"), Preference::Dark);
        assert!(matches!(
            match_preference("/tmp/x.toml"),
            Preference::Path(_)
        ));
        assert!(matches!(match_preference("x/y"), Preference::Path(_)));
        assert!(matches!(
            match_preference("theme.toml"),
            Preference::Path(_)
        ));
        let registered = registered();
        assert!(registered.iter().any(|(n, _, b)| n == "dark" && *b));
    }

    #[test]
    fn parse_color_handles_hex_named_and_index() {
        let c = parse_color("#ff0000").unwrap();
        assert_eq!(c.foreground, "38;2;255;0;0");
        assert_eq!(c.background, "48;2;255;0;0");
        let c = parse_color("#f00").unwrap();
        assert_eq!(c.foreground, "38;2;255;0;0");
        let c = parse_color("red").unwrap();
        assert_eq!(c.foreground, "31");
        assert_eq!(c.background, "41");
        let c = parse_color("208").unwrap();
        assert_eq!(c.foreground, "38;5;208");
        assert!(parse_color("blue-purple").is_err());
        assert!(parse_color("#12345").is_err());
    }

    #[test]
    fn parse_theme_spec_validates_and_applies_overrides() {
        let spec = parse_theme_spec(
            r##"
name = "Midnight"
mode = "dark"
[colors]
prompt = "#4f9ef7"
border = "gray"
agent_background = "black"
"##,
        )
        .unwrap();
        assert_eq!(spec.name, "midnight");
        let palette = palette_for_spec(&spec);
        assert_eq!(palette.prompt, "38;2;79;158;247");
        assert_eq!(palette.border, "90");
        assert_eq!(palette.agent_background, "40");
        // Unset roles keep the dark base.
        assert_eq!(palette.indicator, "1;32");
    }

    #[test]
    fn parse_theme_spec_rejects_bad_documents() {
        assert!(parse_theme_spec("name = 5").is_err());
        assert!(parse_theme_spec("name = \"ok\" mode = \"nope\"").is_err());
        assert!(parse_theme_spec("name = \"ok\"\n[colors]\nnope = \"red\"").is_err());
        assert!(parse_theme_spec("name = \"ok\"\n[colors]\nprompt = \"magenta-blue\"").is_err());
    }

    #[test]
    fn palette_for_spec_defaults_to_dark_mode() {
        let spec = parse_theme_spec("name = \"paper\"").unwrap();
        assert_eq!(spec.mode, "dark");
        let palette = palette_for_spec(&spec);
        assert_eq!(palette.agent_background, "40");
    }

    #[test]
    fn palette_for_spec_light_mode_uses_light_base() {
        let spec = parse_theme_spec("name = \"paper\"\nmode = \"light\"").unwrap();
        let palette = palette_for_spec(&spec);
        assert_eq!(palette.agent_background, "47");
        assert_eq!(palette.agent_foreground, "30");
    }

    #[test]
    fn custom_theme_round_trip_through_the_registry() {
        let spec =
            parse_theme_spec("name = \"test-custom-rt\"\n[colors]\nprompt = \"green\"").unwrap();
        let def = ThemeDef {
            name: spec.name.clone(),
            palette: palette_for_spec(&spec),
            builtin: false,
        };
        let id = register(def);
        assert_eq!(Theme::by_id(id).name(), "test-custom-rt");
        assert_eq!(Theme::by_id(id).palette().prompt, "32");
        assert_eq!(match_preference("test-custom-rt"), Preference::Named(id));
    }

    #[test]
    fn luminance_classification() {
        assert!(is_dark_surface((0, 0, 0)));
        assert!(is_dark_surface((30, 30, 46)));
        assert!(!is_dark_surface((255, 255, 255)));
        assert!(!is_dark_surface((230, 230, 230)));
    }

    #[test]
    fn osc11_parsing() {
        let data = b"\x1b]11;rgb:1e/1e/2e\x1b\\";
        assert_eq!(parse_osc11(data), Some((0x1e, 0x1e, 0x2e)));
        let data = b"\x1b]11;rgb:ff/ff/ff\x07";
        assert_eq!(parse_osc11(data), Some((255, 255, 255)));
        assert_eq!(parse_osc11(b"\x1b]11;rgb:1e/1e"), None);
        assert_eq!(parse_osc11(b"nothing"), None);
    }
}
