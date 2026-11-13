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
        ) {
            return unsupported(
                "expected flowchart, sequence, state, class, ER, mindmap or treeView syntax",
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
    fn non_tree_structured_text_is_rejected_in_text_mode() {
        let source = "gantt\ntitle P\nt1 : 2024-01-01, 3d";
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
}
