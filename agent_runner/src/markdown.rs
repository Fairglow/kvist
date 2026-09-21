//! Markdown rendering for the transcript: parse GitHub-flavored Markdown and
//! emit a flat list of styled, width-limited lines the ratatui transcript
//! draws directly. The conversion is a pure function of its input, so it is
//! unit-tested without a terminal.
//!
//! Supported: headings, emphasis, inline code, fenced and indented code blocks
//! with syntax highlighting, blockquotes, ordered/unordered and task lists,
//! strikethrough, GitHub tables, links, and horizontal rules.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style as RStyle};
use ratatui::text::{Line, Span};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SColor, FontStyle, Style as SStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

/// One visual transcript row plus a flag marking whether it is code.
#[derive(Debug, Clone)]
pub struct RenderedLine {
    pub line: Line<'static>,
    pub is_code: bool,
}

/// Styling for each Markdown construct. Defaults suit a dark terminal; the
/// code-block background is derived from the syntect theme.
#[derive(Debug, Clone)]
pub struct MarkdownStyles {
    pub paragraph: RStyle,
    pub heading: [RStyle; 6],
    pub code_inline: RStyle,
    pub code_bg: RStyle,
    pub code_label: RStyle,
    pub code_gutter: RStyle,
    pub quote: RStyle,
    pub quote_gutter: RStyle,
    pub link: RStyle,
    pub link_url: RStyle,
    pub html: RStyle,
    pub rule: RStyle,
    pub list_bullet: RStyle,
    pub task_open: RStyle,
    pub task_closed: RStyle,
    pub table_header: RStyle,
    pub table_sep: RStyle,
}

impl Default for MarkdownStyles {
    fn default() -> Self {
        Self {
            paragraph: RStyle::default(),
            heading: [
                RStyle::default().fg(Color::Red),
                RStyle::default().fg(Color::Yellow),
                RStyle::default().fg(Color::Green),
                RStyle::default().fg(Color::Cyan),
                RStyle::default().fg(Color::Blue),
                RStyle::default().fg(Color::Magenta),
            ],
            code_inline: RStyle::default().bg(Color::DarkGray),
            code_bg: RStyle::default().bg(Color::DarkGray),
            code_label: RStyle::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
            code_gutter: RStyle::default().fg(Color::Gray),
            quote: RStyle::default().fg(Color::Gray),
            quote_gutter: RStyle::default().fg(Color::DarkGray),
            link: RStyle::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            link_url: RStyle::default().fg(Color::DarkGray),
            html: RStyle::default().fg(Color::Gray),
            rule: RStyle::default().fg(Color::DarkGray),
            list_bullet: RStyle::default().fg(Color::DarkGray),
            task_open: RStyle::default().fg(Color::Gray),
            task_closed: RStyle::default().fg(Color::Green),
            table_header: RStyle::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            table_sep: RStyle::default().fg(Color::DarkGray),
        }
    }
}

const GUTTER: &str = "\u{258f}";
const BULLET: &str = "\u{2022}";
const CHECKED: &str = "\u{2611}";
const UNCHECKED: &str = "\u{2610}";

fn srgb(c: SColor) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

fn syntax_set() -> &'static SyntaxSet {
    static LOAD: OnceLock<SyntaxSet> = OnceLock::new();
    LOAD.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn default_theme() -> &'static Theme {
    static LOAD: OnceLock<Theme> = OnceLock::new();
    LOAD.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        const CANDIDATES: [&str; 8] = [
            "base16_default_dark", "solarized-dark", "gruvbox-dark", "base16-ocean.dark",
            "monokai", "two-point-four-dark", "everforest", "tokyonight",
        ];
        if let Some(theme) = CANDIDATES.iter().find_map(|name| ts.themes.get(*name)) {
            return theme.clone();
        }
        ts.themes
            .values()
            .find(|t| matches!(t.settings.background, Some(c) if c.r < 0x80 && c.g < 0x80 && c.b < 0x80))
            .cloned()
            .or_else(|| ts.themes.values().next().cloned())
            .expect("syntect's default theme set is not empty")
    })
}

/// Renders Markdown to styled, width-limited lines using the default styles.
pub fn render_document(md: &str, width: usize) -> Vec<RenderedLine> {
    render_document_styles(md, width, &MarkdownStyles::default())
}

/// Renders Markdown using supplied styles, for restyling or tests.
pub fn render_document_styles(
    md: &str,
    width: usize,
    styles: &MarkdownStyles,
) -> Vec<RenderedLine> {
    let width = width.max(1);
    let renderer = Render::new(styles, width, syntax_set(), default_theme());
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut renderer = renderer;
    for event in Parser::new_ext(md, options) {
        renderer.handle_event(event);
    }
    renderer.flush_current();
    renderer.out
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Current {
    None,
    Para,
    Heading(u16),
    Item,
}

enum Layer {
    Emphasis,
    Strong,
    Strike,
    Link(RStyle),
}

enum Run {
    Text(String, RStyle),
    HardBreak,
}

struct ListState {
    ordered: bool,
    next: u64,
}

struct CodeCtx<'a> {
    info: String,
    hl: HighlightLines<'a>,
    pending: String,
}

struct TableCtx {
    aligns: Vec<Alignment>,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
    in_head: bool,
}

struct Render<'a> {
    styles: &'a MarkdownStyles,
    width: usize,
    margin: usize,
    ss: &'a SyntaxSet,
    theme: &'a Theme,
    out: Vec<RenderedLine>,
    current: Current,
    runs: Vec<Run>,
    layers: Vec<Layer>,
    quote_depth: usize,
    quote_buf: Vec<Vec<RenderedLine>>,
    lists: Vec<ListState>,
    item_task: Option<bool>,
    code: Option<CodeCtx<'a>>,
    table: Option<TableCtx>,
}

impl<'a> Render<'a> {
    fn new(styles: &'a MarkdownStyles, width: usize, ss: &'a SyntaxSet, theme: &'a Theme) -> Self {
        Self {
            styles,
            width,
            margin: 0,
            ss,
            theme,
            out: Vec::new(),
            current: Current::None,
            runs: Vec::new(),
            layers: Vec::new(),
            quote_depth: 0,
            quote_buf: Vec::new(),
            lists: Vec::new(),
            item_task: None,
            code: None,
            table: None,
        }
    }

    fn content_width(&self) -> usize {
        self.width.saturating_sub(self.margin).max(1)
    }

    fn emit(&mut self, rendered: RenderedLine) {
        if let Some(buf) = self.quote_buf.last_mut() {
            buf.push(rendered);
        } else {
            self.out.push(rendered);
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag_end) => self.end_tag(tag_end),
            Event::Rule => {
                self.flush_current();
                let line = Line::from(vec![Span::styled(
                    String::from('-').repeat(self.content_width().min(60)),
                    self.styles.rule,
                )]);
                self.emit(RenderedLine {
                    line,
                    is_code: false,
                });
            }
            Event::Text(text) => self.on_text(text.as_ref()),
            Event::Code(code) => self.on_code(code.as_ref()),
            Event::SoftBreak => self.on_soft_break(),
            Event::HardBreak => self.runs.push(Run::HardBreak),
            Event::TaskListMarker(checked) => self.item_task = Some(checked),
            Event::Html(html) | Event::InlineHtml(html) => self.on_html(&html),
            _ => {}
        }
    }

    fn on_text(&mut self, text: &str) {
        if let Some(code) = &mut self.code {
            code.pending.push_str(text);
            return;
        }
        if self.table.is_some() {
            if let Some(t) = &mut self.table {
                t.cell.push_str(text);
            }
            return;
        }
        let style = self.current_style();
        match self.runs.last_mut() {
            Some(Run::Text(s, st)) if *st == style => s.push_str(text),
            _ => self.runs.push(Run::Text(text.to_owned(), style)),
        }
    }

    fn on_code(&mut self, code: &str) {
        if self.table.is_some() {
            if let Some(t) = &mut self.table {
                t.cell.push_str(code);
            }
            return;
        }
        let style = self.styles.code_inline;
        match self.runs.last_mut() {
            Some(Run::Text(s, st)) if *st == style => s.push_str(code),
            _ => self.runs.push(Run::Text(code.to_owned(), style)),
        }
    }

    fn on_soft_break(&mut self) {
        if let Some(code) = &mut self.code {
            code.pending.push('\n');
        } else if let Some(t) = &mut self.table {
            t.cell.push(' ');
        } else {
            self.runs
                .push(Run::Text(" ".to_owned(), self.current_style()));
        }
    }

    fn on_html(&mut self, html: &str) {
        if self.current == Current::Item && html.to_lowercase().contains("checkbox") {
            self.item_task = Some(html.to_lowercase().contains("checked"));
            return;
        }
        if let Some(Run::Text(s, _)) = self.runs.last_mut() {
            s.push_str(html);
        } else {
            self.runs.push(Run::Text(html.to_owned(), self.styles.html));
        }
    }

    fn start_tag(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                self.flush_current();
                self.current = Current::Para;
            }
            Tag::Heading { level, .. } => {
                self.flush_current();
                self.current = Current::Heading(level_num(level));
            }
            Tag::BlockQuote(_) => {
                self.flush_current();
                self.quote_depth += 1;
                self.margin += 1;
                self.quote_buf.push(Vec::new());
            }
            Tag::CodeBlock(kind) => self.start_code(kind),
            Tag::List(start) => {
                self.lists.push(match start {
                    Some(first) => ListState {
                        ordered: true,
                        next: first,
                    },
                    None => ListState {
                        ordered: false,
                        next: 1,
                    },
                });
            }
            Tag::Item => {
                self.flush_current();
                self.current = Current::Item;
                self.item_task = None;
            }
            Tag::Table(aligns) => {
                self.table = Some(TableCtx {
                    aligns,
                    header: Vec::new(),
                    rows: Vec::new(),
                    row: Vec::new(),
                    cell: String::new(),
                    in_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(t) = &mut self.table {
                    t.in_head = true;
                }
            }
            Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.row.clear();
                }
            }
            Tag::TableCell => {
                if let Some(t) = &mut self.table {
                    t.cell.clear();
                }
            }
            Tag::Emphasis => self.layers.push(Layer::Emphasis),
            Tag::Strong => self.layers.push(Layer::Strong),
            Tag::Strikethrough => self.layers.push(Layer::Strike),
            Tag::Link { .. } => self.layers.push(Layer::Link(self.styles.link)),
            _ => {}
        }
    }

    fn start_code(&mut self, kind: CodeBlockKind) {
        let info = match kind {
            CodeBlockKind::Fenced(s) => s.to_string(),
            CodeBlockKind::Indented => String::new(),
        };
        let syntax = find_syntax(self.ss, &info);
        let fallback = self.ss.syntaxes().first().expect("syntax set is not empty");
        let hl = HighlightLines::new(syntax.unwrap_or(fallback), self.theme);
        self.code = Some(CodeCtx {
            info,
            hl,
            pending: String::new(),
        });
    }

    fn end_tag(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Paragraph => self.flush_current(),
            TagEnd::Heading(_) => self.flush_current(),
            TagEnd::BlockQuote(_) => self.finish_blockquote(),
            TagEnd::CodeBlock => self.finish_code(),
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::Item => self.flush_current(),
            TagEnd::Table => self.finish_table(),
            TagEnd::TableRow => {
                if let Some(t) = &mut self.table {
                    if t.in_head {
                        t.header = std::mem::take(&mut t.row);
                    } else {
                        t.rows.push(std::mem::take(&mut t.row));
                    }
                }
            }
            TagEnd::TableCell => {
                if let Some(t) = &mut self.table {
                    t.row.push(std::mem::take(&mut t.cell));
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    if t.in_head && !t.row.is_empty() {
                        t.header = std::mem::take(&mut t.row);
                    }
                    t.in_head = false;
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.layers.pop();
            }
            TagEnd::Link => {
                self.layers.pop();
            }
            _ => {}
        }
    }

    fn current_style(&self) -> RStyle {
        let mut bold = false;
        let mut italic = false;
        let mut strike = false;
        for layer in &self.layers {
            match layer {
                Layer::Emphasis => italic = true,
                Layer::Strong => bold = true,
                Layer::Strike => strike = true,
                Layer::Link(_) => {}
            }
        }
        if let Some(Layer::Link(style)) = self.layers.last() {
            let mut style = *style;
            if strike {
                style = style.add_modifier(Modifier::CROSSED_OUT);
            }
            return style;
        }
        let mut style = self.styles.paragraph;
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if strike {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        style
    }

    fn flush_current(&mut self) {
        match self.current {
            Current::Para => {
                for line in wrap_runs(&self.runs, self.content_width()) {
                    self.emit(RenderedLine {
                        line,
                        is_code: false,
                    });
                }
            }
            Current::Heading(_) => self.flush_heading(),
            Current::Item => self.flush_item(),
            Current::None => {}
        }
        self.current = Current::None;
        self.runs.clear();
    }

    fn flush_heading(&mut self) {
        let level = match self.current {
            Current::Heading(level) => level,
            _ => return,
        };
        let style = self.styles.heading[(level as usize).saturating_sub(1).min(5)]
            .add_modifier(Modifier::BOLD);
        let forced: Vec<Run> = self
            .runs
            .iter()
            .map(|run| match run {
                Run::HardBreak => Run::HardBreak,
                Run::Text(text, _) => Run::Text(text.clone(), style),
            })
            .collect();
        for line in wrap_runs(&forced, self.content_width()) {
            self.emit(RenderedLine {
                line,
                is_code: false,
            });
        }
    }

    fn finish_blockquote(&mut self) {
        let lines = self.quote_buf.pop().unwrap_or_default();
        self.quote_depth = self.quote_depth.saturating_sub(1);
        self.margin = self.margin.saturating_sub(1);
        for mut rendered in lines {
            let mut line = Line::from(vec![]);
            for _ in 0..self.quote_depth {
                line.spans
                    .push(Span::styled(GUTTER, self.styles.quote_gutter));
            }
            line.spans.extend(rendered.line.spans);
            rendered.line = line;
            self.emit(rendered);
        }
    }

    fn finish_code(&mut self) {
        let mut code = self.code.take().expect("code block open on close");
        let bg = self
            .theme
            .settings
            .background
            .map(srgb)
            .unwrap_or(Color::Gray);
        let lang = parse_language(&code.info);
        if let Some(lang) = &lang {
            let label = Line::from(vec![
                Span::styled(" ", self.styles.code_gutter),
                Span::styled(lang.clone(), self.styles.code_label),
            ]);
            self.emit(RenderedLine {
                line: label,
                is_code: true,
            });
        }
        let area = self.content_width().saturating_sub(2).max(1);
        let mut sources: Vec<&str> = code.pending.split('\n').collect();
        if sources.last().is_some_and(|s| s.is_empty()) {
            sources.pop();
        }
        for source in sources {
            let ranges = code
                .hl
                .highlight_line(source, self.ss)
                .unwrap_or_else(|_| vec![(SStyle::default(), source)]);
            let spans = clip_ranges(ranges, area, bg);
            let mut line = Line::from(vec![
                Span::styled(GUTTER, self.styles.code_gutter),
                Span::styled(" ", self.styles.code_gutter),
            ]);
            line.spans.extend(spans);
            self.emit(RenderedLine {
                line,
                is_code: true,
            });
        }
    }

    fn flush_item(&mut self) {
        let list = self.lists.last();
        let (prefix, marker_style) = match list {
            Some(s) if s.ordered => (format!("{}.", s.next), self.styles.list_bullet),
            Some(_) => (BULLET.to_owned(), self.styles.list_bullet),
            None => (String::new(), self.styles.list_bullet),
        };
        let marker = match self.item_task {
            Some(true) => Some((CHECKED.to_owned(), self.styles.task_closed)),
            Some(false) => Some((UNCHECKED.to_owned(), self.styles.task_open)),
            None => None,
        };
        let mut lines = wrap_runs(&self.runs, self.content_width());
        if let Some(first) = lines.first_mut() {
            let mut spans = Vec::new();
            if let Some((marker_text, marker_style)) = &marker {
                spans.push(Span::styled(marker_text.clone(), *marker_style));
            } else {
                spans.push(Span::styled(prefix, marker_style));
            }
            spans.append(&mut first.spans);
            first.spans = spans;
        }
        for line in lines {
            self.emit(RenderedLine {
                line,
                is_code: false,
            });
        }
        if let Some(s) = self.lists.last_mut()
            && s.ordered
        {
            s.next += 1;
        }
    }

    fn finish_table(&mut self) {
        let table = self.table.take().expect("table open on close");
        let aligns = table.aligns;
        let header = table.header;
        let rows = table.rows;
        let ncols = aligns.len().max(
            header
                .len()
                .max(rows.iter().map(Vec::len).max().unwrap_or(0)),
        );
        if ncols == 0 {
            return;
        }
        let mut widths: Vec<usize> = (0..ncols)
            .map(|c| {
                header
                    .get(c)
                    .map(|cells| cells.chars().count())
                    .unwrap_or(0)
                    .max(
                        rows.iter()
                            .filter_map(|r| r.get(c))
                            .map(|c| c.chars().count())
                            .max()
                            .unwrap_or(0),
                    )
                    .max(1)
            })
            .collect();
        let sum_widths: usize = widths.iter().sum();
        let needed = sum_widths + ncols + 1;
        if needed > self.content_width() {
            let overflow = needed - self.content_width();
            for w in widths.iter_mut() {
                *w = w.saturating_sub(*w * overflow / sum_widths.max(1)).max(1);
            }
        }
        if let Some(header_line) =
            render_table_row(&header, &widths, &aligns, self.styles.table_header)
        {
            self.emit(RenderedLine {
                line: header_line,
                is_code: false,
            });
        }
        if let Some(sep_spans) = render_table_separator(&widths, &aligns, self.styles.table_sep) {
            self.emit(RenderedLine {
                line: Line::from(sep_spans),
                is_code: false,
            });
        }
        for row in rows {
            if let Some(line) = render_table_row(&row, &widths, &aligns, self.styles.paragraph) {
                self.emit(RenderedLine {
                    line,
                    is_code: false,
                });
            }
        }
    }
}

fn level_num(level: HeadingLevel) -> u16 {
    (level as u32) as u16
}

fn parse_language(info: &str) -> Option<String> {
    let lang = info.split_whitespace().next()?;
    if lang.is_empty() || lang.starts_with('#') || lang.starts_with('.') {
        return None;
    }
    Some(lang.to_ascii_lowercase())
}

fn find_syntax<'a>(ss: &'a SyntaxSet, info: &str) -> Option<&'a SyntaxReference> {
    let lang = parse_language(info)?;
    const ALIASES: [(&str, &str); 24] = [
        ("rs", "Rust"),
        ("py", "Python"),
        ("py3", "Python"),
        ("js", "JavaScript"),
        ("mjs", "JavaScript"),
        ("cjs", "JavaScript"),
        ("ts", "TypeScript"),
        ("tsx", "TypeScript"),
        ("jsx", "JavaScript"),
        ("sh", "Bash"),
        ("bash", "Bash"),
        ("zsh", "Bash"),
        ("yml", "YAML"),
        ("yaml", "YAML"),
        ("toml", "TOML"),
        ("json", "JSON"),
        ("css", "CSS"),
        ("html", "HTML"),
        ("sql", "SQL"),
        ("go", "Go"),
        ("rb", "Ruby"),
        ("kt", "Kotlin"),
        ("c", "C"),
        ("cpp", "C++"),
    ];
    if let Some((_, name)) = ALIASES.iter().find(|(alias, _)| *alias == lang)
        && let Some(s) = ss.find_syntax_by_name(name)
    {
        return Some(s);
    }
    ss.find_syntax_by_extension(&lang)
        .or_else(|| ss.find_syntax_by_name(&lang))
        .or_else(|| ss.find_syntax_by_name(&lang.to_ascii_uppercase()))
}

struct Word {
    text: String,
    style: RStyle,
    space_before: bool,
}

/// Wraps styled runs to `width`, merging same-style runs and breaking only at
/// whitespace; an over-long token is chunked rather than dropped.
fn wrap_runs(runs: &[Run], width: usize) -> Vec<Line<'static>> {
    let width = width.max(10);
    let chunks: Vec<&[Run]> = {
        let mut chunks = Vec::new();
        let mut start = 0;
        for (i, run) in runs.iter().enumerate() {
            if matches!(run, Run::HardBreak) {
                chunks.push(&runs[start..i]);
                start = i + 1;
            }
        }
        if start < runs.len() {
            chunks.push(&runs[start..]);
        }
        chunks
    };
    let mut lines = Vec::new();
    for chunk in chunks {
        let mut merged: Vec<(String, RStyle)> = Vec::new();
        for run in chunk {
            if let Run::Text(text, style) = run {
                if let Some((last, last_style)) = merged.last_mut()
                    && last_style == style
                {
                    last.push_str(text);
                    continue;
                }
                merged.push((text.clone(), *style));
            }
        }
        let mut words: Vec<Word> = Vec::new();
        for (text, style) in merged {
            for token in text.split_whitespace() {
                words.push(Word {
                    text: token.to_owned(),
                    style,
                    space_before: !words.is_empty(),
                });
            }
        }
        greedy_wrap(words, width, &mut lines);
    }
    lines
}

fn greedy_wrap(words: Vec<Word>, width: usize, lines: &mut Vec<Line<'static>>) {
    lines.push(Line::from(vec![]));
    for word in words {
        let word_len = word.text.chars().count();
        if word_len > width {
            if !lines.last().expect("a line exists").spans.is_empty() {
                lines.push(Line::from(vec![]));
            }
            for chunk in word
                .text
                .chars()
                .collect::<Vec<char>>()
                .chunks(width)
                .map(|c| c.iter().collect::<String>())
            {
                lines.push(Line::from(vec![Span::styled(chunk, word.style)]));
            }
            continue;
        }
        let last = lines.last_mut().expect("a line exists");
        let extra = if !last.spans.is_empty() && word.space_before {
            1
        } else {
            0
        };
        if !last.spans.is_empty() && last.width() + extra + word_len > width {
            lines.push(Line::from(vec![]));
        }
        let last = lines.last_mut().expect("a line exists");
        if !last.spans.is_empty() && word.space_before {
            last.spans.push(Span::styled(" ", RStyle::default()));
        }
        last.spans.push(Span::styled(word.text.clone(), word.style));
    }
}

fn clip_ranges(ranges: Vec<(SStyle, &str)>, max_chars: usize, bg: Color) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (style, text) in ranges {
        let chars: Vec<char> = text.chars().collect();
        if used + chars.len() <= max_chars {
            spans.push(Span::styled(text.to_owned(), syntect_style(style, bg)));
            used += chars.len();
        } else {
            let remaining = max_chars.saturating_sub(used);
            if remaining > 0 {
                let s: String = chars[..remaining].iter().collect();
                spans.push(Span::styled(s, syntect_style(style, bg)));
            }
            break;
        }
    }
    spans
}

fn syntect_style(style: SStyle, bg: Color) -> RStyle {
    let mut out = RStyle::default().fg(srgb(style.foreground)).bg(bg);
    let fs = style.font_style;
    if fs.contains(FontStyle::BOLD) {
        out = out.add_modifier(Modifier::BOLD);
    }
    if fs.contains(FontStyle::ITALIC) {
        out = out.add_modifier(Modifier::ITALIC);
    }
    if fs.contains(FontStyle::UNDERLINE) {
        out = out.add_modifier(Modifier::UNDERLINED);
    }
    out
}

fn render_table_row(
    cells: &[String],
    widths: &[usize],
    aligns: &[Alignment],
    style: RStyle,
) -> Option<Line<'static>> {
    if cells.is_empty() {
        return None;
    }
    let mut spans = vec![Span::styled(" ", style)];
    for (i, width) in widths.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" ", style));
        }
        let cell = cells.get(i).map(String::as_str).unwrap_or("");
        let aligned = align_cell(
            cell,
            *width,
            aligns.get(i).copied().unwrap_or(Alignment::Left),
        );
        spans.push(Span::styled(aligned, style));
    }
    spans.push(Span::styled(" ", style));
    Some(Line::from(spans))
}

fn render_table_separator(
    widths: &[usize],
    aligns: &[Alignment],
    style: RStyle,
) -> Option<Vec<Span<'static>>> {
    if widths.is_empty() {
        return None;
    }
    let mut spans = vec![Span::styled(" ", style)];
    for (i, width) in widths.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" ", style));
        }
        let align = aligns.get(i).copied().unwrap_or(Alignment::Left);
        spans.push(Span::styled(separator_cell(*width, align), style));
    }
    spans.push(Span::styled(" ", style));
    Some(spans)
}

/// A table separator cell of exactly `width` columns, marked with a colon to
/// show column alignment: none for left (`---`), one at each end for center
/// (`:--:`), and one at the right for right (`---:`). Keeping the width equal to
/// the column width keeps the separator row straight with the cells.
fn separator_cell(width: usize, align: Alignment) -> String {
    let width = width.max(1);
    let mut cell: Vec<char> = std::iter::repeat_n('-', width).collect();
    match align {
        Alignment::None | Alignment::Left => {}
        Alignment::Center => {
            cell[0] = ':';
            cell[width - 1] = ':';
        }
        Alignment::Right => cell[width - 1] = ':',
    }
    cell.into_iter().collect()
}

fn align_cell(cell: &str, width: usize, align: Alignment) -> String {
    let shown: Vec<char> = cell.chars().take(width).collect();
    let len = shown.len();
    if len >= width {
        return shown.iter().collect();
    }
    let padding = width - len;
    let (left, right) = match align {
        Alignment::Right => (0, padding),
        Alignment::Center => (padding / 2, padding - padding / 2),
        _ => (padding, 0),
    };
    let mut out = String::new();
    out.push_str(&" ".repeat(left));
    out.extend(shown);
    out.push_str(&" ".repeat(right));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(lines: &[RenderedLine]) -> String {
        lines.iter().map(|rl| rl.line.to_string()).collect()
    }

    #[test]
    fn headings_are_bold() {
        let out = render_document("# Title\n\nsome text\n", 40);
        assert_eq!(out[0].line.to_string(), "Title");
        assert!(
            out[0].line.spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(out[1].line.to_string(), "some text");
    }

    #[test]
    fn bold_applies_modifier() {
        let out = render_document("a **b** c", 40);
        assert_eq!(out[0].line.to_string(), "a b c");
        let strong = out[0]
            .line
            .spans
            .iter()
            .find(|s| s.content == "b")
            .expect("b span");
        assert!(strong.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn inline_code_is_styled() {
        let out = render_document("use `cargo build` now", 40);
        assert_eq!(out[0].line.to_string(), "use cargo build now");
        assert!(
            out[0]
                .line
                .spans
                .iter()
                .any(|s| s.style.bg == Some(Color::DarkGray))
        );
    }

    #[test]
    fn code_fence_is_highlighted_and_marked() {
        let out = render_document("```rust\nfn main() {}\n```\n", 40);
        assert!(out.iter().all(|rl| rl.is_code));
        assert_eq!(out[0].line.to_string(), " rust");
        assert!(out[1].line.to_string().contains("fn main() {}"));
    }

    #[test]
    fn indented_code_has_no_language_label() {
        let out = render_document("a code block\n\n    let x = 1;\n", 40);
        assert_eq!(out[0].line.to_string(), "a code block");
        let code_lines: Vec<&RenderedLine> = out.iter().filter(|rl| rl.is_code).collect();
        assert_eq!(code_lines.len(), 1);
        assert!(code_lines[0].line.to_string().contains("let x = 1;"));
        assert!(!out.iter().any(
            |rl| rl.line.to_string().contains("rust") || rl.line.to_string().contains("python")
        ));
    }

    #[test]
    fn table_renders_header_separator_and_row() {
        let out = render_document("| a | b |\n| --- | --- |\n| 1 | 2 |\n", 40);
        assert_eq!(out[0].line.to_string(), " a b ");
        assert_eq!(out[1].line.to_string(), " - - ");
        assert_eq!(out[2].line.to_string(), " 1 2 ");
    }

    #[test]
    fn table_separator_marks_alignment() {
        let out = render_document(
            "| aaa | bbb | ccc |\n| --- | :--: | ---: |\n| 1 | 2 | 3 |\n",
            40,
        );
        let sep = out[1].line.to_string();
        assert!(sep.contains(":-:"), "center marker: {sep:?}");
        assert!(sep.contains("-:"), "right marker: {sep:?}");
    }

    #[test]
    fn task_list_uses_checkboxes() {
        let text = joined(&render_document("- [x] done\n- [ ] todo\n", 40));
        assert!(text.contains(CHECKED), "checked box: {text:?}");
        assert!(text.contains(UNCHECKED), "unchecked box: {text:?}");
    }

    #[test]
    fn ordered_list_numbers_progress() {
        let out = render_document("1. one\n2. two\n", 40);
        assert!(out[0].line.to_string().starts_with("1."));
        assert!(out[1].line.to_string().starts_with("2."));
    }

    #[test]
    fn long_word_is_chunked_not_overflowing() {
        let word = String::from("a").repeat(50);
        for line in render_document(&word, 20) {
            assert!(line.line.width() <= 20, "line too wide: {}", line.line);
        }
    }
}
