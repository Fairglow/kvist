use blad::{DiagramCharset, MarkdownRenderer, MermaidMode, RenderCancellation, RenderOptions};

fn render(
    source: &str,
    charset: DiagramCharset,
    mode: MermaidMode,
    width: Option<usize>,
) -> String {
    let mut renderer = MarkdownRenderer::with_options(RenderOptions {
        mermaid: mode,
        diagram_charset: charset,
        ..RenderOptions::plain()
    });
    renderer.set_width(width);
    let mut output = Vec::new();
    renderer
        .render_markdown(&format!("```mermaid\n{source}\n```"), &mut output)
        .unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn all_five_required_families_render_spatial_text_not_placeholders() {
    for (source, labels) in [
        (
            "flowchart LR\nA[Input] -->|accept| B[Output]",
            vec!["Input", "Output", "accept"],
        ),
        (
            "sequenceDiagram\nparticipant User\nparticipant Server\nUser->>Server: request",
            vec!["User", "Server", "request"],
        ),
        (
            "stateDiagram-v2\n[*] --> Idle\nIdle --> Busy: run\nBusy --> [*]",
            vec!["Idle", "Busy", "run"],
        ),
        (
            "classDiagram\nclass Reader\nclass Book\nReader : +read()\nReader --> Book : reads",
            vec!["Reader", "Book", "read()", "reads"],
        ),
        (
            "erDiagram\nCUSTOMER ||--o{ ORDER : places\nCUSTOMER {\nstring id PK\n}\nORDER {\nstring id PK\n}",
            vec!["CUSTOMER", "ORDER", "places", "id", "PK"],
        ),
    ] {
        let output = render(source, DiagramCharset::Unicode, MermaidMode::Text, None);
        assert!(!output.contains("[Mermaid diagram"), "{output}");
        assert!(
            !output.contains("mmdc") && !output.contains("<svg"),
            "{output}"
        );
        assert!(
            output.contains('┌') || output.contains('╭') || output.contains('┏'),
            "{output}"
        );
        for label in labels {
            assert!(output.contains(label), "missing {label}: {output}");
        }
    }
}

#[test]
fn diagram_geometry_does_not_rotate_or_reflow_at_narrow_widths() {
    let source = "flowchart LR\nA[Input] --> B[Output]";
    let wide = render(
        source,
        DiagramCharset::Unicode,
        MermaidMode::Text,
        Some(120),
    );
    let narrow = render(source, DiagramCharset::Unicode, MermaidMode::Text, Some(5));
    assert_eq!(wide, narrow);
    let labels = wide.lines().find(|line| line.contains("Input")).unwrap();
    assert!(
        labels.find("Input").unwrap() < labels.find("Output").unwrap(),
        "{wide}"
    );
    let reverse = render(
        "flowchart RL\nA[Input] --> B[Output]",
        DiagramCharset::Unicode,
        MermaidMode::Text,
        None,
    );
    let labels = reverse.lines().find(|line| line.contains("Input")).unwrap();
    assert!(
        labels.find("Output").unwrap() < labels.find("Input").unwrap(),
        "{reverse}"
    );
}

#[test]
fn ascii_changes_structure_without_losing_authored_unicode() {
    let output = render(
        "flowchart LR\nA[Καλημέρα] --> B[世界]",
        DiagramCharset::Ascii,
        MermaidMode::Text,
        None,
    );
    assert!(
        output.contains("Καλημέρα") && output.contains("世界"),
        "{output}"
    );
    assert!(
        output.contains('+') && output.contains('-') && output.contains('|'),
        "{output}"
    );
    assert!(!output.contains('┌') && !output.contains('─'), "{output}");
}

#[test]
fn source_mode_and_unsupported_diagrams_preserve_source_explicitly() {
    let source = "flowchart LR\nA[Input] --> B[Output]";
    let output = render(source, DiagramCharset::Unicode, MermaidMode::Source, None);
    assert!(
        output.contains(source) && output.contains("source"),
        "{output}"
    );
    let unsupported = "pie title Sales\n\"A\" : 20";
    let output = render(
        unsupported,
        DiagramCharset::Unicode,
        MermaidMode::Text,
        None,
    );
    assert!(
        output.contains(unsupported) && output.contains("unsupported Mermaid"),
        "{output}"
    );
    let invalid = "flowchart LR\nA[";
    let output = render(invalid, DiagramCharset::Unicode, MermaidMode::Text, None);
    assert!(
        output.contains(invalid) && output.contains("invalid Mermaid"),
        "{output}"
    );
}

#[test]
fn resource_fallback_is_bounded_and_reports_source_omission() {
    let source = format!("flowchart LR\n%% {}", "x".repeat(70_000));
    let output = render(&source, DiagramCharset::Unicode, MermaidMode::Text, None);
    assert!(output.contains("Mermaid resource limit") && output.contains("source bytes omitted"));
    assert!(output.len() < 66_000);
}

#[test]
fn explicit_cancellation_propagates_instead_of_source_fallback_success() {
    let cancellation = RenderCancellation::new();
    let clone = cancellation.clone();
    clone.cancel();
    let mut output = Vec::new();
    let error = MarkdownRenderer::new()
        .render_markdown_with_cancellation(
            "```mermaid\nflowchart LR\nA --> B\n```",
            &mut output,
            &cancellation,
        )
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    assert!(output.is_empty());
    let deadline = RenderCancellation::new().with_deadline(std::time::Duration::ZERO);
    assert_eq!(
        MarkdownRenderer::new()
            .render_markdown_with_cancellation("$x^2$", &mut Vec::new(), &deadline,)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::Interrupted
    );
}

#[test]
fn writer_failure_is_not_replaced_with_a_successful_diagram_fallback() {
    struct Failed;
    impl std::io::Write for Failed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let error = MarkdownRenderer::new()
        .render_markdown("```mermaid\nflowchart LR\nA --> B\n```", &mut Failed)
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
}

#[test]
fn rejected_and_source_diagrams_cannot_emit_invisible_bidi_controls() {
    let source = "flowchart LR\nA[\"before\u{202e}after\u{2066}\"] --> B";
    for mode in [MermaidMode::Text, MermaidMode::Source] {
        let output = render(source, DiagramCharset::Unicode, mode, None);
        assert!(
            !output.contains('\u{202e}') && !output.contains('\u{2066}'),
            "{output:?}"
        );
        assert!(
            output.contains("\\u{202e}") && output.contains("\\u{2066}"),
            "{output:?}"
        );
    }
}

#[test]
fn entity_decoded_labels_and_malformed_diagnostics_are_terminal_safe() {
    for source in [
        "flowchart LR\nA[\"before&#27;[8mhidden&#7;&#127;after\"] --> B",
        "flowchart LR\nA[\"before&#x202e;after\"] --> B",
        "flowchart LR\nA[\"broken&#27;[2J\"",
    ] {
        let output = render(source, DiagramCharset::Unicode, MermaidMode::Text, None);
        assert!(
            !output
                .chars()
                .any(|c| (c.is_control() && !matches!(c, '\n' | '\t'))
                    || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
            "{output:?}"
        );
        assert!(!output.is_empty());
    }
}

#[test]
fn cancellation_during_document_writes_stays_an_error() {
    struct Cancelling(blad::RenderCancellation);
    impl std::io::Write for Cancelling {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.cancel();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let cancellation = blad::RenderCancellation::new();
    let error = MarkdownRenderer::new()
        .render_markdown_with_cancellation(
            "# Heading\n\n$x^2$\n\n```mermaid\nflowchart LR\nA --> B\n```",
            &mut Cancelling(cancellation.clone()),
            &cancellation,
        )
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
}
