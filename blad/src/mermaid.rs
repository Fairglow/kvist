//! Bounded native Mermaid-to-text adapter (Merman 0.9.1, MIT OR Apache-2.0).
//!
//! The admitted profile is spatial flowchart, sequence, state, class and ER:
//! boxes/shape approximations, labelled directed connections, groups, notes,
//! class members and ER attributes/cardinalities. Browser interactions, actor
//! properties/links, animation, accessibility metadata, configuration
//! directives, front matter and rich HTML labels (other than `<br>`) are
//! unsupported. Styling is not interpreted.
//! Class/ER scenes requiring Merman's textual `relations:` projection are
//! rejected rather than misrepresented as fully routed diagrams. Actors use
//! participant boxes; some composite-state boundary/cycle routes are unsupported.
//!
//! Canonical layout and Unicode's ordinary (not CJK ambiguous-wide) width
//! convention are fixed. Viewport width is deliberately not a layout bound:
//! callers horizontally scroll complete, unwrapped rows. Engine-owned label
//! layout can break long labels inside boxes; authored Unicode is never
//! transliterated by ASCII structural-glyph selection.
//!
//! Before layout: 64 KiB source, 8,192 lexical units, 256 nodes/participants
//! (including containers), 512 edges/messages, 32 nesting levels, 32,768 model
//! items and 256 KiB model text. During layout, the engine charges deterministic
//! work before execution (2,000,000 units), admits grids/documents before
//! allocation (262,144 cells), and admits encoded output before materialization
//! (2 MiB). Individual graphemes are limited to 256 UTF-8 bytes.
//! Cancellation and a 500 ms monotonic deadline are cooperative checkpoints,
//! not thread preemption or a promise that a callback returns within 500 ms.
//! There are no subprocesses, files, network requests or graphical intermediates.

use std::sync::OnceLock;
use std::time::Duration;

use merman::ascii::{
    AsciiColorMode, AsciiError, AsciiOutputOutcome, AsciiProjection, AsciiRenderOptions,
    AsciiRenderer, AsciiResourceLimitId, AsciiResourcePolicy, AsciiViewportPolicy,
};
use merman::diagram::RenderSemanticModel;
use merman::resources::{InputResourceLimitId, InputResourcePolicy, ResourceProfile};
use merman::{Engine, OperationLedgerError, ParseOptions};

pub(crate) use merman::OperationControl as DiagramControl;

const MAX_SOURCE: usize = 64 * 1024;
const MAX_LEXICAL_UNITS: usize = 8192;
const MAX_NODES: usize = 256;
const MAX_EDGES: usize = 512;
const MAX_NESTING: usize = 32;
const MAX_MODEL_ITEMS: usize = 32_768;
const MAX_MODEL_TEXT: usize = 256 * 1024;
const MAX_GRID: usize = 262_144;
const MAX_OUTPUT: usize = 2 * 1024 * 1024;
const MAX_WORK: usize = 2_000_000;
const MAX_GRAPHEME: usize = 256;
const DEADLINE: Duration = Duration::from_millis(500);

/// Failure categories remain distinct so the host never turns cancellation
/// into a successful source fallback.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum DiagramError {
    #[error("unsupported Mermaid content: {0}")]
    Unsupported(String),
    #[error("invalid Mermaid content: {0}")]
    Invalid(String),
    #[error("Mermaid resource limit: {0}")]
    Resource(String),
    #[error("Mermaid rendering cancelled: {0}")]
    Cancelled(String),
}

/// Render plain text. Width never rotates, clips or reflows completed rows.
pub(crate) fn render(
    source: &str,
    ascii: bool,
    width: Option<usize>,
) -> Result<String, DiagramError> {
    render_controlled(source, ascii, width, &DiagramControl::new())
}

/// The host can cancel through a retained clone, including from another thread.
/// Each render owns a child deadline without changing the host's control.
pub(crate) fn render_controlled(
    source: &str,
    ascii: bool,
    _width: Option<usize>,
    control: &DiagramControl,
) -> Result<String, DiagramError> {
    render_bounded(source, ascii, control, MAX_WORK, MAX_GRID, MAX_OUTPUT)
}

fn render_bounded(
    source: &str,
    ascii: bool,
    control: &DiagramControl,
    work: usize,
    cells: usize,
    bytes: usize,
) -> Result<String, DiagramError> {
    let control = control.child().with_deadline(DEADLINE);
    checkpoint(&control)?;
    preflight(source, &control)?;
    let mut input = InputResourcePolicy::for_profile(ResourceProfile::Constrained);
    for (id, value) in [
        (InputResourceLimitId::MaxSourceBytes, MAX_SOURCE),
        (InputResourceLimitId::MaxModelItems, MAX_MODEL_ITEMS),
        (InputResourceLimitId::MaxModelTextBytes, MAX_MODEL_TEXT),
        (InputResourceLimitId::MaxModelNestingDepth, MAX_NESTING),
    ] {
        input
            .apply_limit(id, value)
            .map_err(|error| DiagramError::Resource(error.to_string()))?;
    }
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    let engine = ENGINE.get_or_init(Engine::new);
    let context = engine
        .begin_operation()
        .map_err(|error| DiagramError::Invalid(error.to_string()))?;
    let parsed = engine
        .parse_diagram_for_render_model_controlled_in_context_sync(
            source,
            ParseOptions::strict(),
            &control,
            &context,
        )
        .map_err(|error| DiagramError::Cancelled(error.to_string()))?
        .map_err(parse_error)?
        .ok_or_else(|| DiagramError::Invalid("no diagram found".into()))?;
    checkpoint(&control)?;
    input
        .check_parsed_render(&parsed)
        .map_err(|error| DiagramError::Resource(error.to_string()))?;
    admit_model(parsed.model())?;
    let mut resources = AsciiResourcePolicy::for_profile(ResourceProfile::Constrained);
    for (id, value) in [
        (AsciiResourceLimitId::MaxGridCells, cells),
        (AsciiResourceLimitId::MaxDocumentCells, cells),
        (AsciiResourceLimitId::MaxOutputBytes, bytes),
        (AsciiResourceLimitId::MaxLayoutWorkUnits, work),
        (AsciiResourceLimitId::MaxNestingDepth, MAX_NESTING),
        (AsciiResourceLimitId::MaxGraphemeBytes, MAX_GRAPHEME),
    ] {
        resources
            .apply_limit(id, value)
            .map_err(|error| DiagramError::Resource(error.to_string()))?;
    }
    let options = if ascii {
        AsciiRenderOptions::ascii()
    } else {
        AsciiRenderOptions::unicode()
    }
    .with_color_mode(AsciiColorMode::Plain);
    let report = AsciiRenderer::new(options)
        .map_err(ascii_error)?
        .render_parsed_report(
            &parsed,
            AsciiViewportPolicy::unrestricted(),
            &control,
            &context,
            resources,
        )
        .map_err(ascii_error)?;
    checkpoint(&control)?;
    if report.projection != AsciiProjection::Diagrammatic
        || report.outcome == AsciiOutputOutcome::Fallback
        || report.text.lines().any(|line| line.trim() == "relations:")
    {
        return unsupported("this topology requires a non-spatial relation summary");
    }
    Ok(report.text)
}

fn checkpoint(control: &DiagramControl) -> Result<(), DiagramError> {
    control
        .checkpoint()
        .map_err(|error| DiagramError::Cancelled(error.to_string()))
}

fn unsupported<T>(message: &str) -> Result<T, DiagramError> {
    Err(DiagramError::Unsupported(message.into()))
}

fn ceiling(name: &str, actual: usize, maximum: usize) -> Result<(), DiagramError> {
    if actual > maximum {
        Err(DiagramError::Resource(format!(
            "{name}: {actual} exceeds {maximum}; simplify the diagram"
        )))
    } else {
        Ok(())
    }
}

/// Conservative lexical admission, not another Mermaid parser. In particular,
/// excluding endpoint products prevents short source from expanding to N² edges
/// inside the parser before the typed-model admission can run.
fn preflight(source: &str, control: &DiagramControl) -> Result<(), DiagramError> {
    ceiling("source bytes", source.len(), MAX_SOURCE)?;
    if source.chars().any(|ch| {
        (ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
            || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    }) {
        return Err(DiagramError::Invalid(
            "terminal or bidirectional controls are not admitted".into(),
        ));
    }
    if source.contains("%%{") {
        return unsupported("configuration directives");
    }
    for (offset, _) in source.match_indices('<') {
        if source[offset..].starts_with("<<") || source[..offset].ends_with('<') {
            continue;
        }
        let suffix = source[offset + 1..].trim_start_matches('/');
        if suffix.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
            let tag = suffix.split(|ch: char| !ch.is_ascii_alphabetic()).next();
            if !tag.is_some_and(|tag| tag.eq_ignore_ascii_case("br")) {
                return unsupported("rich HTML labels; only <br> line breaks are admitted");
            }
        }
    }
    let header = source
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
        .and_then(|line| {
            line.split(|ch: char| ch.is_whitespace() || ch == ';')
                .next()
        })
        .unwrap_or_default();
    if header.is_empty() {
        return Err(DiagramError::Invalid("no diagram header found".into()));
    }
    if !matches!(
        header,
        "flowchart"
            | "graph"
            | "sequenceDiagram"
            | "stateDiagram"
            | "stateDiagram-v2"
            | "classDiagram"
            | "erDiagram"
    ) {
        return unsupported("expected flowchart, sequence, state, class or ER syntax");
    }
    let mut units = 0usize;
    let mut delimiters = 0usize;
    let mut blocks = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut in_word = false;
    for line in source.lines() {
        checkpoint(control)?;
        let trimmed = line.trim();
        if trimmed == "---" {
            return unsupported("configuration directives and front matter");
        }
        if trimmed.starts_with("%%") {
            continue;
        }
        let mut at_start = true;
        let mut chars = line.char_indices().peekable();
        while let Some((offset, ch)) = chars.next() {
            if quote.is_none()
                && delimiters == 0
                && ch == '%'
                && chars.peek().is_some_and(|(_, next)| *next == '%')
            {
                break;
            }
            if let Some(delimiter) = quote {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == delimiter {
                    quote = None;
                }
                continue;
            }
            if at_start && delimiters == 0 && !ch.is_whitespace() {
                at_start = false;
                let first = line[offset..]
                    .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
                    .next()
                    .unwrap_or_default();
                if matches!(first, "accTitle" | "accDescr") {
                    return unsupported("accessibility metadata has no text-diagram projection");
                }
                if matches!(header, "flowchart" | "graph" | "sequenceDiagram") {
                    if matches!(
                        first,
                        "subgraph"
                            | "loop"
                            | "alt"
                            | "opt"
                            | "par"
                            | "critical"
                            | "break"
                            | "rect"
                            | "box"
                    ) {
                        blocks += 1;
                    } else if first == "end" {
                        blocks = blocks.saturating_sub(1);
                    }
                    ceiling("nesting depth", blocks, MAX_NESTING)?;
                }
            }
            if matches!(ch, '"' | '`') {
                quote = Some(ch);
            }
            if matches!(ch, '[' | '(' | '{') {
                delimiters += 1;
                ceiling("nesting depth", blocks + delimiters, MAX_NESTING)?;
            } else if matches!(ch, ']' | ')' | '}') {
                delimiters = delimiters.saturating_sub(1);
            }
            if ch == '&' && delimiters == 0 && matches!(header, "flowchart" | "graph") {
                return unsupported("multi-endpoint '&' shorthand; use individual edges");
            }
            if ch == ';' && delimiters == 0 {
                at_start = true;
            }
            let word = ch.is_alphanumeric() || ch == '_';
            if !ch.is_whitespace() && (!word || !in_word) {
                units += 1;
                if units.is_multiple_of(128) {
                    checkpoint(control)?;
                }
                ceiling("lexical work units", units, MAX_LEXICAL_UNITS)?;
            }
            in_word = word;
        }
        in_word = false;
    }
    Ok(())
}

fn admit_model(model: &RenderSemanticModel) -> Result<(), DiagramError> {
    let (nodes, edges) =
        match model {
            RenderSemanticModel::Flowchart(model) => {
                if model.nodes.iter().any(|node| {
                    node.link.is_some()
                        || node.have_callback
                        || node.icon.is_some()
                        || node.img.is_some()
                }) || model
                    .edges
                    .iter()
                    .any(|edge| edge.animate == Some(true) || edge.animation.is_some())
                {
                    return unsupported("flowchart interactions, icons, images or animation");
                }
                (model.nodes.len() + model.subgraphs.len(), model.edges.len())
            }
            RenderSemanticModel::Sequence(model) => {
                if model.actors.values().any(|actor| {
                    !actor.links.is_empty()
                        || !actor.properties.is_empty()
                        || !matches!(actor.actor_type.as_str(), "" | "participant" | "actor")
                }) {
                    return unsupported("sequence actor presentation metadata or links");
                }
                (model.actors.len() + model.boxes.len(), model.messages.len())
            }
            RenderSemanticModel::State(model) => {
                if !model.links.is_empty() {
                    return unsupported("state links");
                }
                (model.nodes.len().max(model.states.len()), model.edges.len())
            }
            RenderSemanticModel::Class(model) => {
                if model.classes.values().any(|node| {
                    node.link.is_some() || node.have_callback || node.callback.is_some()
                }) {
                    return unsupported("class interactions");
                }
                (
                    model.classes.len() + model.namespaces.len() + model.interfaces.len(),
                    model.relations.len(),
                )
            }
            RenderSemanticModel::Er(model) => (
                model.entities.len() + model.subgraphs.len(),
                model.relationships.len(),
            ),
            _ => return unsupported("the parsed family has no admitted spatial text backend"),
        };
    ceiling("nodes/participants", nodes, MAX_NODES)?;
    ceiling("edges/messages", edges, MAX_EDGES)
}

fn parse_error(error: merman::Error) -> DiagramError {
    let message = merman::ascii::normalize_terminal_diagnostic(&error.to_string());
    match error {
        merman::Error::UnsupportedDiagram { .. } => DiagramError::Unsupported(message),
        merman::Error::OperationCancelled(_) => DiagramError::Cancelled(message),
        _ => DiagramError::Invalid(message),
    }
}

fn ascii_error(error: AsciiError) -> DiagramError {
    let message = merman::ascii::normalize_terminal_diagnostic(&error.to_string());
    match error {
        AsciiError::Cancelled(_)
        | AsciiError::OperationResourceTerminal(OperationLedgerError::Cancelled(_)) => {
            DiagramError::Cancelled(message)
        }
        AsciiError::ResourceLimitExceeded(_)
        | AsciiError::AllocationFailed { .. }
        | AsciiError::OperationResourceTerminal(_) => DiagramError::Resource(message),
        AsciiError::UnsupportedDiagram { .. }
        | AsciiError::UnsupportedFeature { .. }
        | AsciiError::FallbackUnavailable { .. } => DiagramError::Unsupported(message),
        _ => DiagramError::Invalid(message),
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn engine_work_grid_and_output_limits_are_hard_errors() {
        for source in [
            "flowchart LR\nA[Alpha] -->|calls| B[Beta]",
            "sequenceDiagram\nA->>B: calls",
            "stateDiagram-v2\nAlpha --> Beta: calls",
            "classDiagram\nAlpha --> Beta: calls",
            "erDiagram\nAlpha ||--o{ Beta: owns",
        ] {
            for (work, cells, bytes) in [
                (1, MAX_GRID, MAX_OUTPUT),
                (MAX_WORK, 1, MAX_OUTPUT),
                (MAX_WORK, MAX_GRID, 1),
            ] {
                let control = DiagramControl::new();
                let error =
                    render_bounded(source, false, &control, work, cells, bytes).unwrap_err();
                assert!(matches!(error, DiagramError::Resource(_)), "{error}");
                assert!(render_controlled(source, false, None, &control).is_ok());
            }
        }
    }

    #[test]
    fn encoded_output_is_admitted_at_its_exact_byte_boundary() {
        for source in [
            "flowchart LR\nA[Alpha] --> B[Beta]",
            "sequenceDiagram\nA->>B: calls",
            "stateDiagram-v2\nAlpha --> Beta: calls",
            "classDiagram\nAlpha --> Beta: calls",
            "erDiagram\nAlpha ||--o{ Beta: owns",
        ] {
            let text = render(source, false, None).unwrap();
            let control = DiagramControl::new();
            assert_eq!(
                render_bounded(source, false, &control, MAX_WORK, MAX_GRID, text.len()).unwrap(),
                text,
            );
            assert!(matches!(
                render_bounded(source, false, &control, MAX_WORK, MAX_GRID, text.len() - 1),
                Err(DiagramError::Resource(_)),
            ));
        }
    }
}
