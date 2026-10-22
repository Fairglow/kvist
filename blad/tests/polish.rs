use blad::{ColorDepth, MarkdownRenderer, RenderOptions};
use std::process::Command;

fn render(source: &str, width: usize, plain: bool) -> String {
    let mut renderer = MarkdownRenderer::with_options(RenderOptions {
        plain,
        ..RenderOptions::default()
    });
    renderer.set_width(Some(width));
    let mut bytes = Vec::new();
    renderer.render_markdown(source, &mut bytes).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn nested_lists_resume_each_parents_own_number() {
    let output = render(
        "7. parent\n   1. child\n   2. child\n   3. child\n8. next",
        40,
        true,
    );
    assert!(output.contains("8. next"), "{output}");
    assert!(!output.contains("10. next"), "{output}");
    let bullets = render("- first\n  - second\n    - third", 40, true);
    assert!(
        bullets.contains("• first") && bullets.contains("◦ second") && bullets.contains("– third"),
        "{bullets}"
    );
}

#[test]
fn quotes_have_one_surrounding_blank_and_bordered_internal_blanks() {
    let output = render("before\n\n> first\n>\n> second\n\nafter", 20, true);
    let lines: Vec<_> = output.lines().map(str::trim_end).collect();
    assert_eq!(
        lines,
        ["before", "", "│ first", "│", "│ second", "", "after"]
    );
    for width in 1..25 {
        let output = render("> abcdef ghijkl\n>\n> > nested quote", width, true);
        assert!(
            output
                .lines()
                .all(|line| unicode_width::UnicodeWidthStr::width(line) <= width),
            "{width}: {output}"
        );
    }
}

#[test]
fn quote_background_survives_inline_resets_and_alerts_stay_colored() {
    let output = render("> [!NOTE]\n> **bold** and `code`", 30, false);
    assert!(output.contains("\x1b[96m"), "{output:?}");
    assert!(
        output.lines().all(|line| line.contains("\x1b[48;5;235m")),
        "{output:?}"
    );
    assert!(output.contains("\x1b[0m\x1b[48;5;235m"), "{output:?}");
}

#[test]
fn quotes_decorate_code_tables_nested_quotes_and_footnote_cells() {
    let source = "> intro\n>\n> ```python\n> print('yes')\n> ```\n>\n> | Footnote |\n> |---|\n> | ref[^2] |\n>\n> > nested\n\n[^2]: text";
    let output = render(source, 30, true);
    for line in output.lines().take_while(|line| !line.contains("² text")) {
        if !line.trim().is_empty() {
            assert!(line.starts_with("│ "), "{output}");
        }
    }
    assert!(
        output.contains("ref²") && output.contains("│ │ nested"),
        "{output}"
    );
    assert!(
        output
            .lines()
            .all(|line| unicode_width::UnicodeWidthStr::width(line) <= 30)
    );
}

#[test]
fn showcase_features_retain_their_semantics() {
    let output = render(
        "a[^12] b[^name]\n\n[^12]: number\n[^name]: named\n\n---\n\n***\n\n~~~text\nliteral\n~~~",
        30,
        true,
    );
    assert!(output.contains("a¹² b[name]"), "{output}");
    assert!(
        output.contains("¹² number") && output.contains("[name] named"),
        "{output}"
    );
    assert!(output.contains('─') && output.contains('━') && output.contains("literal"));
    for syntax in ["bash", "python"] {
        let mut renderer = MarkdownRenderer::with_options(RenderOptions {
            color_depth: ColorDepth::Ansi16,
            ..RenderOptions::default()
        });
        renderer.set_width(Some(50));
        let mut bytes = Vec::new();
        renderer
            .render_markdown(
                &format!("```{syntax}\n# comment\nfor x in items:\n    print(\"string\")\n```"),
                &mut bytes,
            )
            .unwrap();
        let output = String::from_utf8(bytes).unwrap();
        let colors: std::collections::BTreeSet<_> = output
            .split("\x1b[")
            .filter_map(|s| s.split_once('m').map(|(s, _)| s))
            .filter(|s| s.starts_with('3') || s.starts_with('9'))
            .collect();
        assert!(colors.len() >= 2, "{syntax}: {output:?}");
        assert!(output.lines().next().unwrap().contains("\x1b[40m"));
        assert!(output.lines().last().unwrap().contains("\x1b[40m"));
    }
}

#[test]
fn theme_file_is_strict_bounded_and_independent_of_plain_policy() {
    let temp = tempfile::tempdir().unwrap();
    let doc = temp.path().join("doc.md");
    let theme = temp.path().join("theme.json");
    std::fs::write(&doc, "# Heading\n\n> quote\n\n```text\ncode\n```").unwrap();
    std::fs::write(
        &theme,
        r##"{"theme":"light","palette":{"heading":"#010203","quote_background":17}}"##,
    )
    .unwrap();
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_blad"))
            .env("COLORTERM", "truecolor")
            .args(["--color", "always", "--theme-file"])
            .arg(&theme)
            .args(extra)
            .arg("view")
            .arg(&doc)
            .output()
            .unwrap()
    };
    let output = run(&[]);
    assert!(output.status.success(), "{:?}", output.stderr);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("\x1b[38;2;1;2;3m") && text.contains("\x1b[48;5;254m"),
        "{text:?}"
    );
    assert!(text.contains("\x1b[48;5;17m"), "{text:?}");
    let dark = String::from_utf8(run(&["--theme", "dark"]).stdout).unwrap();
    assert!(dark.contains("\x1b[48;5;236m"), "{dark:?}");
    assert!(
        !String::from_utf8(run(&["--dump"]).stdout)
            .unwrap()
            .contains('\x1b')
    );
    for invalid in [
        r#"{"typo":1}"#.to_string(),
        r#"{"palette":{"heading":"red"}}"#.to_string(),
        r#"{"palette":{"alerts":[1,2]}}"#.to_string(),
        r#"{"palette":{"heading":256}}"#.to_string(),
        r#"{"palette":{"heading":null}}"#.to_string(),
        r#"{"palette":{"\u001b[2J":1}}"#.to_string(),
        r#"{"\u001b[2J":1}"#.to_string(),
        "{".to_string(),
        " ".repeat(65_537),
    ] {
        std::fs::write(&theme, invalid).unwrap();
        let output = run(&[]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("theme"));
        assert!(!output.stderr.contains(&0x1b));
    }
    std::fs::write(&theme, format!("{{}}{}", " ".repeat(65_534))).unwrap();
    assert!(run(&[]).status.success(), "64 KiB is an inclusive bound");
    std::fs::remove_file(&theme).unwrap();
    assert!(!run(&[]).status.success());
}
