//! Validation for Kvist's layered `SPEC.md` format.
//!
//! The validator reads Markdown without rewriting it. Markdown outside the
//! required template marker, disclosure layers, and required headings remains
//! user-authored content and is preserved by callers.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    KvistError, Result, artifacts::SPECIFICATION_VERSION, file_io::write_new_file_atomically,
};

/// Maximum supported size of a specification read from disk.
pub const MAX_SPECIFICATION_BYTES: u64 = 1024 * 1024;

/// Deterministic template for a newly created component specification.
pub const COMPONENT_SPEC_TEMPLATE: &str = r#"<!-- kvist-specification-version: 1 -->
# Component Specification

<details open>
<summary>Layer 1: Executive summary and public contract</summary>

## Purpose

[Describe this component's purpose and why it exists.]

## Public contract

[Describe callers, inputs, outputs, and observable behavior.]

</details>

<details>
<summary>Layer 2: Architectural guarantees</summary>

## Constraints and invariants

[Describe performance bounds, concurrency invariants, memory limits, dependency
policy, and compatibility commitments.]

</details>

<details>
<summary>Layer 3: Detailed strategy and algorithms</summary>

## Design and failure paths

[Describe algorithms, state transitions, validation, error handling, and edge
cases.]

</details>
"#;

/// A required progressive-disclosure layer in `SPEC.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SpecificationLayer {
    /// Purpose and observable public contract.
    ExecutiveSummary,
    /// Architectural constraints and invariants.
    ArchitecturalGuarantees,
    /// Algorithms, state transitions, and failure paths.
    DetailedStrategy,
}

impl SpecificationLayer {
    const fn summary(self) -> &'static str {
        match self {
            Self::ExecutiveSummary => "Layer 1: Executive summary and public contract",
            Self::ArchitecturalGuarantees => "Layer 2: Architectural guarantees",
            Self::DetailedStrategy => "Layer 3: Detailed strategy and algorithms",
        }
    }

    const fn opening_tag(self) -> &'static str {
        match self {
            Self::ExecutiveSummary => "<details open>",
            Self::ArchitecturalGuarantees | Self::DetailedStrategy => "<details>",
        }
    }
}

const REQUIRED_LAYERS: [SpecificationLayer; 3] = [
    SpecificationLayer::ExecutiveSummary,
    SpecificationLayer::ArchitecturalGuarantees,
    SpecificationLayer::DetailedStrategy,
];

/// A required Markdown heading within a disclosure layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SpecificationSection {
    /// Layer 1 purpose heading.
    Purpose,
    /// Layer 1 public-contract heading.
    PublicContract,
    /// Layer 2 constraints and invariants heading.
    ConstraintsAndInvariants,
    /// Layer 3 design and failure-paths heading.
    DesignAndFailurePaths,
}

impl SpecificationSection {
    const fn heading(self) -> &'static str {
        match self {
            Self::Purpose => "## Purpose",
            Self::PublicContract => "## Public contract",
            Self::ConstraintsAndInvariants => "## Constraints and invariants",
            Self::DesignAndFailurePaths => "## Design and failure paths",
        }
    }
}

fn required_sections(layer: SpecificationLayer) -> &'static [SpecificationSection] {
    match layer {
        SpecificationLayer::ExecutiveSummary => &[
            SpecificationSection::Purpose,
            SpecificationSection::PublicContract,
        ],
        SpecificationLayer::ArchitecturalGuarantees => {
            &[SpecificationSection::ConstraintsAndInvariants]
        }
        SpecificationLayer::DetailedStrategy => &[SpecificationSection::DesignAndFailurePaths],
    }
}

/// The class of a specification validation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecificationDiagnosticKind {
    /// The version marker is not the first line.
    MissingTemplateVersion,
    /// The version marker is not a positive integer.
    InvalidTemplateVersion,
    /// The version marker is recognized but unsupported.
    UnsupportedTemplateVersion {
        /// Version found in the document.
        found: u32,
        /// Version required by this binary.
        supported: u32,
    },
    /// A required disclosure layer is absent.
    MissingLayer {
        /// Required layer that was not found.
        layer: SpecificationLayer,
    },
    /// A disclosure layer appears more than once.
    DuplicateLayer {
        /// Duplicated layer.
        layer: SpecificationLayer,
    },
    /// A layer's opening tag or summary is malformed.
    InvalidLayerSyntax {
        /// Layer with invalid syntax.
        layer: SpecificationLayer,
    },
    /// A layer has no matching closing `</details>` tag.
    UnclosedLayer {
        /// Unclosed layer.
        layer: SpecificationLayer,
    },
    /// Required layers do not appear in progressive-disclosure order.
    InvalidLayerOrder {
        /// Layer appearing out of order.
        layer: SpecificationLayer,
    },
    /// A required section heading is absent.
    MissingSection {
        /// Layer that requires the section.
        layer: SpecificationLayer,
        /// Required section that was not found.
        section: SpecificationSection,
    },
    /// A required section heading appears more than once.
    DuplicateSection {
        /// Layer containing the duplicate heading.
        layer: SpecificationLayer,
        /// Duplicated heading.
        section: SpecificationSection,
    },
    /// Required section headings do not appear in order.
    InvalidSectionOrder {
        /// Layer containing the out-of-order heading.
        layer: SpecificationLayer,
        /// Out-of-order heading.
        section: SpecificationSection,
    },
    /// A required section has no non-whitespace content.
    EmptySection {
        /// Layer containing the empty section.
        layer: SpecificationLayer,
        /// Empty required section.
        section: SpecificationSection,
    },
}

/// A structured, one-based source location diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecificationDiagnostic {
    /// Classification of the validation failure.
    pub kind: SpecificationDiagnosticKind,
    /// One-based line number; `line_count + 1` identifies end of file.
    pub line: usize,
    /// One-based column number.
    pub column: usize,
}

/// Validation result for a `SPEC.md` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecificationValidation {
    /// Version declared by the source document, when it is parseable.
    pub template_version: Option<u32>,
    /// All detected diagnostics in deterministic source order.
    pub diagnostics: Vec<SpecificationDiagnostic>,
}

impl SpecificationValidation {
    /// Returns true when the document has no diagnostics.
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// A successfully generated component specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSpecification {
    /// New `SPEC.md` path.
    pub path: PathBuf,
}

/// Creates a validated `SPEC.md` in a component directory without overwriting.
pub fn create(component_dir: &Path) -> Result<GeneratedSpecification> {
    ensure_component_directory(component_dir)?;
    let path = component_dir.join("SPEC.md");
    match fs::symlink_metadata(&path) {
        Ok(_) => return Err(KvistError::SpecificationAlreadyExists { path }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect specification destination",
                path,
                source,
            });
        }
    }

    let validation = validate(COMPONENT_SPEC_TEMPLATE);
    if !validation.is_valid() {
        return Err(KvistError::GeneratedSpecificationInvalid {
            diagnostics: format_diagnostics(&validation.diagnostics),
        });
    }
    write_new_file_atomically(&path, COMPONENT_SPEC_TEMPLATE)?;

    Ok(GeneratedSpecification { path })
}

/// Validates a UTF-8 `SPEC.md` document without modifying its contents.
pub fn validate(contents: &str) -> SpecificationValidation {
    let lines = contents.lines().collect::<Vec<_>>();
    let eof_line = lines.len() + 1;
    let mut diagnostics = Vec::new();
    let template_version = validate_template_version(&lines, &mut diagnostics);
    let mut layer_bounds = Vec::new();

    for layer in REQUIRED_LAYERS {
        if let Some(bounds) = find_layer(&lines, layer, eof_line, &mut diagnostics) {
            layer_bounds.push(bounds);
        }
    }
    validate_layer_order(&layer_bounds, &mut diagnostics);
    for bounds in &layer_bounds {
        validate_sections(&lines, *bounds, eof_line, &mut diagnostics);
    }

    diagnostics.sort_by_key(|diagnostic| (diagnostic.line, diagnostic.column));
    SpecificationValidation {
        template_version,
        diagnostics,
    }
}

/// Reads and validates a UTF-8 `SPEC.md` file.
pub fn validate_file(path: &Path) -> Result<SpecificationValidation> {
    let metadata = fs::symlink_metadata(path).map_err(|source| KvistError::Io {
        operation: "inspect specification",
        path: path.to_path_buf(),
        source,
    })?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(KvistError::SpecificationIsSymlink {
            path: path.to_path_buf(),
        });
    }
    if !file_type.is_file() {
        return Err(KvistError::SpecificationNotFile {
            path: path.to_path_buf(),
        });
    }
    if metadata.len() > MAX_SPECIFICATION_BYTES {
        return Err(KvistError::SpecificationTooLarge {
            path: path.to_path_buf(),
            max_bytes: MAX_SPECIFICATION_BYTES,
        });
    }

    let contents = fs::read_to_string(path).map_err(|source| KvistError::Io {
        operation: "read specification",
        path: path.to_path_buf(),
        source,
    })?;
    Ok(validate(&contents))
}

/// Renders structured diagnostics in a stable human-readable form.
pub fn format_diagnostics(diagnostics: &[SpecificationDiagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| {
            format!(
                "{}:{}: {}",
                diagnostic.line,
                diagnostic.column,
                diagnostic_message(&diagnostic.kind)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, Copy)]
struct LayerBounds {
    layer: SpecificationLayer,
    summary_line: usize,
    end_line: usize,
}

fn validate_template_version(
    lines: &[&str],
    diagnostics: &mut Vec<SpecificationDiagnostic>,
) -> Option<u32> {
    let Some(first_line) = lines.first() else {
        diagnostics.push(diagnostic(
            SpecificationDiagnosticKind::MissingTemplateVersion,
            1,
        ));
        return None;
    };
    let Some(version_text) = first_line
        .strip_prefix("<!-- kvist-specification-version: ")
        .and_then(|value| value.strip_suffix(" -->"))
    else {
        diagnostics.push(diagnostic(
            SpecificationDiagnosticKind::MissingTemplateVersion,
            1,
        ));
        return None;
    };
    let Ok(version) = version_text.parse::<u32>() else {
        diagnostics.push(diagnostic(
            SpecificationDiagnosticKind::InvalidTemplateVersion,
            1,
        ));
        return None;
    };
    if version == 0 {
        diagnostics.push(diagnostic(
            SpecificationDiagnosticKind::InvalidTemplateVersion,
            1,
        ));
        return None;
    }
    if version != SPECIFICATION_VERSION {
        diagnostics.push(diagnostic(
            SpecificationDiagnosticKind::UnsupportedTemplateVersion {
                found: version,
                supported: SPECIFICATION_VERSION,
            },
            1,
        ));
    }

    Some(version)
}

fn find_layer(
    lines: &[&str],
    layer: SpecificationLayer,
    eof_line: usize,
    diagnostics: &mut Vec<SpecificationDiagnostic>,
) -> Option<LayerBounds> {
    let summary = format!("<summary>{}</summary>", layer.summary());
    let matches = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (*line == summary).then_some(index))
        .collect::<Vec<_>>();
    let Some(summary_index) = matches.first().copied() else {
        diagnostics.push(diagnostic(
            SpecificationDiagnosticKind::MissingLayer { layer },
            eof_line,
        ));
        return None;
    };
    for duplicate_index in matches.iter().skip(1) {
        diagnostics.push(diagnostic(
            SpecificationDiagnosticKind::DuplicateLayer { layer },
            duplicate_index + 1,
        ));
    }

    let opening_index = summary_index.checked_sub(1);
    if opening_index
        .and_then(|index| lines.get(index))
        .is_none_or(|line| *line != layer.opening_tag())
    {
        diagnostics.push(diagnostic(
            SpecificationDiagnosticKind::InvalidLayerSyntax { layer },
            summary_index + 1,
        ));
    }
    let end_index = lines
        .iter()
        .enumerate()
        .skip(summary_index + 1)
        .find_map(|(index, line)| (*line == "</details>").then_some(index));
    let Some(end_index) = end_index else {
        diagnostics.push(diagnostic(
            SpecificationDiagnosticKind::UnclosedLayer { layer },
            summary_index + 1,
        ));
        return Some(LayerBounds {
            layer,
            summary_line: summary_index + 1,
            end_line: eof_line,
        });
    };

    Some(LayerBounds {
        layer,
        summary_line: summary_index + 1,
        end_line: end_index + 1,
    })
}

fn validate_layer_order(
    layer_bounds: &[LayerBounds],
    diagnostics: &mut Vec<SpecificationDiagnostic>,
) {
    let mut previous_line = 0;
    for layer in REQUIRED_LAYERS {
        if let Some(bounds) = layer_bounds.iter().find(|bounds| bounds.layer == layer) {
            if bounds.summary_line < previous_line {
                diagnostics.push(diagnostic(
                    SpecificationDiagnosticKind::InvalidLayerOrder { layer },
                    bounds.summary_line,
                ));
            }
            previous_line = bounds.summary_line;
        }
    }
}

fn validate_sections(
    lines: &[&str],
    bounds: LayerBounds,
    eof_line: usize,
    diagnostics: &mut Vec<SpecificationDiagnostic>,
) {
    let section_lines = required_sections(bounds.layer)
        .iter()
        .copied()
        .filter_map(|section| {
            let heading = section.heading();
            let matches = lines
                .iter()
                .enumerate()
                .skip(bounds.summary_line)
                .take(bounds.end_line.saturating_sub(bounds.summary_line + 1))
                .filter_map(|(index, line)| (*line == heading).then_some(index))
                .collect::<Vec<_>>();
            let Some(first_index) = matches.first().copied() else {
                diagnostics.push(diagnostic(
                    SpecificationDiagnosticKind::MissingSection {
                        layer: bounds.layer,
                        section,
                    },
                    bounds.end_line.min(eof_line),
                ));
                return None;
            };
            for duplicate_index in matches.iter().skip(1) {
                diagnostics.push(diagnostic(
                    SpecificationDiagnosticKind::DuplicateSection {
                        layer: bounds.layer,
                        section,
                    },
                    duplicate_index + 1,
                ));
            }
            Some((section, first_index))
        })
        .collect::<Vec<_>>();

    let mut previous_index = 0;
    for (section, index) in &section_lines {
        if *index < previous_index {
            diagnostics.push(diagnostic(
                SpecificationDiagnosticKind::InvalidSectionOrder {
                    layer: bounds.layer,
                    section: *section,
                },
                index + 1,
            ));
        }
        previous_index = *index;
    }

    for (section, heading_index) in &section_lines {
        let closing_index = bounds.end_line - 1;
        let next_heading_index = lines
            .iter()
            .enumerate()
            .skip(heading_index + 1)
            .take(closing_index.saturating_sub(heading_index + 1))
            .find_map(|(index, line)| line.starts_with("## ").then_some(index))
            .unwrap_or(closing_index);
        let has_content = lines[heading_index + 1..next_heading_index]
            .iter()
            .any(|line| !line.trim().is_empty());
        if !has_content {
            diagnostics.push(diagnostic(
                SpecificationDiagnosticKind::EmptySection {
                    layer: bounds.layer,
                    section: *section,
                },
                heading_index + 1,
            ));
        }
    }
}

fn diagnostic(kind: SpecificationDiagnosticKind, line: usize) -> SpecificationDiagnostic {
    SpecificationDiagnostic {
        kind,
        line,
        column: 1,
    }
}

fn ensure_component_directory(component_dir: &Path) -> Result<()> {
    match fs::symlink_metadata(component_dir) {
        Ok(metadata) => validate_component_directory(component_dir, &metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(component_dir).map_err(|source| KvistError::Io {
                operation: "create component directory",
                path: component_dir.to_path_buf(),
                source,
            })?;
            let metadata =
                fs::symlink_metadata(component_dir).map_err(|source| KvistError::Io {
                    operation: "inspect component directory",
                    path: component_dir.to_path_buf(),
                    source,
                })?;
            validate_component_directory(component_dir, &metadata)
        }
        Err(source) => Err(KvistError::Io {
            operation: "inspect component directory",
            path: component_dir.to_path_buf(),
            source,
        }),
    }
}

fn validate_component_directory(component_dir: &Path, metadata: &fs::Metadata) -> Result<()> {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(KvistError::ComponentDirectoryIsSymlink {
            path: component_dir.to_path_buf(),
        });
    }
    if !file_type.is_dir() {
        return Err(KvistError::ComponentDirectoryNotDirectory {
            path: component_dir.to_path_buf(),
        });
    }

    Ok(())
}

fn diagnostic_message(kind: &SpecificationDiagnosticKind) -> String {
    match kind {
        SpecificationDiagnosticKind::MissingTemplateVersion => {
            "expected template version marker on line 1".to_owned()
        }
        SpecificationDiagnosticKind::InvalidTemplateVersion => {
            "template version must be a positive integer".to_owned()
        }
        SpecificationDiagnosticKind::UnsupportedTemplateVersion { found, supported } => {
            format!("template version {found} is unsupported; expected {supported}")
        }
        SpecificationDiagnosticKind::MissingLayer { layer } => {
            format!("missing {} layer", layer_name(*layer))
        }
        SpecificationDiagnosticKind::DuplicateLayer { layer } => {
            format!("duplicate {} layer", layer_name(*layer))
        }
        SpecificationDiagnosticKind::InvalidLayerSyntax { layer } => {
            format!("invalid {} layer syntax", layer_name(*layer))
        }
        SpecificationDiagnosticKind::UnclosedLayer { layer } => {
            format!("unclosed {} layer", layer_name(*layer))
        }
        SpecificationDiagnosticKind::InvalidLayerOrder { layer } => {
            format!("{} layer is out of order", layer_name(*layer))
        }
        SpecificationDiagnosticKind::MissingSection { layer, section } => {
            format!(
                "missing {} in {} layer",
                section.heading(),
                layer_name(*layer)
            )
        }
        SpecificationDiagnosticKind::DuplicateSection { layer, section } => {
            format!(
                "duplicate {} in {} layer",
                section.heading(),
                layer_name(*layer)
            )
        }
        SpecificationDiagnosticKind::InvalidSectionOrder { layer, section } => {
            format!(
                "{} is out of order in {} layer",
                section.heading(),
                layer_name(*layer)
            )
        }
        SpecificationDiagnosticKind::EmptySection { layer, section } => {
            format!(
                "{} has no content in {} layer",
                section.heading(),
                layer_name(*layer)
            )
        }
    }
}

const fn layer_name(layer: SpecificationLayer) -> &'static str {
    match layer {
        SpecificationLayer::ExecutiveSummary => "executive summary",
        SpecificationLayer::ArchitecturalGuarantees => "architectural guarantees",
        SpecificationLayer::DetailedStrategy => "detailed strategy",
    }
}
