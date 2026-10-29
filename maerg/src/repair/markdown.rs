//! Approved structural salvage preserves prose instead of guessing its meaning.

use crate::component_documents::{self, DocumentKind};

use super::queue::Candidate;

pub(super) fn candidate(path: &str, contents: &str) -> std::result::Result<Candidate, String> {
    let template = super::template(path).map_err(|error| error.to_string())?;
    let marker = template
        .lines()
        .next()
        .ok_or("native template has no marker")?;
    let marker_name = marker
        .trim_start_matches("<!--")
        .split_once(':')
        .map(|(name, _)| name.trim())
        .ok_or("invalid native marker")?;
    let first = contents.lines().next().unwrap_or_default();
    let declaration = contents
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    if let Some((name, version)) = declaration
        .trim()
        .trim_start_matches('\u{feff}')
        .trim()
        .trim_start_matches("<!--")
        .split_once(':')
        && name.trim().eq_ignore_ascii_case(marker_name)
        && version
            .trim()
            .trim_end_matches("-->")
            .trim()
            .parse::<u32>()
            .is_ok_and(|version| version != 1)
    {
        return Err(
            "known unsupported Markdown version requires an explicit reset, not relabeling"
                .to_owned(),
        );
    }
    let mut reasons = Vec::new();
    let mut recovered = contents.to_owned();
    if first != marker {
        recovered = format!("{marker}\n{contents}");
        reasons.push("prepend native version-one marker while retaining original material; assume current structural format, not recovered stakeholder intent".to_owned());
    }
    let kind = [
        DocumentKind::Requirements,
        DocumentKind::Contract,
        DocumentKind::Design,
    ]
    .into_iter()
    .find(|kind| {
        std::path::Path::new(path)
            .file_name()
            .is_some_and(|name| name == kind.filename())
    });
    if let Some(kind) = kind {
        if !component_documents::validate(kind, &recovered).is_valid() {
            if recovered.lines().any(|line| {
                line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~")
            }) {
                return Err("required-section repair with fenced content is ambiguous; use an approved reset rather than guessing at code/prose boundaries".to_owned());
            }
            recovered = sections(kind, &recovered, &mut reasons);
        }
        let validation = component_documents::validate(kind, &recovered);
        if !validation.is_valid() {
            return Err(component_documents::format_diagnostics(
                &validation.diagnostics,
            ));
        }
    } else {
        let heading = template
            .lines()
            .find(|line| line.starts_with("# "))
            .ok_or("native template has no document heading")?;
        if !recovered.lines().any(|line| line == heading) {
            recovered = format!(
                "{marker}\n{heading}\n\n{}",
                recovered
                    .strip_prefix(marker)
                    .unwrap_or(&recovered)
                    .trim_start_matches('\n')
            );
            reasons.push(format!("add required native heading `{heading}`; preserve original prose without asserting implementation evidence"));
        }
    }
    if recovered.len() > component_documents::MAX_COMPONENT_DOCUMENT_BYTES as usize {
        return Err(
            "structural candidate exceeds the native 1 MiB bound; use an approved reset".to_owned(),
        );
    }
    reasons.push("format/section recovery is a draft, not acceptance or an independent compliance observation; original bytes remain in backup".to_owned());
    Ok(Candidate {
        contents: recovered,
        reasons,
    })
}

fn sections(kind: DocumentKind, contents: &str, reasons: &mut Vec<String>) -> String {
    let headings = kind.required_sections();
    let mut bodies = vec![String::new(); headings.len()];
    let mut seen = vec![false; headings.len()];
    let mut extra = String::new();
    let mut current = None;
    for line in contents.lines() {
        if let Some(index) = headings
            .iter()
            .position(|heading| line.trim().eq_ignore_ascii_case(heading))
        {
            if seen[index] {
                reasons.push(format!(
                    "merge duplicate `{}` without discarding its paragraphs",
                    headings[index]
                ));
            } else if line != headings[index] {
                reasons.push(format!("canonicalize heading to `{}`", headings[index]));
            }
            seen[index] = true;
            current = Some(index);
        } else {
            if line.starts_with("## ") {
                current = None;
            }
            let destination = current
                .map(|index| &mut bodies[index])
                .unwrap_or(&mut extra);
            destination.push_str(line);
            destination.push('\n');
        }
    }
    let mut output = extra;
    for (index, heading) in headings.iter().enumerate() {
        output.push_str(&format!("\n{heading}\n\n"));
        let mut lines = vec![*heading];
        lines.extend(bodies[index].lines());
        if !component_documents::section_has_content(&lines, 0) {
            output.push_str("Recovery draft: this required section was missing or empty. Review retained material before accepting.\n\n");
            reasons.push(format!("add explicitly labeled recovery draft content to `{heading}`; no stakeholder intent is inferred"));
        }
        output.push_str(&bodies[index]);
    }
    reasons.push("put required sections in native order; preserve unrecognized sections and all non-heading prose".to_owned());
    output
}
