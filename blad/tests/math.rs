use blad::{MarkdownRenderer, MathMode, RenderOptions};

fn render(source: &str, mode: MathMode) -> String {
    let mut output = Vec::new();
    MarkdownRenderer::with_options(RenderOptions {
        math: mode,
        ..RenderOptions::plain()
    })
    .render_markdown(source, &mut output)
    .unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn inline_math_projects_literals_greek_operators_and_scripts() {
    let output = render(
        r"The formula $E=mc^2$ and $\alpha_i^2 \leq \infty$.",
        MathMode::Unicode,
    );
    assert_eq!(output.trim(), "The formula E=mc² and αᵢ² ≤ ∞.");
    assert_eq!(
        render(r"$x^{n+1}+x_{ij}+x^2_i$", MathMode::Unicode).trim(),
        "xⁿ⁺¹+xᵢⱼ+x²ᵢ"
    );
}

#[test]
fn fractions_roots_and_groups_never_lose_binding() {
    assert_eq!(
        render(r"$\frac{a+b}{\sqrt{c+d}}$", MathMode::Unicode).trim(),
        "(a+b)/(√(c+d))"
    );
    assert_eq!(
        render(r"${a+b}^2+\sqrt{x+1}$", MathMode::Unicode).trim(),
        "(a+b)²+√(x+1)"
    );
    assert_eq!(
        render(r"$\frac{1}{\frac{2}{3}}$", MathMode::Unicode).trim(),
        "(1)/((2)/(3))"
    );
    assert_eq!(render(r"$x^{a+b}$", MathMode::Unicode).trim(), "x^(a+b)");
    assert_eq!(
        render(r"$\frac{a}{b}^2$", MathMode::Unicode).trim(),
        "((a)/(b))²"
    );
    assert_eq!(
        render(r"$x/\frac{a}{b}$", MathMode::Unicode).trim(),
        "x/((a)/(b))"
    );
    assert_eq!(
        render(r"$\frac{a}{b}c$", MathMode::Unicode).trim(),
        "((a)/(b))c"
    );
    assert_eq!(
        render(r"$\text{a+b}^2$", MathMode::Unicode).trim(),
        "(a+b)²"
    );
}

#[test]
fn display_math_is_a_separate_unwrapped_text_block() {
    let output = render("Before\n\n$$\n\\frac{1}{2}\n$$\n\nAfter", MathMode::Unicode);
    assert!(output.contains("Before\n\n(1)/(2)\n\nAfter"), "{output:?}");
    assert_eq!(
        render("Before $$x^2$$ after", MathMode::Unicode).trim(),
        "Before\n\nx²\n\nafter"
    );
}

#[test]
fn greek_variants_and_named_operator_aliases_are_distinct() {
    assert_eq!(
        render(
            r"$\epsilon \varepsilon \phi \varphi \theta \vartheta$",
            MathMode::Unicode
        )
        .trim(),
        "ϵ ε ϕ φ θ ϑ"
    );
    assert_eq!(
        render(
            r"$\Gamma \Delta \sum_{i=0}^n \sin x \to \infty$",
            MathMode::Unicode
        )
        .trim(),
        "Γ Δ ∑ᵢ₌₀ⁿ sin x → ∞"
    );
    assert_eq!(
        render(
            r"$a\ne b \neq c \le d \leq e \ge f \geq g$",
            MathMode::Unicode
        )
        .trim(),
        "a≠ b ≠ c ≤ d ≤ e ≥ f ≥ g"
    );
}

#[test]
fn text_and_paired_delimiters_have_explicit_limited_semantics() {
    assert_eq!(
        render(
            r"$\text{speed} = \left(\frac{d}{t}\right)$",
            MathMode::Unicode
        )
        .trim(),
        "speed = ((d)/(t))"
    );
    assert_eq!(
        render(
            r"$\left\{x\right\} \left|y\right| \left\langle z\right\rangle$",
            MathMode::Unicode
        )
        .trim(),
        "{x} |y| ⟨ z⟩"
    );
    assert_eq!(
        render(r"$\text{a{b}c}+\text{50\%}$", MathMode::Unicode).trim(),
        "a{b}c+50%"
    );
}

#[test]
fn unsupported_syntax_retains_the_entire_original_expression() {
    for expression in [
        r"$\alpha+\unknown{x}$",
        r"$\input{secret}$",
        r"$\includegraphics{file}$",
        r"$\newcommand{\x}{a}$",
        r"$\sqrt[3]{x}$",
        r"$\text{\alpha}$",
    ] {
        let output = render(expression, MathMode::Unicode);
        assert!(output.contains(expression), "{output:?}");
        assert!(output.contains("[math unsupported:"), "{output:?}");
        assert!(!output.contains('α'), "{output:?}");
    }
}

#[test]
fn malformed_expressions_are_distinct_from_unsupported_syntax() {
    for expression in [
        r"$\frac{a}$",
        r"$\frac{}{b}$",
        r"$x^2^3$",
        r"$\left(x\right]$",
    ] {
        let output = render(expression, MathMode::Unicode);
        assert!(
            output.contains(expression) && output.contains("[math invalid:"),
            "{output:?}"
        );
    }
}

#[test]
fn source_mode_preserves_original_delimiters_and_whitespace() {
    for source in [r"$\alpha  +  x^{2}$", "$$\n\\frac{a}{b}\n$$", r"$x^n^$"] {
        assert_eq!(render(source, MathMode::Source).trim(), source.trim());
    }
}

#[test]
fn currency_escaped_dollars_unmatched_dollars_and_code_remain_literal() {
    for (source, expected) in [
        ("$5$", "$5$"),
        ("$12.50$", "$12.50$"),
        ("$-1,200.00$", "$-1,200.00$"),
        ("$5 and $10", "$5 and $10"),
        (r"\$x^2$", "$x^2$"),
        ("$x^2", "$x^2"),
        ("`$x^2$`", "$x^2$"),
        ("${x$", "${x$"),
    ] {
        assert_eq!(
            render(source, MathMode::Unicode).trim(),
            expected,
            "{source}"
        );
    }
    assert_eq!(render("$$5$$", MathMode::Unicode).trim(), "5");
}

#[test]
fn math_in_tables_and_surrounding_markup_remains_coherent() {
    let output = render(
        "| Formula | Meaning |\n|---|---|\n| $H_2O$ | water |\n| $\\sqrt{x}$ | root |",
        MathMode::Unicode,
    );
    assert!(
        output.contains("H₂O") && output.contains("√(x)"),
        "{output:?}"
    );
    assert_eq!(
        render(r"**$x^2$** and H~2~O", MathMode::Unicode).trim(),
        "x² and H₂O"
    );
}

#[test]
fn input_and_nesting_limits_are_inclusive_and_visibly_bounded() {
    let at_limit = format!("${}$", "x".repeat(16_384));
    assert!(!render(&at_limit, MathMode::Unicode).contains("[math resource limit:"));
    let too_large = format!("${}$", "x".repeat(20_000));
    let output = render(&too_large, MathMode::Unicode);
    assert!(
        output.contains("[math resource limit:") && output.contains("source bytes omitted"),
        "{output:?}"
    );
    assert!(output.len() < 17_000);
    let deep = |depth: usize| format!("${}x{}$", "{".repeat(depth), "}".repeat(depth));
    assert!(!render(&deep(32), MathMode::Unicode).contains("[math"));
    assert!(render(&deep(33), MathMode::Unicode).contains("[math resource limit:"));
}

#[test]
fn authored_controls_are_inert_in_formulas_and_fallbacks() {
    let output = render("$\\unknown{\x1b[2J}\x07$", MathMode::Unicode);
    assert!(!output.contains('\x1b') && !output.contains('\x07'));
    assert!(output.contains("\\u{1b}[2J") && output.contains("\\u{7}"));
    for mode in [MathMode::Unicode, MathMode::Source] {
        let output = render("$$a\rb$$", mode);
        assert!(!output.contains('\r'), "{output:?}");
        assert!(output.contains("\\u{d}"), "{output:?}");
    }
    assert_eq!(render("$$\r\nx^2\r\n$$", MathMode::Unicode).trim(), "x²");
}
