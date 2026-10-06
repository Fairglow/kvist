//! Loading `Theme` values from external TOML theme files, and discovering
//! the set of themes available to the UI (the built-ins plus any `*.toml`
//! file in a user themes directory).
//!
//! A theme is plain data (see `themes/dark.toml` and `themes/light.toml` in
//! the repository for the canonical examples and the full format
//! description). Nothing about a theme's colors lives in Rust source: editing
//! or adding a theme file takes effect on the next run, with no rebuild.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ratatui::style::{Modifier, Style};
use serde::Deserialize;

use super::Theme;
use super::color::parse_color;
use crate::markdown::MarkdownStyles;

/// Maximum encoded size of a theme file, mirroring `config::MAX_CONFIG_BYTES`:
/// theme files are untrusted external input like the configuration file.
pub const MAX_THEME_BYTES: u64 = 64 * 1024;

/// The file-name extension theme files use.
pub const THEME_FILE_EXTENSION: &str = "toml";

/// One `fg`/`bg`/modifier table in a theme file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StyleSpec {
    fg: Option<String>,
    bg: Option<String>,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    dim: bool,
    #[serde(default)]
    italic: bool,
}

impl StyleSpec {
    /// Builds a style, requiring `fg`; `item` names the field for error text.
    fn style(&self, item: &str) -> Result<Style, String> {
        let fg = self
            .fg
            .as_deref()
            .ok_or_else(|| format!("`{item}.fg` is required"))?;
        let mut style = Style::default().fg(parse_color(item, "fg", fg)?);
        if let Some(bg) = &self.bg {
            style = style.bg(parse_color(item, "bg", bg)?);
        }
        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.dim {
            style = style.add_modifier(Modifier::DIM);
        }
        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        Ok(style)
    }

    /// Builds a style that must declare its own background: used for every
    /// surface the renderer paints as a whole widget (the reasoning strip,
    /// the scrollbar, and the menu rows), so nothing there can fall back to
    /// the terminal's own default background.
    fn style_with_required_bg(&self, item: &str) -> Result<Style, String> {
        if self.bg.is_none() {
            return Err(format!(
                "`{item}.bg` is required: this surface paints its own background, so it must \
                 be explicit and cannot fall back to the terminal's default"
            ));
        }
        self.style(item)
    }

    /// Builds a background-only style (the code-block patch): `fg` is not
    /// required because the patch is applied under already-colored text.
    fn bg_only(&self, item: &str) -> Result<Style, String> {
        let bg = self
            .bg
            .as_deref()
            .ok_or_else(|| format!("`{item}.bg` is required"))?;
        if self.fg.is_some() {
            return Err(format!("`{item}.fg` is not used; remove it"));
        }
        Ok(Style::default().bg(parse_color(item, "bg", bg)?))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ColorsSpec {
    dim: String,
    ok: String,
    warn: String,
    err: String,
    info: String,
    accent: String,
    model: String,
    model_active: String,
    spinner: String,
    stats: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelSpec {
    bg: String,
    border: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MenuSpec {
    selected: StyleSpec,
    plain: StyleSpec,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MarkdownSpec {
    paragraph: StyleSpec,
    heading: [StyleSpec; 6],
    code_inline: StyleSpec,
    code_bg: StyleSpec,
    code_label: StyleSpec,
    code_gutter: StyleSpec,
    quote: StyleSpec,
    quote_gutter: StyleSpec,
    link: StyleSpec,
    link_url: StyleSpec,
    html: StyleSpec,
    rule: StyleSpec,
    list_bullet: StyleSpec,
    task_open: StyleSpec,
    task_closed: StyleSpec,
    table_header: StyleSpec,
    table_sep: StyleSpec,
}

/// The full schema of a theme file: see `themes/dark.toml` for the canonical,
/// documented example.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeSpec {
    title: StyleSpec,
    colors: ColorsSpec,
    panel: PanelSpec,
    reasoning: StyleSpec,
    reasoning_placeholder: StyleSpec,
    prompt: StyleSpec,
    scrollbar: StyleSpec,
    menu: MenuSpec,
    markdown: MarkdownSpec,
}

/// Fails when `fg` equals `bg`: text styled this way would be invisible, so a
/// theme that declares it is rejected rather than silently shipped broken.
fn ensure_visible(item: &str, style: Style, bg: ratatui::style::Color) -> Result<(), String> {
    let fg = style.fg.unwrap_or(bg);
    let effective_bg = style.bg.unwrap_or(bg);
    if fg == effective_bg {
        return Err(format!(
            "`{item}` foreground and background are the same color ({fg:?}); its text would be \
             invisible"
        ));
    }
    Ok(())
}

impl ThemeSpec {
    fn into_theme(self, name: String) -> Result<Theme, String> {
        let panel_bg = parse_color("panel", "bg", &self.panel.bg)?;
        let panel_border = parse_color("panel", "border", &self.panel.border)?;

        let title = self.title.style("title")?;
        ensure_visible("title", title, panel_bg)?;

        let color_item = |item: &str, spec: &str| -> Result<ratatui::style::Color, String> {
            let color = parse_color(item, "fg", spec)?;
            if color == panel_bg {
                return Err(format!(
                    "`{item}` ({spec}) is the same color as `panel.bg`; its text would be \
                     invisible"
                ));
            }
            Ok(color)
        };
        let colors = &self.colors;
        let dim = color_item("colors.dim", &colors.dim)?;
        let ok = color_item("colors.ok", &colors.ok)?;
        let warn = color_item("colors.warn", &colors.warn)?;
        let err = color_item("colors.err", &colors.err)?;
        let info = color_item("colors.info", &colors.info)?;
        let accent = color_item("colors.accent", &colors.accent)?;
        let model = color_item("colors.model", &colors.model)?;
        let model_active = color_item("colors.model_active", &colors.model_active)?;
        let spinner = color_item("colors.spinner", &colors.spinner)?;
        let stats = color_item("colors.stats", &colors.stats)?;

        let reasoning_style = self.reasoning.style_with_required_bg("reasoning")?;
        ensure_visible("reasoning", reasoning_style, panel_bg)?;
        let reasoning_bg = reasoning_style.bg.expect("required above");

        let reasoning_placeholder = self.reasoning_placeholder.style("reasoning_placeholder")?;
        ensure_visible("reasoning_placeholder", reasoning_placeholder, panel_bg)?;

        let prompt = self.prompt.style("prompt")?;
        ensure_visible("prompt", prompt, panel_bg)?;

        let scrollbar = self.scrollbar.style_with_required_bg("scrollbar")?;
        ensure_visible("scrollbar", scrollbar, panel_bg)?;

        let menu_selected = self.menu.selected.style_with_required_bg("menu.selected")?;
        ensure_visible("menu.selected", menu_selected, panel_bg)?;
        let menu_plain = self.menu.plain.style_with_required_bg("menu.plain")?;
        ensure_visible("menu.plain", menu_plain, panel_bg)?;

        let md = &self.markdown;
        let markdown_style = |item: &str, spec: &StyleSpec| -> Result<Style, String> {
            let style = spec.style(item)?;
            ensure_visible(item, style, panel_bg)?;
            Ok(style)
        };
        let heading = [
            markdown_style("markdown.heading[0]", &md.heading[0])?,
            markdown_style("markdown.heading[1]", &md.heading[1])?,
            markdown_style("markdown.heading[2]", &md.heading[2])?,
            markdown_style("markdown.heading[3]", &md.heading[3])?,
            markdown_style("markdown.heading[4]", &md.heading[4])?,
            markdown_style("markdown.heading[5]", &md.heading[5])?,
        ];
        let code_inline = md.code_inline.style("markdown.code_inline")?;
        ensure_visible("markdown.code_inline", code_inline, panel_bg)?;
        let code_bg = md.code_bg.bg_only("markdown.code_bg")?;
        let markdown = MarkdownStyles {
            paragraph: markdown_style("markdown.paragraph", &md.paragraph)?,
            heading,
            code_inline,
            code_bg,
            code_label: markdown_style("markdown.code_label", &md.code_label)?,
            code_gutter: markdown_style("markdown.code_gutter", &md.code_gutter)?,
            quote: markdown_style("markdown.quote", &md.quote)?,
            quote_gutter: markdown_style("markdown.quote_gutter", &md.quote_gutter)?,
            link: markdown_style("markdown.link", &md.link)?,
            link_url: markdown_style("markdown.link_url", &md.link_url)?,
            html: markdown_style("markdown.html", &md.html)?,
            rule: markdown_style("markdown.rule", &md.rule)?,
            list_bullet: markdown_style("markdown.list_bullet", &md.list_bullet)?,
            task_open: markdown_style("markdown.task_open", &md.task_open)?,
            task_closed: markdown_style("markdown.task_closed", &md.task_closed)?,
            table_header: markdown_style("markdown.table_header", &md.table_header)?,
            table_sep: markdown_style("markdown.table_sep", &md.table_sep)?,
        };

        Ok(Theme {
            name,
            title,
            dim,
            ok,
            warn,
            err,
            info,
            accent,
            model,
            model_active,
            spinner,
            stats,
            panel_bg,
            panel_border,
            reasoning_bg,
            reasoning: reasoning_style,
            reasoning_placeholder,
            prompt,
            scrollbar,
            menu_selected,
            menu_plain,
            markdown,
        })
    }
}

/// Parses one theme file's text (its `name` is supplied separately: by
/// convention the file's stem, e.g. `dark.toml` names the theme `dark`).
pub fn parse_str(text: &str, name: &str) -> Result<Theme, String> {
    let spec: ThemeSpec =
        toml::from_str(text).map_err(|error| format!("theme `{name}`: {error}"))?;
    spec.into_theme(name.to_owned())
        .map_err(|reason| format!("theme `{name}`: {reason}"))
}

/// Loads one theme file from disk, bounding its size like the configuration
/// file: a theme is untrusted external input.
pub fn load_file(path: &Path) -> Result<Theme, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("could not read theme file `{}`: {error}", path.display()))?;
    if metadata.len() > MAX_THEME_BYTES {
        return Err(format!(
            "theme file `{}` is {} bytes, over the {MAX_THEME_BYTES}-byte limit",
            path.display(),
            metadata.len()
        ));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read theme file `{}`: {error}", path.display()))?;
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| format!("theme file `{}` has no usable file name", path.display()))?;
    parse_str(&text, name)
}

/// Lists the themes available: the two built-ins (`dark`, `light`) plus any
/// `*.toml` file in `dir`, alphabetically, with duplicates (a user file that
/// overrides a built-in name) counted once.
pub fn discover(dir: Option<&Path>) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    names.insert("dark".to_owned());
    names.insert("light".to_owned());
    if let Some(dir) = dir
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some(THEME_FILE_EXTENSION)
                && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
            {
                names.insert(stem.to_owned());
            }
        }
    }
    names.into_iter().collect()
}

/// Resolves the themes directory for a configuration file: the `themes`
/// subdirectory next to it, so `~/.config/agent-runner/config.toml` pairs
/// with `~/.config/agent-runner/themes/`. Themes there override a built-in of
/// the same name and need no rebuild to take effect.
pub fn themes_dir_for_config(config_path: &Path) -> Option<PathBuf> {
    config_path.parent().map(|dir| dir.join("themes"))
}

/// Loads a theme by name: a file named `<name>.toml` in `dir` when present,
/// else the embedded built-in `dark`/`light`.
pub fn load(name: &str, dir: Option<&Path>) -> Result<Theme, String> {
    if let Some(dir) = dir {
        let candidate = dir.join(format!("{name}.{THEME_FILE_EXTENSION}"));
        if candidate.is_file() {
            return load_file(&candidate);
        }
    }
    match name {
        "dark" => Ok(Theme::dark()),
        "light" => Ok(Theme::light()),
        _ => Err(format!(
            "theme `{name}` is not recognized; expected one of: {}",
            discover(dir).join(", ")
        )),
    }
}
