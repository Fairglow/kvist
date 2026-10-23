use std::io::Read;
use std::process::{Command, Stdio};

#[test]
fn cli_theme_hints_explicit_appearance_and_color_encodings_are_independent() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("policy.md");
    std::fs::write(&path, "# Heading\n\n```rust\nlet x = 1;\n```").unwrap();
    for (theme, hint, background) in [
        ("auto", "0;15", Some(254)),
        ("auto", "15;0", Some(236)),
        ("light", "15;0", Some(254)),
        ("dark", "0;15", Some(236)),
        ("terminal", "0;15", None),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_blad"))
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "")
            .env("NO_COLOR", "")
            .env("COLORFGBG", hint)
            .args(["--theme", theme, "--color", "always"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        if let Some(background) = background {
            assert!(
                text.contains(&format!("[48;5;{background}m")),
                "{theme} {hint}: {text:?}"
            );
        } else {
            assert!(!text.contains("[48;"), "{text:?}");
        }
    }
    for (term, colorterm, expected, excluded) in [
        ("xterm", "", "", ";5;"),
        ("xterm-256color", "", ";5;", ";2;"),
        ("xterm", "truecolor", ";2;", ""),
        ("xterm", "24bit", ";2;", ""),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_blad"))
            .env("TERM", term)
            .env("COLORTERM", colorterm)
            .env("NO_COLOR", "1")
            .args(["--color", "always"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(expected), "{text:?}");
        if !excluded.is_empty() {
            assert!(!text.contains(excluded), "{text:?}");
        }
        if term == "xterm" && colorterm.is_empty() {
            assert!(!text.contains(";2;"), "{text:?}");
        }
    }
}

#[test]
fn rendering_needs_no_runtime_tools_and_does_not_consume_stdin() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("portable.md");
    std::fs::write(&path, "# Portable\n\nH~2~O $x^2$\n\n```rust\nlet x = 1;\n```\n\n```mermaid\nflowchart LR\nA[Input] --> B[Output]\n```").unwrap();
    let stdin_path = temp.path().join("stdin");
    std::fs::write(&stdin_path, "DO NOT CONSUME THIS INPUT").unwrap();
    let mut stdin = std::fs::File::open(stdin_path).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_blad"))
        .env_clear()
        .env("PATH", "")
        .arg("--dump")
        .arg(&path)
        .stdin(Stdio::from(stdin.try_clone().unwrap()))
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("H₂O x²") && text.contains('┌'), "{text:?}");
    assert!(!text.contains('\x1b') && !text.contains("unsupported Mermaid"));
    let mut remaining = String::new();
    stdin.read_to_string(&mut remaining).unwrap();
    assert_eq!(remaining, "DO NOT CONSUME THIS INPUT");
}

#[test]
fn cli_projection_opt_outs_keep_source_and_plain_dump_precedence() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.md");
    let source = "$\\alpha  + x^n^$\n\n```mermaid\nflowchart LR\nA --> B\n```";
    std::fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_blad"))
        .args([
            "--dump",
            "--color",
            "always",
            "--math",
            "source",
            "--mermaid",
            "source",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("$\\alpha  + x^n^$") && text.contains("flowchart LR\nA --> B"),
        "{text:?}"
    );
    assert!(!text.contains('\x1b') && !text.contains('┌'));
}

#[test]
fn script_math_html_and_authored_unicode_boundaries_remain_distinct() {
    let mut output = Vec::new();
    blad::MarkdownRenderer::new().render_markdown(
        "# H~2~O\n\n> [!NOTE]\n> x^2^ $x_i^2$\n\nx^<span>2</span>^ x^<!--hidden-->2^ x^a~2~^ x^中^", &mut output,
    ).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(
        text.contains("H₂O") && text.contains("x²") && text.contains("xᵢ²"),
        "{text:?}"
    );
    assert!(text.contains("\x1b[34m") && text.contains("\x1b[96m") && text.contains("\x1b[95m"));
    let mut plain = Vec::new();
    blad::MarkdownRenderer::with_options(blad::RenderOptions::plain())
        .render_markdown(
            "x^<span>2</span>^ x^<!--hidden-->2^ x^a~2~^ x^中^",
            &mut plain,
        )
        .unwrap();
    assert_eq!(
        String::from_utf8(plain).unwrap().trim(),
        "x^2^ x^2^ x^(a~2~) x^(中)"
    );
}

#[test]
#[ignore = "opt-in warm/cold text-rendering characterization"]
fn characterize_repeated_text_rendering() {
    let source = "# A heading\n\nH~2~O and $E=mc^2$, **important** and [a link](https://example.test).\n\n```rust\nlet answer = 42;\n```\n\n| A | B |\n|---|---|\n| one | two |\n\n".repeat(100);
    let renderer = blad::MarkdownRenderer::new();
    let start = std::time::Instant::now();
    renderer.render_markdown(&source, &mut Vec::new()).unwrap();
    let cold = start.elapsed();
    for color_depth in [
        blad::ColorDepth::TrueColor,
        blad::ColorDepth::Ansi256,
        blad::ColorDepth::Ansi16,
    ] {
        let renderer = blad::MarkdownRenderer::with_options(blad::RenderOptions {
            color_depth,
            ..blad::RenderOptions::default()
        });
        let start = std::time::Instant::now();
        for _ in 0..100 {
            renderer.render_markdown(&source, &mut Vec::new()).unwrap();
        }
        eprintln!(
            "text bytes={} depth={color_depth:?} cold={cold:?} warm_mean={:?}",
            source.len(),
            start.elapsed() / 100
        );
    }
}
