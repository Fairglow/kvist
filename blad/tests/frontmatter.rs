use std::io::{self, Write};
use std::process::{Command, Stdio};

use blad::{
    Color, ColorDepth, MarkdownRenderer, Palette, RenderCancellation, RenderOptions, Theme,
};
use vte::{Params, Parser, Perform};

fn render(source: &str, width: Option<usize>, options: RenderOptions) -> String {
    let mut renderer = MarkdownRenderer::with_options(options);
    renderer.set_width(width);
    let mut output = Vec::new();
    renderer.render_markdown(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

#[derive(Default)]
struct Tokens {
    text: String,
    colors: Vec<Option<[u16; 3]>>,
    foreground: Option<[u16; 3]>,
}

impl Perform for Tokens {
    fn print(&mut self, character: char) {
        self.text.push(character);
        self.colors.push(self.foreground);
    }

    fn execute(&mut self, byte: u8) {
        if byte == b'\n' {
            self.print('\n');
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _: &[u8], _: bool, action: char) {
        if action != 'm' {
            return;
        }
        let values: Vec<_> = params.iter().map(|value| value[0]).collect();
        let mut values = values.into_iter();
        while let Some(value) = values.next() {
            match value {
                0 | 39 => self.foreground = None,
                38 | 48 => match values.next() {
                    Some(2) => {
                        let color = [
                            values.next().unwrap(),
                            values.next().unwrap(),
                            values.next().unwrap(),
                        ];
                        if value == 38 {
                            self.foreground = Some(color);
                        }
                    }
                    Some(5) => {
                        values.next();
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

fn tokens(output: &str) -> Tokens {
    let mut tokens = Tokens::default();
    Parser::new().advance(&mut tokens, output.as_bytes());
    tokens
}

#[test]
fn frontmatter_preserves_yaml_layout_and_literal_markdown() {
    let header = "---\n# authored comment\nname: general-document\nmetadata:\n  author: \"Someone\"\n  version: '1.0'\n  active: true\n  count: 42\nitems:\n  - one\n  - two\n\nliteral: |+\n  **bold** &amp; <tag> `code`\n  H~2~O x^2^ $x^2$ [link](target.md)\nfolded: >-\n  first line\n  second line\n[ref]: target.md\n[^1]: not a Markdown definition\n---\n";
    let source = format!("{header}# Body\n\n[ref] and [^1].\n");
    assert_eq!(
        render(&source, None, RenderOptions::plain()),
        format!("{header}\n# Body\n\n[ref] and [^1].\n")
    );
    for width in [None, Some(0), Some(1), Some(25), Some(100)] {
        let actual = render(&source, width, RenderOptions::plain());
        let expected = render(
            &format!("```yaml\n{header}```\n\n# Body\n\n[ref] and [^1].\n"),
            width,
            RenderOptions::plain(),
        );
        assert_eq!(actual, expected, "width {width:?}");
        for line in header.lines().filter(|line| !line.is_empty()) {
            assert!(
                actual.lines().any(|actual| actual.trim_end() == line),
                "{actual:?}"
            );
        }
    }
}

#[test]
fn frontmatter_accepts_empty_blank_bom_crlf_and_alternate_closers() {
    for (header, displayed) in [
        ("---\n---\n", "---\n---\n"),
        ("---\n\nkey: value\n---\n", "---\n\nkey: value\n---\n"),
        (
            "--- \t\nkey: value\n...\t \n",
            "--- \t\nkey: value\n...\t \n",
        ),
        ("\u{feff}---\nkey: value\n---\n", "---\nkey: value\n---\n"),
        ("---\r\nkey: value\r\n...\r\n", "---\nkey: value\n...\n"),
        ("---\n---", "---\n---\n"),
        ("---\nkey: value\n...", "---\nkey: value\n...\n"),
    ] {
        let actual = render(header, None, RenderOptions::plain());
        assert_eq!(actual, format!("{displayed}\n"), "{header:?}");
    }
}

#[test]
fn frontmatter_has_effective_yaml_token_colors_without_geometry_changes() {
    let source = "---\nname: \"skill-name\"\n# comment\ncount: 42\n---\n\n# Body\n";
    let expected = render(source, Some(40), RenderOptions::plain());
    for theme in [Theme::Dark, Theme::Light, Theme::Terminal] {
        for depth in [
            ColorDepth::None,
            ColorDepth::Ansi16,
            ColorDepth::Ansi256,
            ColorDepth::TrueColor,
        ] {
            for plain in [false, true] {
                let options = RenderOptions {
                    theme,
                    color_depth: depth,
                    plain,
                    ..RenderOptions::default()
                };
                let output = render(source, Some(40), options.clone());
                assert_eq!(
                    output,
                    render(
                        "```yaml\n---\nname: \"skill-name\"\n# comment\ncount: 42\n---\n```\n\n# Body\n",
                        Some(40),
                        options,
                    ),
                    "{theme:?}/{depth:?}/{plain}"
                );
                let observed = tokens(&output);
                assert_eq!(observed.text, expected);
                if plain {
                    assert!(!output.contains('\x1b'));
                } else if depth == ColorDepth::TrueColor {
                    let name = observed.colors[observed.text.find("name:").unwrap()];
                    let string = observed.colors[observed.text.find("skill-name").unwrap()];
                    let comment = observed.colors[observed.text.find("# comment").unwrap()];
                    assert!(name.is_some() && string.is_some() && comment.is_some());
                    assert_ne!(name, string, "{theme:?}: {output:?}");
                    assert_ne!(string, comment, "{theme:?}: {output:?}");
                }
                if theme == Theme::Terminal {
                    assert!(!output.contains("\x1b[48;"), "{output:?}");
                }
            }
        }
    }
    let custom = render(
        source,
        Some(40),
        RenderOptions {
            palette: Some(Palette {
                code_background: Some(Color::Rgb(1, 2, 3)),
                ..Palette::default()
            }),
            ..RenderOptions::default()
        },
    );
    assert!(custom.contains("\x1b[48;2;1;2;3m"));
    assert_eq!(tokens(&custom).text, expected);
}

#[test]
fn frontmatter_is_prefix_only_and_unclosed_candidates_stay_markdown() {
    for source in [
        "\n---\n**bold**\n---",
        " ---\n**bold**\n---",
        "before\n\n---\n**bold**\n---",
        "> ---\n> **bold**\n> ---",
        "+++\n**bold**\n+++",
        "---\n**bold**",
        "---\n**bold**\n---not-a-closer",
        "---\n**bold**\n ---",
        "---\n**bold**\n----",
    ] {
        let output = render(source, None, RenderOptions::plain());
        assert!(
            output.contains("bold") && !output.contains("**bold**"),
            "{source:?}: {output:?}"
        );
    }
    let source = "---\nkey: value\n---\n\n---\n**bold**\n---";
    let output = render(source, None, RenderOptions::plain());
    assert!(output.starts_with("---\nkey: value\n---\n"));
    assert!(!output.contains("**bold**"), "{output:?}");
    let example = render(
        "```markdown\n---\n**bold**\n---\n```",
        None,
        RenderOptions::plain(),
    );
    assert_eq!(example, "---\n**bold**\n---\n\n");
}

#[test]
fn frontmatter_never_creates_links_or_definitions_and_body_offsets_survive() {
    let header = "\u{feff}---\r\nurl: https://metadata.test\r\nlink: '[hidden](hidden.md)'\r\n# Duplicate\r\n[reference]: hidden.md\r\n---\r\n";
    let source = format!(
        "{header}# Duplicate\n\n[body](#duplicate) https://body.test [reference]\n\n[body-ref][real]\n\n[real]: actual.md\n"
    );
    let links = blad::links::extract_links(&source);
    assert_eq!(links.len(), 3, "{links:?}");
    for (link, label, destination, offset) in [
        (
            &links[0],
            "body",
            "#duplicate",
            source.find("[body]").unwrap(),
        ),
        (
            &links[1],
            "https://body.test",
            "https://body.test",
            source.find("https://body.test").unwrap(),
        ),
        (
            &links[2],
            "body-ref",
            "actual.md",
            source.find("[body-ref]").unwrap(),
        ),
    ] {
        assert_eq!(link.text, label);
        assert_eq!(link.destination, destination);
        // Bare links identify their containing text event, not the URL start.
        if label != "https://body.test" {
            assert_eq!(link.source_offset, offset);
        }
        assert!(link.source_offset >= header.len());
        assert!(link.source_line >= header.lines().count());
    }
}

#[test]
fn frontmatter_keeps_invalid_yaml_visible_and_terminal_controls_inert() {
    let source = "---\ninvalid: [unterminated\nalias: *unknown\nkey: !!custom tag\nescape: \x1b[2J\x07\u{202e}\n---\n";
    for options in [RenderOptions::plain(), RenderOptions::default()] {
        let output = render(source, None, options);
        let observed = tokens(&output).text;
        assert_eq!(
            observed,
            "---\ninvalid: [unterminated\nalias: *unknown\nkey: !!custom tag\nescape: \\u{1b}[2J\\u{7}\\u{202e}\n---\n\n"
        );
        assert!(
            !output.contains("\x1b[2J") && !output.contains('\x07') && !output.contains('\u{202e}')
        );
    }
}

#[test]
fn frontmatter_propagates_writer_errors_and_cancellation() {
    struct Failed;
    impl Write for Failed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("frontmatter writer failed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let renderer = MarkdownRenderer::new();
    let source = "---\nkey: value\n---\n";
    assert_eq!(
        renderer
            .render_markdown(source, &mut Failed)
            .unwrap_err()
            .to_string(),
        "frontmatter writer failed"
    );
    let cancellation = RenderCancellation::new();
    cancellation.cancel();
    let mut output = Vec::new();
    assert_eq!(
        renderer
            .render_markdown_with_cancellation(source, &mut output, &cancellation)
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );
    assert!(output.is_empty());
}

#[test]
fn frontmatter_cli_is_general_for_files_and_stdin_and_dump_is_escape_free() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("unrelated.txt");
    let source = "---\ncustom-field: \"**literal**\"\n---\n\n# Body\n";
    std::fs::write(&path, source).unwrap();
    let expected = "---\ncustom-field: \"**literal**\"\n---\n\n# Body\n";
    for dump in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_blad"));
        command
            .args(["--color", "always"])
            .env("COLORTERM", "truecolor");
        if dump {
            command.arg("--dump");
        }
        let output = command.arg(&path).output().unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        let rendered = String::from_utf8(output.stdout).unwrap();
        assert_eq!(tokens(&rendered).text, expected);
        assert_eq!(rendered.contains('\x1b'), !dump);
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_blad"))
        .args(["--dump", "--color", "always"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}
