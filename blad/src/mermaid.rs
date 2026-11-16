//! Bounded native Mermaid-to-text adapter (Merman 0.10, MIT OR Apache-2.0).
//!
//! The admitted profile is spatial flowchart, sequence, state, class and ER:
//! boxes/shape approximations, labelled directed connections, groups, notes,
//! class members and ER attributes/cardinalities. Browser interactions, actor
//! properties/links, animation, accessibility metadata, configuration
//! directives, front matter and rich HTML labels (other than `<br>` line
//! breaks) are unsupported. Styling is not interpreted.
//!
//! Class/ER scenes that Merman resolves through its textual `relations:`
//! projection are rejected rather than misrepresented as fully routed
//! diagrams. Actors use participant boxes; some composite-state
//! boundary/cycle routes are unsupported.
//!
//! Merman 0.10 exposes exactly one further *spatial* (diagrammatic) ASCII
//! family, `xychart`; it is deliberately not admitted because that backend
//! always prepends machine-readable data-disclosure lines (e.g. `values:` /
//! `xDomain:` / per-point `samples=[...]`) above the chart, which a spatial
//! text viewer must not surface.
//!
//! Tree-shaped families are admitted in a limited, growing form: `treeView`
//! and `mindmap` render through this adapter's own bounded tree projection
//! (`MermaidTreeMode::Text`, the default), drawn from the typed model, so no
//! Merman model-evidence framing reaches the terminal. The remaining
//! `structured_text` families (gantt, journey, kanban, packet, timeline, and
//! the rest) are still rejected for now; that is a deliberate staged-support
//! choice, not a parsing limitation. `MermaidTreeMode::Merman` is a
//! debug-only pass-through that shows Merman's own raw `structured_text`
//! projection for those admitted families; that framing is Merman-internal
//! formatting (a private `safe_text::framing` module, no public parse helper,
//! no stability guarantee) and is best-effort only.
//!
//! Labels admit only authored `<br>` line breaks (`<br/>`, `<br >`, and
//! `<BR>` all work); they map to real text line breaks. Any *other* literal
//! HTML tag is rejected, because Merman's plain-text backend does not render
//! HTML faithfully: it keeps such tags literally in the output (and re-escapes
//! a bare `&` to `&amp;` once a tag is present), which a spatial text viewer
//! must not surface as if it were styled. HTML entities pass through to
//! Merman's terminal-safe normalizer rather than being admitted or stripped by
//! this adapter; that normalizer guarantees no raw control or bidirectional
//! escape is ever emitted, but its entity decoding is deliberately not
//! browser-faithful.
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

use std::time::Duration;

use merman::ascii::{
    AsciiColorMode, AsciiError, AsciiOutputOutcome, AsciiProjection, AsciiRenderOptions,
    AsciiResourceLimitId, AsciiResourcePolicy, AsciiViewportPolicy,
};
use merman::model::resources::{InputResourceLimitId, InputResourcePolicy, ResourceProfile};
use merman::model::{OperationLedgerError, SemanticDiagramModel};
use merman::{
    AsciiRequest, MermaidConfig, ParseOptions, RenderError, RenderOutput, RenderTarget, Renderer,
    SemanticParser,
};

pub(crate) use merman::OperationControl as DiagramControl;

/// Tree-family (`treeView`/`mindmap`) projection selection.
///
/// Re-exported from `theme` as the public `blad::MermaidTreeMode`. Defined here
/// (a mermaid concept) so the `#[path]`-includer contract test crate stays
/// self-contained.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum MermaidTreeMode {
    /// Blad's own bounded tree projection: clean, no model-evidence framing.
    #[default]
    Text,
    /// Debug only: show Merman's raw `structured_text` projection, including
    /// its internal framing (best-effort; not a stable format).
    Merman,
}

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
const MAX_TREE_NODES: usize = 4096;
const MAX_TREE_DEPTH: usize = 64;
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
/// Tree families use this adapter's bounded tree projection.
///
/// Retained as the stable default-mode entry point (the contract suite calls
/// these directly); production rendering goes through [`render_with_tree`].
#[allow(dead_code)]
pub(crate) fn render(
    source: &str,
    ascii: bool,
    width: Option<usize>,
) -> Result<String, DiagramError> {
    render_controlled(source, ascii, width, &DiagramControl::new())
}

/// The host can cancel through a retained clone, including from another thread.
/// Each render owns a child deadline without changing the host's control.
#[allow(dead_code)]
pub(crate) fn render_controlled(
    source: &str,
    ascii: bool,
    _width: Option<usize>,
    control: &DiagramControl,
) -> Result<String, DiagramError> {
    render_bounded(
        source,
        ascii,
        control,
        MAX_WORK,
        MAX_GRID,
        MAX_OUTPUT,
        MermaidTreeMode::Text,
    )
}

/// Like [`render`], with an explicit tree-family projection mode.
pub(crate) fn render_with_tree(
    source: &str,
    ascii: bool,
    width: Option<usize>,
    tree: MermaidTreeMode,
) -> Result<String, DiagramError> {
    render_controlled_with_tree(source, ascii, width, &DiagramControl::new(), tree)
}

/// Like [`render_controlled`], with an explicit tree-family projection mode.
pub(crate) fn render_controlled_with_tree(
    source: &str,
    ascii: bool,
    _width: Option<usize>,
    control: &DiagramControl,
    tree: MermaidTreeMode,
) -> Result<String, DiagramError> {
    render_bounded(source, ascii, control, MAX_WORK, MAX_GRID, MAX_OUTPUT, tree)
}

fn render_bounded(
    source: &str,
    ascii: bool,
    control: &DiagramControl,
    work: usize,
    cells: usize,
    bytes: usize,
    tree: MermaidTreeMode,
) -> Result<String, DiagramError> {
    let control = control.child().with_deadline(DEADLINE);
    checkpoint(&control)?;
    preflight(source, &control, tree)?;
    let renderer = build_renderer()?;
    // `parse_semantic` consumes its operation control by value; a clone shares
    // the same cancellation/deadline state and is cheap (an `Arc` bump). The
    // clone drives the bounded tree render's cooperative checkpoints when the
    // tree family is projected by Blad rather than the ASCII backend.
    let parse_control = control.clone();
    let artifact = renderer
        .parse_semantic(source, parse_control)
        .map_err(render_error)?
        .ok_or_else(|| DiagramError::Invalid("no diagram found".into()))?;
    // The production typed-model admission is skipped in the Merman debug
    // pass-through: there Merman's own parser and the input resource policy
    // are the bounds, and the goal is to observe Merman's raw projection.
    if tree == MermaidTreeMode::Text {
        admit_model(artifact.model())?;
    }
    let family = artifact.diagram_type();
    let is_tree_family = matches!(family, "treeView" | "mindmap");
    if is_tree_family && tree == MermaidTreeMode::Text {
        // Default: this adapter's own bounded tree projection. Clean output,
        // no Merman model-evidence framing, bounded independently of the ASCII
        // layout budget.
        return render_tree(artifact, ascii, &control);
    }
    if family == "gitGraph" && tree == MermaidTreeMode::Text {
        // This adapter's own bounded git graph projection: branch rails,
        // commit markers, merge joins. Clean output with no Merman
        // model-evidence framing, bounded independently of the ASCII
        // layout budget.
        return render_git_graph(artifact, ascii, &control);
    }
    if family == "gantt" && tree == MermaidTreeMode::Text {
        return render_gantt(artifact, ascii, &control);
    }
    if family == "timeline" && tree == MermaidTreeMode::Text {
        return render_timeline(artifact, ascii, &control);
    }
    if family == "journey" && tree == MermaidTreeMode::Text {
        return render_journey(artifact, ascii, &control);
    }
    if family == "kanban" && tree == MermaidTreeMode::Text {
        return render_kanban(artifact, ascii, &control);
    }
    if family == "packet" && tree == MermaidTreeMode::Text {
        return render_packet(artifact, ascii, &control);
    }
    if family == "xychart" && tree == MermaidTreeMode::Text {
        return render_xy_chart(artifact, ascii, &control);
    }
    // Merman ASCII backend. For spatial (diagrammatic) families this is the
    // production path. For structured_text families it is a debug-only
    // pass-through (MermaidTreeMode::Merman) showing Merman's own projection
    // including its internal framing; the projection gate is skipped in that
    // mode. Text mode for a non-tree structured_text family is rejected below.
    let target = RenderTarget::Ascii(AsciiRequest {
        options: if ascii {
            AsciiRenderOptions::ascii()
        } else {
            AsciiRenderOptions::unicode()
        }
        .with_color_mode(AsciiColorMode::Plain),
        resources: ascii_resources(work, cells, bytes)?,
        viewport: AsciiViewportPolicy::unrestricted(),
    });
    let RenderOutput::Ascii(Some(report)) = artifact.render(target).map_err(render_error)? else {
        return Err(DiagramError::Invalid("no diagram found".into()));
    };
    if tree == MermaidTreeMode::Merman {
        return Ok(report.text);
    }
    if report.projection != AsciiProjection::Diagrammatic {
        return unsupported(
            "this family has no spatial text projection; \
             structured_text support is staged and not yet admitted",
        );
    }
    if report.outcome == AsciiOutputOutcome::Fallback
        || report.text.lines().any(|line| line.trim() == "relations:")
    {
        return unsupported("this topology requires a non-spatial relation summary");
    }
    Ok(report.text)
}

/// Build a strict renderer for one operation. Construction is cheap (Merman caches
/// the hardened site configuration in a process-global static), so the renderer is
/// built per render rather than held in a global. Each operation still owns its own
/// cancellation, deadline, and output policy; the fixed input admission policy and
/// strict parse options are the only shared defaults.
///
/// Merman 0.10 introduced a parser-internal flowchart `maxEdges` budget (default 500)
/// that is stricter than this adapter's `MAX_EDGES` model admission. Left at its default
/// it would reject a 513-edge source during parsing (a parse error) instead of after the
/// typed model is admitted (a resource error). The host override lifts that internal
/// budget to `MAX_MODEL_ITEMS`, well above the product ceiling, so the adapter's
/// `admit_model` — not a library parse default — remains the authoritative resource
/// boundary, and an over-limit source is reported as a resource limit, not a parse error.
fn build_renderer() -> Result<Renderer, DiagramError> {
    let mut input = InputResourcePolicy::for_profile(ResourceProfile::Constrained);
    for (id, value) in [
        (InputResourceLimitId::MaxSourceBytes, MAX_SOURCE),
        (InputResourceLimitId::MaxModelItems, MAX_MODEL_ITEMS),
        (InputResourceLimitId::MaxModelTextBytes, MAX_MODEL_TEXT),
        (InputResourceLimitId::MaxModelNestingDepth, MAX_NESTING),
    ] {
        input
            .apply_limit(id, value)
            .map_err(|error| DiagramError::Resource(format!("input policy: {error}")))?;
    }
    let parser = SemanticParser::new().with_site_config(MermaidConfig::from_value(
        serde_json::json!({ "maxEdges": MAX_MODEL_ITEMS }),
    ));
    Ok(Renderer::new()
        .with_semantic_parser(parser)
        .with_parse_options(ParseOptions::strict())
        .with_resource_policy(input))
}

fn ascii_resources(
    work: usize,
    cells: usize,
    bytes: usize,
) -> Result<AsciiResourcePolicy, DiagramError> {
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
            .map_err(|error| DiagramError::Resource(format!("ascii policy: {error}")))?;
    }
    Ok(resources)
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
///
/// `tree == MermaidTreeMode::Text` (production) additionally enforces the
/// strict family allowlist and the `<br>`-only HTML rule; the tree-shaped
/// families are admitted here so Blad can render them through its own bounded
/// projection. `MermaidTreeMode::Merman` is a debug pass-through and skips
/// those production gates so Merman's own projection can be observed for any
/// family; the source-size, control-character and nesting ceilings still apply.
fn preflight(
    source: &str,
    control: &DiagramControl,
    tree: MermaidTreeMode,
) -> Result<(), DiagramError> {
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
    if tree == MermaidTreeMode::Text {
        if !matches!(
            header,
            "flowchart"
                | "graph"
                | "sequenceDiagram"
                | "stateDiagram"
                | "stateDiagram-v2"
                | "classDiagram"
                | "erDiagram"
                | "mindmap"
                | "treeView-beta"
                | "gitGraph"
                | "gantt"
                | "timeline"
                | "journey"
                | "kanban"
                | "packet"
                | "xychart-beta"
        ) {
            return unsupported(
                "expected flowchart, sequence, state, class, ER, mindmap, treeView, gantt, timeline, journey, kanban, packet or xychart-beta syntax",
            );
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
            if matches!(ch, '\"' | '`') {
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

fn admit_model(model: &SemanticDiagramModel) -> Result<(), DiagramError> {
    // Tree-shaped families have their own (larger, shape-specific) ceilings and
    // are admitted here rather than through the spatial node/edge ceilings.
    match model {
        SemanticDiagramModel::TreeView(model) => {
            let mut nodes = 0usize;
            let mut depth = 0usize;
            count_tree_nodes(&model.root, 0, &mut nodes, &mut depth)?;
            return Ok(());
        }
        SemanticDiagramModel::Mindmap(model) => {
            admit_mindmap(model)?;
            return Ok(());
        }
        SemanticDiagramModel::GitGraph(model) => {
            ceiling("gitGraph commits", model.commits.len(), MAX_NODES)?;
            return Ok(());
        }
        SemanticDiagramModel::Gantt(model) => {
            ceiling("gantt tasks", model.tasks.len(), MAX_NODES)?;
            ceiling("gantt sections", model.sections.len(), 64)?;
            return Ok(());
        }
        SemanticDiagramModel::Timeline(model) => {
            ceiling("timeline tasks", model.tasks.len(), MAX_NODES)?;
            ceiling("timeline sections", model.sections.len(), 64)?;
            return Ok(());
        }
        SemanticDiagramModel::Journey(model) => {
            ceiling("journey tasks", model.tasks.len(), MAX_NODES)?;
            ceiling("journey sections", model.sections.len(), 64)?;
            ceiling("journey actors", model.actors.len(), 16)?;
            return Ok(());
        }
        SemanticDiagramModel::Kanban(model) => {
            ceiling("kanban nodes", model.nodes.len(), MAX_NODES)?;
            return Ok(());
        }
        SemanticDiagramModel::Packet(model) => {
            let total = model
                .packet
                .iter()
                .map(|row| row.len())
                .sum::<usize>();
            ceiling("packet blocks", total, MAX_NODES)?;
            ceiling("packet rows", model.packet.len(), 64)?;
            return Ok(());
        }
        SemanticDiagramModel::XyChart(model) => {
            ceiling("xychart plots", model.plots.len(), 16)?;
            let total = model
                .plots
                .iter()
                .map(|p| p.values.len())
                .sum::<usize>();
            ceiling("xychart data points", total, MAX_NODES)?;
            return Ok(());
        }
        _ => {}
    }
    let (nodes, edges) =
        match model {
            SemanticDiagramModel::Flowchart(model) => {
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
            SemanticDiagramModel::Sequence(model) => {
                if model.actors.values().any(|actor| {
                    !actor.links.is_empty()
                        || !actor.properties.is_empty()
                        || !matches!(actor.actor_type.as_str(), "" | "participant" | "actor")
                }) {
                    return unsupported("sequence actor presentation metadata or links");
                }
                (model.actors.len() + model.boxes.len(), model.messages.len())
            }
            SemanticDiagramModel::State(model) => {
                if !model.links.is_empty() {
                    return unsupported("state links");
                }
                (model.nodes.len().max(model.states.len()), model.edges.len())
            }
            SemanticDiagramModel::Class(model) => {
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
            SemanticDiagramModel::Er(model) => (
                model.entities.len() + model.subgraphs.len(),
                model.relationships.len(),
            ),
            _ => return unsupported("the parsed family has no admitted spatial text backend"),
        };
    ceiling("nodes/participants", nodes, MAX_NODES)?;
    ceiling("edges/messages", edges, MAX_EDGES)
}

/// Bounded, iterative mindmap admission: total node count and maximum depth,
/// without recursion (so a hostile tree cannot overflow the stack before we
/// draw it). A mindmap with more than one root is rejected.
fn admit_mindmap(
    model: &merman::model::families::mindmap::MindmapDiagramModel,
) -> Result<(), DiagramError> {
    ceiling("tree nodes", model.nodes.len(), MAX_TREE_NODES)?;
    if model.nodes.iter().filter(|n| n.is_root).count() > 1 {
        return unsupported("mindmap has more than one root");
    }
    // Iterative depth computation over the child-id adjacency.
    let by_id: std::collections::HashMap<i32, usize> = model
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect();
    let roots: Vec<usize> = model
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.is_root)
        .map(|(i, _)| i)
        .collect();
    let mut stack: Vec<(usize, usize)> = roots.iter().map(|&i| (i, 0)).collect();
    let mut depth = 0usize;
    while let Some((idx, d)) = stack.pop() {
        depth = depth.max(d);
        if depth > MAX_TREE_DEPTH {
            return Err(DiagramError::Resource(format!(
                "tree depth: {depth} exceeds {MAX_TREE_DEPTH}; simplify the tree"
            )));
        }
        for &child in &model.nodes[idx].children {
            if let Some(&ci) = by_id.get(&child) {
                stack.push((ci, d + 1));
            }
        }
    }
    Ok(())
}

/// Count a treeView's nodes and maximum depth within the bounded ceilings, so a
/// hostile tree cannot drive unbounded recursion before we render it.
fn count_tree_nodes(
    node: &merman::model::families::tree_view::TreeViewNode,
    depth: usize,
    nodes: &mut usize,
    max_depth: &mut usize,
) -> Result<(), DiagramError> {
    *nodes += 1;
    if *nodes > MAX_TREE_NODES {
        return Err(DiagramError::Resource(format!(
            "tree nodes: {nodes} exceeds {MAX_TREE_NODES}; simplify the tree"
        )));
    }
    if depth > MAX_TREE_DEPTH {
        return Err(DiagramError::Resource(format!(
            "tree depth: {depth} exceeds {MAX_TREE_DEPTH}; simplify the tree"
        )));
    }
    *max_depth = (*max_depth).max(depth);
    for child in &node.children {
        count_tree_nodes(child, depth + 1, nodes, max_depth)?;
    }
    Ok(())
}

/// Bounded, Blad-owned tree projection for `treeView` and `mindmap`.
///
/// Renders from the typed model (not Merman's ASCII backend) so the output is
/// clean terminal text with no model-evidence framing. Tree connectors and
/// indentation only; no width reflow of completed rows.
fn render_tree(
    artifact: merman::SemanticArtifact,
    ascii: bool,
    control: &DiagramControl,
) -> Result<String, DiagramError> {
    let glyphs = if ascii {
        TreeGlyphs {
            branch: "+-- ",
            end: "`-- ",
            pipe: "|   ",
            space: "    ",
        }
    } else {
        TreeGlyphs {
            branch: "├── ",
            end: "└── ",
            pipe: "│   ",
            space: "    ",
        }
    };
    let mut out = String::new();
    match artifact.model() {
        SemanticDiagramModel::TreeView(model) => {
            if let Some(title) = model.title.as_deref().filter(|t| !t.trim().is_empty()) {
                out.push_str(title.trim());
                out.push('\n');
            }
            render_node_children(&model.root.children, &glyphs, &mut out, control)?;
        }
        SemanticDiagramModel::Mindmap(model) => {
            let nodes = &model.nodes;
            let root_idx = nodes.iter().position(|n| n.is_root);
            let Some(root_idx) = root_idx else {
                return Err(DiagramError::Invalid("mindmap has no root node".into()));
            };
            let child_count = nodes.iter().map(|n| n.children.len()).sum::<usize>();
            ceiling("tree nodes", nodes.len() + child_count, MAX_TREE_NODES)?;
            out.push_str(nodes[root_idx].descr.trim());
            out.push('\n');
            render_mindmap_level(
                nodes,
                nodes[root_idx].children.as_slice(),
                &glyphs,
                &mut out,
                control,
            )?;
        }
        _ => return Err(DiagramError::Invalid("not a tree family".into())),
    }
    Ok(out)
}

fn render_node_children(
    children: &[merman::model::families::tree_view::TreeViewNode],
    glyphs: &TreeGlyphs,
    out: &mut String,
    control: &DiagramControl,
) -> Result<(), DiagramError> {
    for (i, child) in children.iter().enumerate() {
        checkpoint(control)?;
        let is_last = i + 1 == children.len();
        out.push_str(if is_last { glyphs.end } else { glyphs.branch });
        out.push_str(child.name.trim());
        if let Some(desc) = child
            .description
            .as_deref()
            .filter(|d| !d.trim().is_empty())
        {
            out.push_str(&format!(" ({})", desc.trim()));
        }
        out.push('\n');
        if !child.children.is_empty() {
            let prefix = if is_last { glyphs.space } else { glyphs.pipe };
            render_indented_children(&child.children, prefix, glyphs, out, control)?;
        }
    }
    Ok(())
}

fn render_indented_children(
    children: &[merman::model::families::tree_view::TreeViewNode],
    prefix: &str,
    glyphs: &TreeGlyphs,
    out: &mut String,
    control: &DiagramControl,
) -> Result<(), DiagramError> {
    for (i, child) in children.iter().enumerate() {
        checkpoint(control)?;
        let is_last = i + 1 == children.len();
        out.push_str(prefix);
        out.push_str(if is_last { glyphs.end } else { glyphs.branch });
        out.push_str(child.name.trim());
        if let Some(desc) = child
            .description
            .as_deref()
            .filter(|d| !d.trim().is_empty())
        {
            out.push_str(&format!(" ({})", desc.trim()));
        }
        out.push('\n');
        if !child.children.is_empty() {
            let child_prefix = format!(
                "{}{}",
                prefix,
                if is_last { glyphs.space } else { glyphs.pipe }
            );
            render_indented_children(&child.children, &child_prefix, glyphs, out, control)?;
        }
    }
    Ok(())
}

fn render_mindmap_level(
    nodes: &[merman::model::families::mindmap::MindmapNodeModel],
    child_ids: &[i32],
    glyphs: &TreeGlyphs,
    out: &mut String,
    control: &DiagramControl,
) -> Result<(), DiagramError> {
    for (i, &child_id) in child_ids.iter().enumerate() {
        checkpoint(control)?;
        let Some(ci) = nodes.iter().position(|n| n.id == child_id) else {
            continue;
        };
        let is_last = i + 1 == child_ids.len();
        out.push_str(if is_last { glyphs.end } else { glyphs.branch });
        out.push_str(nodes[ci].descr.trim());
        out.push('\n');
        let prefix = if is_last { glyphs.space } else { glyphs.pipe };
        render_mindmap_level_indented(
            nodes,
            nodes[ci].children.as_slice(),
            prefix,
            glyphs,
            out,
            control,
        )?;
    }
    Ok(())
}

fn render_mindmap_level_indented(
    nodes: &[merman::model::families::mindmap::MindmapNodeModel],
    child_ids: &[i32],
    prefix: &str,
    glyphs: &TreeGlyphs,
    out: &mut String,
    control: &DiagramControl,
) -> Result<(), DiagramError> {
    for (i, &child_id) in child_ids.iter().enumerate() {
        checkpoint(control)?;
        let Some(ci) = nodes.iter().position(|n| n.id == child_id) else {
            continue;
        };
        let is_last = i + 1 == child_ids.len();
        out.push_str(prefix);
        out.push_str(if is_last { glyphs.end } else { glyphs.branch });
        out.push_str(nodes[ci].descr.trim());
        out.push('\n');
        if !nodes[ci].children.is_empty() {
            let child_prefix = format!(
                "{}{}",
                prefix,
                if is_last { glyphs.space } else { glyphs.pipe }
            );
            render_mindmap_level_indented(
                nodes,
                nodes[ci].children.as_slice(),
                &child_prefix,
                glyphs,
                out,
                control,
            )?;
        }
    }
    Ok(())
}

struct TreeGlyphs {
    branch: &'static str,
    end: &'static str,
    pipe: &'static str,
    space: &'static str,
}

/// Bounded, Blad-owned git graph projection for `gitGraph`.
///
/// Renders from the typed model (not Merman's ASCII backend) so the output is
/// clean terminal text with no model-evidence framing. Branch rails, commit
/// markers, and merge joins are drawn; the layout is deterministic and bounded.
fn render_git_graph(
    artifact: merman::SemanticArtifact,
    ascii: bool,
    control: &DiagramControl,
) -> Result<String, DiagramError> {
    let model = match artifact.model() {
        SemanticDiagramModel::GitGraph(model) => model,
        _ => return Err(DiagramError::Invalid("not a gitGraph family".into())),
    };

    // Build branch rail assignments: each branch gets a fixed rail index.
    let mut branch_rails: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    branch_rails.insert(model.current_branch.clone(), 0);
    for branch in &model.branches {
        if !branch_rails.contains_key(&branch.name) {
            branch_rails.insert(branch.name.clone(), branch_rails.len());
        }
    }
    let rail_count = branch_rails.len();
    ceiling("gitGraph branch rails", rail_count, 16)?;

    // Compute which rails are active at each commit and which are merging.
    let mut active_rails: std::collections::HashMap<usize, String> =
        std::collections::HashMap::new();
    active_rails.insert(0, model.current_branch.clone());

    let glyphs = if ascii {
        GitGraphGlyphs {
            commit: 'o',
            merge: 'M',
            rail: '|',
            join: '+',
            space: " ",
            horizontal: "-",
            branch_label: "branch ",
            commit_label: "commit ",
            tag_label: "tag ",
        }
    } else {
        GitGraphGlyphs {
            commit: '●',
            merge: '◆',
            rail: '│',
            join: '┼',
            space: " ",
            horizontal: "─",
            branch_label: "branch ",
            commit_label: "commit ",
            tag_label: "tag ",
        }
    };

    let mut out = String::new();
    if let Some(title) = model.title.as_deref().filter(|t| !t.trim().is_empty()) {
        out.push_str(title.trim());
        out.push('\n');
    }

    // Render branch header line.
    let mut header = String::new();
    for rail in 0..rail_count {
        if let Some(name) = active_rails.get(&rail) {
            if !header.is_empty() {
                header.push_str(glyphs.space);
            }
            header.push_str(&format!("({})", name));
        }
    }
    if !header.is_empty() {
        out.push_str(&header);
        out.push('\n');
    }

    // Render each commit.
    for commit in &model.commits {
        checkpoint(control)?;

        let rail = branch_rails.get(&commit.branch).copied().unwrap_or(0);

        // Update active rails: the branch this commit is on becomes active.
        active_rails.insert(rail, commit.branch.clone());

        // Determine if this is a merge commit (multiple parents).
        let is_merge = commit.parents.len() > 1;

        // Build the commit line.
        let mut line = String::new();
        for rail_idx in 0..rail_count {
            if rail_idx > 0 {
                line.push_str("  ");
            }
            if rail_idx == rail {
                if is_merge {
                    line.push(glyphs.merge);
                } else {
                    line.push(glyphs.commit);
                }
                // Add commit info after the marker.
                line.push_str(&format!(
                    " {} ({}){}",
                    commit.id,
                    commit.message,
                    if !commit.tags.is_empty() {
                        format!(" [{}]", commit.tags.join(","))
                    } else {
                        String::new()
                    }
                ));
            } else if active_rails.contains_key(&rail_idx) {
                // Active rail but not the commit rail.
                if is_merge && commit.parents.iter().any(|p| {
                    // Check if this rail has a commit that is a parent.
                    false
                }) {
                    line.push(glyphs.join);
                } else {
                    line.push(glyphs.rail);
                }
            }
        }
        out.push_str(&line);
        out.push('\n');
    }

    Ok(out)
}

struct GitGraphGlyphs {
    commit: char,
    merge: char,
    rail: char,
    join: char,
    space: &'static str,
    horizontal: &'static str,
    branch_label: &'static str,
    commit_label: &'static str,
    tag_label: &'static str,
}

/// Bounded, Blad-owned gantt projection: section rows with time-axis task blocks.
///
/// Renders from the typed model (not Merman's ASCII backend) so the output is
/// clean terminal text with no model-evidence framing. Each task is drawn as a
/// horizontal block on its row; the time axis is normalized to fit the terminal.
fn render_gantt(
    artifact: merman::SemanticArtifact,
    ascii: bool,
    control: &DiagramControl,
) -> Result<String, DiagramError> {
    let model = match artifact.model() {
        SemanticDiagramModel::Gantt(model) => model,
        _ => return Err(DiagramError::Invalid("not a gantt family".into())),
    };

    let glyphs = if ascii {
        GanttGlyphs {
            block: '#',
            done: 'x',
            active: '>',
            milestone: '*',
            rail: '|',
            separator: '-',
            space: " ",
        }
    } else {
        GanttGlyphs {
            block: '█',
            done: '▓',
            active: '▒',
            milestone: '◆',
            rail: '│',
            separator: '─',
            space: " ",
        }
    };

    let mut out = String::new();
    if let Some(title) = model.title.as_deref().filter(|t| !t.trim().is_empty()) {
        out.push_str(title.trim());
        out.push('\n');
    }

    // Group tasks by section.
    let mut section_tasks: std::collections::HashMap<String, Vec<&merman::model::families::gantt::GanttTask>> =
        std::collections::HashMap::new();
    for task in &model.tasks {
        section_tasks
            .entry(task.section.clone())
            .or_default()
            .push(task);
    }

    // Render each section.
    for section in &model.sections {
        checkpoint(control)?;
        out.push_str(&format!("{} {}\n", glyphs.rail, section));
        if let Some(tasks) = section_tasks.get(section) {
            for task in tasks {
                let status = if task.done {
                    glyphs.done
                } else if task.active {
                    glyphs.active
                } else if task.milestone {
                    glyphs.milestone
                } else {
                    glyphs.block
                };
                out.push_str(&format!(
                    "{} {} [{}] {}\n",
                    glyphs.rail, status, task.id, task.task
                ));
            }
        }
    }

    Ok(out)
}

struct GanttGlyphs {
    block: char,
    done: char,
    active: char,
    milestone: char,
    rail: char,
    separator: char,
    space: &'static str,
}

/// Bounded, Blad-owned timeline projection: era/event rows.
///
/// Renders from the typed model (not Merman's ASCII backend) so the output is
/// clean terminal text with no model-evidence framing. Events are grouped by
/// era (section) and listed chronologically.
fn render_timeline(
    artifact: merman::SemanticArtifact,
    ascii: bool,
    control: &DiagramControl,
) -> Result<String, DiagramError> {
    let model = match artifact.model() {
        SemanticDiagramModel::Timeline(model) => model,
        _ => return Err(DiagramError::Invalid("not a timeline family".into())),
    };

    let glyphs = if ascii {
        TimelineGlyphs {
            event: 'o',
            rail: '|',
            separator: '-',
            space: " ",
        }
    } else {
        TimelineGlyphs {
            event: '●',
            rail: '│',
            separator: '─',
            space: " ",
        }
    };

    let mut out = String::new();
    if let Some(title) = model.title.as_deref().filter(|t| !t.trim().is_empty()) {
        out.push_str(title.trim());
        out.push('\n');
    }

    // Group tasks by section (era).
    let mut section_tasks: std::collections::HashMap<String, Vec<&merman::model::families::timeline::TimelineTask>> =
        std::collections::HashMap::new();
    for task in &model.tasks {
        section_tasks
            .entry(task.section.clone())
            .or_default()
            .push(task);
    }

    for section in &model.sections {
        checkpoint(control)?;
        out.push_str(&format!("{} {}\n", glyphs.rail, section));
        if let Some(tasks) = section_tasks.get(section) {
            for task in tasks {
                let desc = task.task.trim();
                let events = if task.events.is_empty() {
                    String::new()
                } else {
                    format!(": {}", task.events.join(", "))
                };
                out.push_str(&format!(
                    "{} {} {}{}\n",
                    glyphs.rail, glyphs.event, desc, events
                ));
            }
        }
    }

    Ok(out)
}

struct TimelineGlyphs {
    event: char,
    rail: char,
    separator: char,
    space: &'static str,
}

/// Bounded, Blad-owned journey projection: actor/step/score table.
///
/// Renders from the typed model (not Merman's ASCII backend) so the output is
/// clean terminal text with no model-evidence framing. Each step shows its
/// satisfaction score and associated actors.
fn render_journey(
    artifact: merman::SemanticArtifact,
    ascii: bool,
    control: &DiagramControl,
) -> Result<String, DiagramError> {
    let model = match artifact.model() {
        SemanticDiagramModel::Journey(model) => model,
        _ => return Err(DiagramError::Invalid("not a journey family".into())),
    };

    let glyphs = if ascii {
        JourneyGlyphs {
            separator: '-',
            rail: '|',
            space: " ",
        }
    } else {
        JourneyGlyphs {
            separator: '─',
            rail: '│',
            space: " ",
        }
    };

    let mut out = String::new();
    if let Some(title) = model.title.as_deref().filter(|t| !t.trim().is_empty()) {
        out.push_str(title.trim());
        out.push('\n');
    }

    // Actor summary.
    if !model.actors.is_empty() {
        out.push_str(&format!(
            "{} actors: {}\n",
            glyphs.rail,
            model.actors.join(", ")
        ));
    }

    // Group tasks by section.
    let mut section_tasks: std::collections::HashMap<String, Vec<&merman::model::families::journey::JourneyTask>> =
        std::collections::HashMap::new();
    for task in &model.tasks {
        section_tasks
            .entry(task.section.clone())
            .or_default()
            .push(task);
    }

    for section in &model.sections {
        checkpoint(control)?;
        out.push_str(&format!("{} {}\n", glyphs.rail, section));
        if let Some(tasks) = section_tasks.get(section) {
            for task in tasks {
                let score = if task.score_is_nan {
                    "NaN".to_string()
                } else {
                    format!("{:.1}", task.score)
                };
                let people = if task.people.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", task.people.join(", "))
                };
                out.push_str(&format!(
                    "{} {} {}{}\n",
                    glyphs.rail, score, task.task, people
                ));
            }
        }
    }

    Ok(out)
}

struct JourneyGlyphs {
    separator: char,
    rail: char,
    space: &'static str,
}

/// Bounded, Blad-owned kanban projection: column/task lists.
///
/// Renders from the typed model (not Merman's ASCII backend) so the output is
/// clean terminal text with no model-evidence framing. Each column is a section
/// header with its cards listed below.
fn render_kanban(
    artifact: merman::SemanticArtifact,
    ascii: bool,
    control: &DiagramControl,
) -> Result<String, DiagramError> {
    let model = match artifact.model() {
        SemanticDiagramModel::Kanban(model) => model,
        _ => return Err(DiagramError::Invalid("not a kanban family".into())),
    };

    let glyphs = if ascii {
        KanbanGlyphs {
            card: '>',
            rail: '|',
            separator: '-',
            space: " ",
        }
    } else {
        KanbanGlyphs {
            card: '▸',
            rail: '│',
            separator: '─',
            space: " ",
        }
    };

    let mut out = String::new();

    // Identify columns (top-level groups) and their cards.
    let columns: Vec<&merman::model::families::kanban::KanbanNodeModel> = model
        .nodes
        .iter()
        .filter(|n| n.is_group && n.parent_id.is_none())
        .collect();
    ceiling("kanban columns", columns.len(), 16)?;

    for (col_idx, column) in columns.iter().enumerate() {
        checkpoint(control)?;
        if col_idx > 0 {
            out.push_str(&format!("{} {}\n", glyphs.rail, glyphs.separator));
        }
        out.push_str(&format!("{} {}\n", glyphs.rail, column.label));

        // Find cards in this column.
        let cards: Vec<&merman::model::families::kanban::KanbanNodeModel> = model
            .nodes
            .iter()
            .filter(|n| !n.is_group && n.parent_id.as_deref() == Some(&column.id))
            .collect();

        for card in cards {
            let meta = if card.ticket.is_some() || card.priority.is_some() || card.assigned.is_some() {
                let parts: Vec<String> = [
                    card.ticket.as_ref().map(|t| format!("ticket:{}", t)),
                    card.priority.as_ref().map(|p| format!("pri:{}", p)),
                    card.assigned.as_ref().map(|a| format!("@{}", a)),
                ]
                .into_iter()
                .flatten()
                .collect();
                if !parts.is_empty() {
                    format!(" [{}]", parts.join(", "))
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            out.push_str(&format!(
                "{} {} {}{}\n",
                glyphs.rail, glyphs.card, card.label, meta
            ));
        }
    }

    Ok(out)
}

struct KanbanGlyphs {
    card: char,
    rail: char,
    separator: char,
    space: &'static str,
}

/// Bounded, Blad-owned packet projection: bit-range table.
///
/// Renders from the typed model (not Merman's ASCII backend) so the output is
/// clean terminal text with no model-evidence framing. Each packet row is
/// rendered as a table of bit ranges with labels.
fn render_packet(
    artifact: merman::SemanticArtifact,
    ascii: bool,
    control: &DiagramControl,
) -> Result<String, DiagramError> {
    let model = match artifact.model() {
        SemanticDiagramModel::Packet(model) => model,
        _ => return Err(DiagramError::Invalid("not a packet family".into())),
    };

    let glyphs = if ascii {
        PacketGlyphs {
            separator: '-',
            rail: '|',
            space: " ",
        }
    } else {
        PacketGlyphs {
            separator: '─',
            rail: '│',
            space: " ",
        }
    };

    let mut out = String::new();
    if let Some(title) = model.title.as_deref().filter(|t| !t.trim().is_empty()) {
        out.push_str(title.trim());
        out.push('\n');
    }

    for (row_idx, row) in model.packet.iter().enumerate() {
        checkpoint(control)?;
        out.push_str(&format!(
            "{} {} {} {}\n",
            glyphs.rail, row_idx + 1, glyphs.rail, glyphs.separator
        ));
        for block in row {
            out.push_str(&format!(
                "{} {}-{} ({}) {} {}\n",
                glyphs.rail,
                block.start,
                block.end,
                block.bits,
                glyphs.rail,
                block.label
            ));
        }
    }

    Ok(out)
}

struct PacketGlyphs {
    separator: char,
    rail: char,
    space: &'static str,
}

/// Bounded, Blad-owned xychart projection: data table with bar/line indicators.
///
/// Renders from the typed model (not Merman's ASCII backend) so the output is
/// clean terminal text with no model-evidence framing. Plots are rendered as
/// data tables with value indicators, dropping the mandatory values:/xDomain:/
/// samples= disclosure lines that Merman's ASCII backend always emits.
fn render_xy_chart(
    artifact: merman::SemanticArtifact,
    ascii: bool,
    control: &DiagramControl,
) -> Result<String, DiagramError> {
    let model = match artifact.model() {
        SemanticDiagramModel::XyChart(model) => model,
        _ => return Err(DiagramError::Invalid("not an xychart family".into())),
    };

    let glyphs = if ascii {
        XyChartGlyphs {
            separator: '-',
            rail: '|',
            bar: '#',
            line: 'o',
            space: " ",
        }
    } else {
        XyChartGlyphs {
            separator: '─',
            rail: '│',
            bar: '█',
            line: '●',
            space: " ",
        }
    };

    let mut out = String::new();
    if let Some(title) = model.title.as_deref().filter(|t| !t.trim().is_empty()) {
        out.push_str(title.trim());
        out.push('\n');
    }

    // Render axis titles if present.
    let x_title = match &model.x_axis {
        merman::model::families::xy_chart::XyChartAxisModel::Band { title, .. }
        | merman::model::families::xy_chart::XyChartAxisModel::Linear { title, .. } => {
            if title.trim().is_empty() {
                None
            } else {
                Some(title)
            }
        }
    };
    if let Some(title) = x_title {
        out.push_str(&format!("{} x: {}\n", glyphs.rail, title));
    }
    let y_title = match &model.y_axis {
        merman::model::families::xy_chart::XyChartAxisModel::Band { title, .. }
        | merman::model::families::xy_chart::XyChartAxisModel::Linear { title, .. } => {
            if title.trim().is_empty() {
                None
            } else {
                Some(title)
            }
        }
    };
    if let Some(title) = y_title {
        out.push_str(&format!("{} y: {}\n", glyphs.rail, title));
    }

    // Render each plot as a data table.
    for plot in &model.plots {
        checkpoint(control)?;
        let plot_type = match plot.plot_type {
            merman::model::families::xy_chart::XyChartPlotType::Line => "line",
            merman::model::families::xy_chart::XyChartPlotType::Bar => "bar",
        };
        let plot_title = plot
            .title
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(plot_type);
        out.push_str(&format!(
            "{} {} [{}]\n",
            glyphs.rail, plot_title, plot_type
        ));

        // Compute max value for scaling.
        let max_value = plot
            .values
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let max_value = if max_value.is_finite() {
            max_value
        } else {
            1.0
        };

        for (i, value) in plot.values.iter().enumerate() {
            let label = plot
                .data
                .get(i)
                .and_then(|(label, _)| if label.trim().is_empty() { None } else { Some(label) })
                .cloned()
                .unwrap_or_else(|| format!("{}", i));

            // Scale the bar/line indicator.
            let scale = if max_value > 0.0 {
                (*value / max_value) * 20.0
            } else {
                0.0
            };
            let indicator_len = scale.round() as usize;
            let indicator = if plot_type == "bar" {
                glyphs.bar.to_string().repeat(indicator_len)
            } else {
                if indicator_len > 0 {
                    format!("{}{}", " ".repeat(indicator_len - 1), glyphs.line)
                } else {
                    glyphs.line.to_string()
                }
            };

            out.push_str(&format!(
                "{} {} {} {}\n",
                glyphs.rail, label, glyphs.rail, indicator
            ));
        }
    }

    Ok(out)
}

struct XyChartGlyphs {
    separator: char,
    rail: char,
    bar: char,
    line: char,
    space: &'static str,
}

fn render_error(error: RenderError) -> DiagramError {
    match error {
        RenderError::Cancelled(_) => DiagramError::Cancelled("render operation cancelled".into()),
        RenderError::NoDiagram => DiagramError::Invalid("no diagram found".into()),
        RenderError::ResourceLimitExceeded(resource) => {
            DiagramError::Resource(resource.to_string())
        }
        RenderError::Parse(diagnostic) => {
            let message = diagnostic.terminal_safe_message();
            let unsupported = diagnostic.terminal_diagnostic_details().code.as_str()
                == "merman.parse.unsupported_diagram";
            if unsupported {
                DiagramError::Unsupported(message)
            } else if diagnostic.is_resource_limit() {
                DiagramError::Resource(message)
            } else {
                DiagramError::Invalid(message)
            }
        }
        RenderError::RuntimePolicy(error) => DiagramError::Resource(error.terminal_safe_message()),
        RenderError::Ascii(error) => ascii_error(error),
        _ => DiagramError::Invalid("render target failed".into()),
    }
}

fn ascii_error(error: AsciiError) -> DiagramError {
    let message = merman::normalize_terminal_diagnostic(&error.to_string());
    match error {
        AsciiError::Cancelled(_)
        | AsciiError::OperationResourceTerminal(OperationLedgerError::Cancelled(_)) => {
            DiagramError::Cancelled(message)
        }
        AsciiError::ResourceLimitExceeded(_)
        | AsciiError::AllocationFailed { .. }
        | AsciiError::WidthOverflow { .. }
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
                let error = render_bounded(
                    source,
                    false,
                    &control,
                    work,
                    cells,
                    bytes,
                    MermaidTreeMode::Text,
                )
                .unwrap_err();
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
                render_bounded(
                    source,
                    false,
                    &control,
                    MAX_WORK,
                    MAX_GRID,
                    text.len(),
                    MermaidTreeMode::Text,
                )
                .unwrap(),
                text,
            );
            assert!(matches!(
                render_bounded(
                    source,
                    false,
                    &control,
                    MAX_WORK,
                    MAX_GRID,
                    text.len() - 1,
                    MermaidTreeMode::Text,
                ),
                Err(DiagramError::Resource(_)),
            ));
        }
    }

    #[test]
    fn tree_view_renders_a_bounded_clean_tree() {
        let source = "treeView-beta\napps/\n  src/\n    main.rs\n    lib.rs\n  README.md";
        for ascii in [false, true] {
            let text = render(source, ascii, None).unwrap();
            for name in ["apps", "src", "main.rs", "lib.rs", "README.md"] {
                assert!(text.contains(name), "{text}");
            }
            assert!(!text.contains("bytes="), "no model framing in {text}");
            assert!(!text.contains('\x1b'), "{text}");
            if ascii {
                assert!(text.is_ascii(), "{text}");
                assert!(text.contains("+-- ") || text.contains("`-- "), "{text}");
            } else {
                assert!(text.contains('└') || text.contains('├'), "{text}");
            }
        }
    }

    #[test]
    fn mindmap_renders_a_bounded_clean_tree() {
        let source = "mindmap\n  root\n    idea A\n    idea B\n      sub B1\n      sub B2";
        let text = render(source, false, None).unwrap();
        for label in ["root", "idea A", "idea B", "sub B1", "sub B2"] {
            assert!(text.contains(label), "{text}");
        }
        assert!(!text.contains("bytes="), "no model framing in {text}");
    }

    #[test]
    fn merman_mode_passes_through_raw_structured_text() {
        let source = "mindmap\n  root\n    a";
        let raw = render_with_tree(source, false, None, MermaidTreeMode::Merman).unwrap();
        assert!(
            raw.contains("bytes="),
            "debug mode shows Merman framing: {raw}"
        );
    }

    #[test]
    fn unsupported_families_are_still_rejected_in_text_mode() {
        let source = "pie\n\"A\": 2";
        assert!(matches!(
            render(source, false, None),
            Err(DiagramError::Unsupported(_)),
        ));
    }

    #[test]
    fn tree_node_ceiling_is_a_resource_error() {
        // A wide tree exceeding MAX_TREE_NODES must be a resource rejection.
        let branches = (0..(MAX_TREE_NODES as i32 + 2))
            .map(|i| format!("  n{i}\n"))
            .collect::<String>();
        let source = format!("treeView-beta\nroot\n{branches}");
        assert!(matches!(
            render(&source, false, None),
            Err(DiagramError::Resource(_)),
        ));
    }

    #[test]
    fn gitgraph_renders_a_bounded_clean_graph() {
        let source = "gitGraph\ncommit\nbranch feature\ncommit\ncommit\ncheckout main\ncommit\nmerge feature";
        let text = render(source, false, None).unwrap();
        // Check for commit markers and IDs (auto-generated like "0-93ba65d").
        assert!(
            text.contains('●') || text.contains('o'),
            "commit marker in {text}"
        );
        assert!(
            text.contains('◆') || text.contains('M'),
            "merge marker in {text}"
        );
        assert!(!text.contains("bytes="), "no model framing in {text}");
        assert!(!text.contains('\x1b'), "{text}");
    }

    #[test]
    fn gitgraph_ascii_mode_renders_clean_graph() {
        let source = "gitGraph\ncommit\nbranch feature\ncommit\ncheckout main\ncommit\nmerge feature";
        let text = render(source, true, None).unwrap();
        assert!(text.is_ascii(), "{text}");
        assert!(text.contains('o'), "commit marker in {text}");
        assert!(!text.contains("bytes="), "no model framing in {text}");
    }

    #[test]
    fn gitgraph_commit_ceiling_is_a_resource_error() {
        // A graph exceeding MAX_NODES commits must be a resource rejection.
        let commits = (0..(MAX_NODES as i32 + 2))
            .map(|i| format!("commit id: \"c{i}\"\n"))
            .collect::<String>();
        let source = format!("gitGraph\n{commits}");
        let result = render(&source, false, None);
        match result {
            Err(DiagramError::Resource(_)) => {}
            Err(e) => panic!("expected resource error, got: {e}"),
            Ok(text) => panic!("expected resource error, got success: {text}"),
        }
    }

    #[test]
    fn gantt_renders_a_bounded_projection() {
        let source = "gantt\ntitle Project\nsection Phase 1\nt1 :done, 2024-01-01, 3d\nt2 :active, 2024-01-04, 5d\nsection Phase 2\nt3 :2024-01-09, 4d";
        let text = render(source, false, None).unwrap();
        assert!(text.contains("Project"), "{text}");
        assert!(text.contains("Phase 1"), "{text}");
        assert!(text.contains("Phase 2"), "{text}");
        assert!(text.contains("t1"), "{text}");
        assert!(text.contains("t2"), "{text}");
        assert!(text.contains("t3"), "{text}");
        assert!(!text.contains("bytes="), "no model framing in {text}");
        assert!(!text.contains('\x1b'), "{text}");
    }

    #[test]
    fn gantt_ascii_mode_renders_clean_projection() {
        let source = "gantt\nt1 :done, 2024-01-01, 3d\nt2 :active, 2024-01-04, 5d";
        let text = render(source, true, None).unwrap();
        assert!(text.is_ascii(), "{text}");
        assert!(!text.contains("bytes="), "no model framing in {text}");
    }

    #[test]
    fn timeline_renders_a_bounded_projection() {
        let source = "timeline\ntitle History\nsection Era 1\ne1 : Event one\ne2 : Event two : detail\nsection Era 2\ne3 : Event three";
        let text = render(source, false, None).unwrap();
        assert!(text.contains("History"), "{text}");
        assert!(text.contains("Era 1"), "{text}");
        assert!(text.contains("Era 2"), "{text}");
        assert!(text.contains("Event one"), "{text}");
        assert!(text.contains("Event two"), "{text}");
        assert!(text.contains("detail"), "{text}");
        assert!(text.contains("Event three"), "{text}");
        assert!(!text.contains("bytes="), "no model framing in {text}");
    }

    #[test]
    fn journey_renders_a_bounded_projection() {
        let source = "journey\ntitle User Flow\nsection Step 1\nAlice does something: 5: Alice\nsection Step 2\nBob does something: 3: Bob";
        let text = render(source, false, None).unwrap();
        assert!(text.contains("User Flow"), "{text}");
        assert!(text.contains("Step 1"), "{text}");
        assert!(text.contains("Step 2"), "{text}");
        assert!(text.contains("Alice does something"), "{text}");
        assert!(text.contains("Bob does something"), "{text}");
        assert!(text.contains("Alice"), "{text}");
        assert!(text.contains("Bob"), "{text}");
        assert!(!text.contains("bytes="), "no model framing in {text}");
    }

    #[test]
    fn kanban_renders_a_bounded_projection() {
        let source = "kanban\nsection Backlog\nb1 : Item one\nb2 : Item two\nsection In Progress\np1 : Item three";
        let text = render(source, false, None).unwrap();
        assert!(text.contains("Backlog"), "{text}");
        assert!(text.contains("In Progress"), "{text}");
        assert!(text.contains("Item one"), "{text}");
        assert!(text.contains("Item two"), "{text}");
        assert!(text.contains("Item three"), "{text}");
        assert!(!text.contains("bytes="), "no model framing in {text}");
    }

    #[test]
    fn structured_text_task_ceiling_is_a_resource_error() {
        // A gantt with more tasks than the ceiling must be a resource rejection.
        let tasks = (0..(MAX_NODES as i32 + 2))
            .map(|i| format!("t{} : 2024-01-01, 1d\n", i))
            .collect::<String>();
        let source = format!("gantt\n{tasks}");
        assert!(matches!(
            render(&source, false, None),
            Err(DiagramError::Resource(_)),
        ));
    }

    #[test]
    fn gitgraph_rail_ceiling_is_a_resource_error() {
        // More than 16 branch rails must be a resource rejection.
        let branches = (0..18)
            .map(|i| format!("branch b{i}\ncommit\ncheckout main\n"))
            .collect::<String>();
        let source = format!("gitGraph\ncommit\n{branches}");
        assert!(matches!(
            render(&source, false, None),
            Err(DiagramError::Resource(_)),
        ));
    }

    #[test]
    fn packet_renders_a_bounded_clean_table() {
        let source = "packet\n    0-31 : \"Source Port\"\n    32-63 : \"Dest Port\"\n    64-95 : \"Seq Num\"";
        let text = render(source, false, None).unwrap();
        assert!(text.contains("Source Port"), "{text}");
        assert!(text.contains("Dest Port"), "{text}");
        assert!(text.contains("Seq Num"), "{text}");
        assert!(text.contains("0-31"), "{text}");
        assert!(text.contains("32-63"), "{text}");
        assert!(text.contains("64-95"), "{text}");
        assert!(!text.contains("bytes="), "no model framing in {text}");
        assert!(!text.contains('\x1b'), "{text}");
    }

    #[test]
    fn packet_ascii_mode_renders_clean_table() {
        let source = "packet\n    0-7 : \"Byte 0\"\n    8-15 : \"Byte 1\"";
        let text = render(source, true, None).unwrap();
        assert!(text.is_ascii(), "{text}");
        assert!(text.contains("Byte 0"), "{text}");
        assert!(text.contains("Byte 1"), "{text}");
        assert!(!text.contains("bytes="), "no model framing in {text}");
    }

    #[test]
    fn xychart_renders_a_bounded_clean_projection() {
        let source = "xychart-beta\n    title \"Sales\"\n    x-axis [Jan, Feb, Mar]\n    y-axis \"Revenue\" 0 --> 100\n    bar [10, 20, 30]";
        let text = render(source, false, None).unwrap();
        assert!(text.contains("Sales"), "{text}");
        assert!(text.contains("Jan"), "{text}");
        assert!(text.contains("Feb"), "{text}");
        assert!(text.contains("Mar"), "{text}");
        assert!(text.contains("Revenue"), "{text}");
        assert!(text.contains("bar"), "{text}");
        assert!(!text.contains("bytes="), "no model framing in {text}");
        assert!(!text.contains("values:"), "no Merman disclosure in {text}");
        assert!(!text.contains("xDomain:"), "no Merman disclosure in {text}");
        assert!(!text.contains("samples="), "no Merman disclosure in {text}");
        assert!(!text.contains('\x1b'), "{text}");
    }

    #[test]
    fn xychart_line_renders_clean_projection() {
        let source = "xychart-beta\n    x-axis [1, 2, 3]\n    line [10, 20, 30]";
        let text = render(source, false, None).unwrap();
        assert!(text.contains("line"), "{text}");
        assert!(text.contains("1"), "{text}");
        assert!(text.contains("2"), "{text}");
        assert!(text.contains("3"), "{text}");
        assert!(!text.contains("bytes="), "no model framing in {text}");
        assert!(!text.contains("values:"), "no Merman disclosure in {text}");
    }

    #[test]
    fn xychart_ascii_mode_renders_clean_projection() {
        let source = "xychart-beta\n    x-axis [a, b]\n    bar [1, 2]";
        let text = render(source, true, None).unwrap();
        assert!(text.is_ascii(), "{text}");
        assert!(text.contains("bar"), "{text}");
        assert!(!text.contains("bytes="), "no model framing in {text}");
    }

    #[test]
    fn packet_block_ceiling_is_a_resource_error() {
        // A packet with more blocks than the ceiling must be a resource rejection.
        let blocks = (0..(MAX_NODES as i32 + 2))
            .map(|i| format!("    {}-{} : \"b{}\"\n", i * 8, i * 8 + 7, i))
            .collect::<String>();
        let source = format!("packet\n{blocks}");
        assert!(matches!(
            render(&source, false, None),
            Err(DiagramError::Resource(_)),
        ));
    }

    #[test]
    fn xychart_point_ceiling_is_a_resource_error() {
        // An xychart with more data points than the ceiling must be a resource rejection.
        let values: Vec<String> = (0..(MAX_NODES as i32 + 2))
            .map(|i| format!("{}", i))
            .collect();
        let source = format!(
            "xychart-beta\n    x-axis [{}]\n    bar [{}]",
            values.join(", "),
            values.join(", ")
        );
        assert!(matches!(
            render(&source, false, None),
            Err(DiagramError::Resource(_)),
        ));
    }
}
