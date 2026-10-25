use blad::{Color, ColorDepth, MarkdownRenderer, Palette, RenderOptions, Theme};
use std::process::Command;

fn render(source: &str, options: RenderOptions) -> String {
    let renderer = MarkdownRenderer::with_options(options);
    let mut output = Vec::new();
    renderer.render_markdown(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn plain_output_contains_rendered_text_but_no_controls() {
    let output = render(
        "# Heading\n\n**bold** and `code`\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```",
        RenderOptions::plain(),
    );
    assert!(output.contains("# Heading"));
    assert!(output.contains("bold and code"));
    assert!(output.contains("fn main()"));
    assert!(!output.contains('\x1b'), "{output:?}");
}

#[test]
fn color_depth_never_changes_visible_geometry() {
    let source = "# Heading\n\n> [!WARNING]\n> Careful\n\nH~2~O $\\alpha_i^2$ and $\\frac{a}{b}$\n\n```rust\nlet x = 1;\n```\n\n| A | B |\n|:---:|---:|\n| 1 | 2 |\n| longest | longest |\n\n```mermaid\nflowchart LR\nA[Input] --> B[Output]\n```";
    let expected = render(source, RenderOptions::plain());
    for theme in [Theme::Dark, Theme::Light, Theme::Terminal] {
        for color_depth in [
            ColorDepth::None,
            ColorDepth::Ansi16,
            ColorDepth::Ansi256,
            ColorDepth::TrueColor,
        ] {
            let output = render(
                source,
                RenderOptions {
                    theme,
                    color_depth,
                    ..RenderOptions::default()
                },
            );
            let stripped = strip_sgr(&output);
            assert_eq!(stripped, expected);
            if color_depth == ColorDepth::Ansi16 {
                assert!(!output.contains(";2;") && !output.contains(";5;"));
            }
            if color_depth == ColorDepth::None {
                assert!(!output.contains("[3") && !output.contains("[4"));
            }
        }
    }
}

#[test]
fn authored_terminal_controls_are_inert_even_in_highlighted_code() {
    let source = "text \x1b[31mred\x07\n\n```rust\n// \x1b[2J\n```";
    let output = render(source, RenderOptions::plain());
    assert!(output.contains("\\u{1b}[31mred\\u{7}"), "{output:?}");
    assert!(output.contains("\\u{1b}[2J"), "{output:?}");
    assert!(!output.contains('\x1b'));
    let output = render("a&#13;b\n\n```text\nx\ry\r\nz\n```", RenderOptions::plain());
    assert!(!output.contains('\r'), "{output:?}");
    assert!(output.contains("\\u{d}"), "{output:?}");
}

#[test]
fn entity_decoded_controls_cannot_become_renderer_owned_styling() {
    let source =
        "before&#7;&#127;&#27;[8mhidden&#27;[0m after &#27;[2J\n\n| A |\n|---|\n| &#27;[31mred |";
    for options in [RenderOptions::plain(), RenderOptions::default()] {
        let output = render(source, options);
        assert!(
            output.contains("\\u{7}\\u{7f}\\u{1b}[8mhidden\\u{1b}[0m"),
            "{output:?}"
        );
        assert!(output.contains("\\u{1b}[2J"), "{output:?}");
        assert!(!output.contains('\x07') && !output.contains('\x7f'));
        assert!(!output.contains("\x1b[8m") && !output.contains("\x1b[2J"));
        assert!(output.contains("\\u{1b}[31mred"));
    }
}

#[test]
fn invalid_explicit_syntax_theme_is_an_error() {
    let mut renderer = MarkdownRenderer::new();
    renderer.set_syntax_theme(Some("not-a-real-theme"));
    let error = renderer
        .render_markdown("```rust\nlet x = 1;\n```", &mut Vec::new())
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn cli_dump_forces_plain_output_and_accepts_theme_options() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("document.md");
    std::fs::write(&path, "# Heading\n\n**bold**").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_blad"))
        .args(["--theme", "light", "--color", "always", "--dump"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(!output.stdout.contains(&0x1b));
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("# Heading")
    );
}

#[test]
fn a_host_palette_overrides_each_rendered_semantic_role() {
    let palette = Palette {
        heading: Color::Rgb(1, 2, 3),
        link: Color::Rgb(4, 5, 6),
        muted: Color::Rgb(7, 8, 9),
        list_marker: Color::Rgb(10, 11, 12),
        alerts: [Color::Rgb(13, 14, 15); 5],
        code_background: Some(Color::Rgb(16, 17, 18)),
        table_background: Some(Color::Rgb(19, 20, 21)),
        math: Color::Rgb(22, 23, 24),
        ..Palette::default()
    };
    let output = render(
        "# H\n\n[link](https://example.test)\n\n---\n\n- item\n\n> [!NOTE]\n> note\n\n$x^2$\n\n```text\ncode\n```\n\n| A |\n|---|\n| B |\n",
        RenderOptions {
            palette: Some(palette),
            ..RenderOptions::default()
        },
    );
    for color in [
        "38;2;1;2;3",
        "38;2;4;5;6",
        "38;2;7;8;9",
        "38;2;10;11;12",
        "38;2;13;14;15",
        "48;2;16;17;18",
        "48;2;19;20;21",
        "38;2;22;23;24",
    ] {
        assert!(output.contains(color), "missing {color}: {output:?}");
    }
}

#[test]
fn terminal_native_appearance_never_adds_a_fixed_background() {
    let output = render(
        "```rust\nlet x = 1;\n```\n\n| A |\n|---|\n| B |",
        RenderOptions {
            theme: Theme::Terminal,
            ..RenderOptions::default()
        },
    );
    assert!(!output.contains("[48;"), "{output:?}");
}

#[test]
fn cli_color_policy_honors_nonempty_no_color_and_explicit_override() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("document.md");
    std::fs::write(&path, "# Heading").unwrap();
    for (color, expected) in [("auto", false), ("always", true), ("never", false)] {
        let output = Command::new(env!("CARGO_BIN_EXE_blad"))
            .env("NO_COLOR", "1")
            .env("TERM", "xterm-256color")
            .args(["--color", color])
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().contains("[34m"),
            expected
        );
    }
}

#[test]
fn renderer_propagates_both_writer_and_flush_failures() {
    struct Failed {
        flush_only: bool,
    }
    impl std::io::Write for Failed {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.flush_only {
                Ok(bytes.len())
            } else {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }
    for flush_only in [false, true] {
        assert_eq!(
            MarkdownRenderer::new()
                .render_markdown("text", &mut Failed { flush_only })
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::BrokenPipe
        );
    }
}

fn strip_sgr(source: &str) -> String {
    let mut output = String::new();
    let mut chars = source.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for next in chars.by_ref() {
                if next == 'm' {
                    break;
                }
            }
        } else {
            output.push(c);
        }
    }
    output
}
