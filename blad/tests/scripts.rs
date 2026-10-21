use blad::{MarkdownRenderer, RenderOptions};

fn render(source: &str) -> String {
    let mut output = Vec::new();
    MarkdownRenderer::with_options(RenderOptions::plain())
        .render_markdown(source, &mut output)
        .unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn scripts_work_standalone_and_adjacent_without_added_spaces() {
    assert_eq!(
        render("^2^ ~2~ E=mc^2^ H~2~O x^n^ y~i~").trim(),
        "² ₂ E=mc² H₂O xⁿ yᵢ"
    );
    assert_eq!(render("^2^^3^ ~2~~3~").trim(), "²³ ₂₃");
}

#[test]
fn whole_spans_map_or_fall_back_without_partial_substitution() {
    assert_eq!(
        render("x^12+n^ y~12+x~ x^2q^ z~2Q~").trim(),
        "x¹²⁺ⁿ y₁₂₊ₓ x^(2q) z_(2Q)"
    );
    assert_eq!(render("x^(2)^ y~(2)~").trim(), "x⁽²⁾ y₍₂₎");
    assert_eq!(render("^0123456789+-=()in^").trim(), "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁱⁿ");
    assert_eq!(
        render("~0123456789+-=()aehijklmnoprstuvx~").trim(),
        "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ₐₑₕᵢⱼₖₗₘₙₒₚᵣₛₜᵤᵥₓ"
    );
}

#[test]
fn scripts_preserve_outer_formatting_and_table_geometry() {
    assert_eq!(
        render("**E=mc^2^** [H~2~O](https://example.test)").trim(),
        "E=mc² H₂O"
    );
    let table = render("| Formula |\n|---|\n| H~2~O |\n| x^n^ |");
    assert!(table.contains("H₂O") && table.contains("xⁿ"), "{table:?}");
    assert!(
        table
            .lines()
            .all(|line| unicode_width::UnicodeWidthStr::width(line) <= 7)
    );
    let mut styled = Vec::new();
    MarkdownRenderer::new()
        .render_markdown("**x^2^** tail [H~2~O](https://example.test)", &mut styled)
        .unwrap();
    let styled = String::from_utf8(styled).unwrap();
    assert!(styled.contains("\x1b[1mx²\x1b[0m"), "{styled:?}");
    assert!(styled.contains("\x1b[96mH₂O"), "{styled:?}");
}

#[test]
fn scripts_do_not_reinterpret_code_escapes_or_encoded_delimiters() {
    assert_eq!(render(r"H\~2\~O x\^2\^").trim(), "H~2~O x^2^");
    assert_eq!(
        render("`H~2~O x^2^`\n\n```text\nH~2~O x^2^\n```").trim(),
        "H~2~O x^2^\n\nH~2~O x^2^"
    );
    assert_eq!(render("x&#94;2&#94; H&#126;2&#126;O").trim(), "x^2^ H~2~O");
    assert_eq!(render("H~2~O &amp; x^2^ &lt; y~i~").trim(), "H₂O & x² < yᵢ");
    assert_eq!(render(r"x\^2^ and x^2\^").trim(), "x^2^ and x^2^");
    assert_eq!(render(r"x^2&amp;n^ x^2\+n^").trim(), "x^2&n^ x^2+n^");
}

#[test]
fn malformed_spans_and_strikethrough_keep_their_existing_meaning() {
    assert_eq!(
        render("~~gone~~ x^2 H~2 x^^ y~~").trim(),
        "gone x^2 H~2 x^^ y~~"
    );
    assert_eq!(
        render("x^two words^ y~two words~").trim(),
        "x^two words^ y~two words~"
    );
    assert_eq!(render("x^**2**^").trim(), "x^2^");
    assert_eq!(render("~~H~2~O~~").trim(), "H₂O");
}

#[test]
fn script_scanning_is_linear_on_unmatched_and_long_notation() {
    let source = format!("{} {}", "x^".repeat(20_000), "H~2~O ".repeat(2_000));
    let output = render(&source);
    assert!(output.contains("H₂O"));
    assert!(output.len() < source.len() * 4);
    let unmatched = format!("x^{}", "a".repeat(200_000));
    assert_eq!(render(&unmatched).trim(), unmatched);
}
