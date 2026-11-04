//! Canonical Markdown presentation, independent of terminal capabilities.

use serde_json::Value;

/// Makes terminal controls inert without discarding report text.
pub fn sanitize(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\n' | '\t' => output.push(ch),
            '\r' => {}
            ch if ch.is_control() => {
                use std::fmt::Write;
                let _ = write!(output, "\\u{{{:04x}}}", ch as u32);
            }
            _ => output.push(ch),
        }
    }
    output
}

/// Escapes an untrusted label or table cell as literal Markdown text.
pub fn literal(text: &str) -> String {
    let text = sanitize(text);
    let mut result = String::new();
    for ch in text.chars() {
        match ch {
            '\n' => result.push_str("<br>"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '#' | '!' => {
                result.push('\\');
                result.push(ch);
            }
            _ => result.push(ch),
        }
    }
    result
}

/// A collision-free fenced block for tool output or executable source.
pub fn fence(text: &str, language: &str) -> String {
    let text = sanitize(text);
    let length = text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(3);
    let delimiter = "`".repeat(length);
    format!("{delimiter}{language}\n{text}\n{delimiter}")
}

/// Projects a report into Markdown, decoding nested JSON and report strings.
/// Unknown fields are retained; report prose remains authored Markdown.
pub fn report(text: &str) -> String {
    report_redacted(text, &[])
}

pub(crate) fn redact(text: &str, values: &[String]) -> String {
    let mut redactor = crate::agent::redaction::StreamRedactor::new(values);
    let mut result = redactor.absorb(text);
    result.push_str(&redactor.finish());
    result
}

pub(crate) fn report_redacted(text: &str, values: &[String]) -> String {
    match serde_json::from_str::<Value>(text) {
        Ok(mut value @ (Value::Object(_) | Value::Array(_))) => {
            redact_value(&mut value, values, 0);
            value_markdown(&value, 2)
        }
        _ => sanitize(&redact(text, values)),
    }
}

fn redact_value(value: &mut Value, values: &[String], depth: usize) {
    match value {
        Value::String(text) => {
            if depth <= 16
                && let Ok(mut nested @ (Value::Object(_) | Value::Array(_))) =
                    serde_json::from_str::<Value>(text)
            {
                redact_value(&mut nested, values, depth + 1);
                *text = nested.to_string();
            } else {
                *text = redact(text, values);
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                redact_value(value, values, depth + 1);
            }
        }
        Value::Array(items) => {
            for value in items {
                redact_value(value, values, depth + 1);
            }
        }
        _ => {}
    }
}

fn label(key: &str) -> String {
    literal(&key.replace('_', " "))
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => literal(text),
        Value::Null => "—".to_owned(),
        _ => literal(&value.to_string()),
    }
}

fn value_markdown(value: &Value, depth: usize) -> String {
    if depth > 12 {
        return fence(&value.to_string(), "json");
    }
    match value {
        Value::Object(fields) => {
            let mut output = String::new();
            for (key, value) in fields {
                match value {
                    Value::String(text)
                        if matches!(
                            key.as_str(),
                            "report"
                                | "markdown"
                                | "message"
                                | "content"
                                | "text"
                                | "output"
                                | "log_content"
                                | "replay"
                        ) =>
                    {
                        output.push_str(&format!(
                            "{} {}\n\n{}\n\n",
                            "#".repeat(depth.min(6)),
                            label(key),
                            report(text)
                        ));
                    }
                    Value::Object(_) | Value::Array(_) => {
                        output.push_str(&format!(
                            "{} {}\n\n{}\n\n",
                            "#".repeat(depth.min(6)),
                            label(key),
                            value_markdown(value, depth + 1)
                        ));
                    }
                    _ => output.push_str(&format!("- **{}:** {}\n", label(key), scalar(value))),
                }
            }
            output
        }
        Value::Array(items) => {
            if items.is_empty() {
                return "_None._".to_owned();
            }
            let table_keys = items.first().and_then(Value::as_object).filter(|first| {
                !first.is_empty()
                    && items.iter().all(|item| {
                        item.as_object().is_some_and(|fields| {
                            fields.keys().eq(first.keys())
                                && fields.values().all(|v| {
                                    !v.is_object()
                                        && !v.is_array()
                                        && v.as_str().is_none_or(|s| !s.contains('\n'))
                                })
                        })
                    })
            });
            if let Some(keys) = table_keys {
                let keys: Vec<_> = keys.keys().collect();
                let mut output = format!(
                    "| {} |\n| {} |\n",
                    keys.iter()
                        .map(|k| label(k))
                        .collect::<Vec<_>>()
                        .join(" | "),
                    vec!["---"; keys.len()].join(" | ")
                );
                for item in items {
                    output.push_str(&format!(
                        "| {} |\n",
                        keys.iter()
                            .map(|key| scalar(&item[*key]))
                            .collect::<Vec<_>>()
                            .join(" | ")
                    ));
                }
                output
            } else {
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        format!(
                            "{} Item {}\n\n{}\n\n",
                            "#".repeat(depth.min(6)),
                            index + 1,
                            value_markdown(item, depth + 1)
                        )
                    })
                    .collect()
            }
        }
        _ => scalar(value),
    }
}

/// Wraps a command result as a standalone Markdown document.
pub fn document(title: &str, body: &str) -> String {
    format!("# {}\n\n{}", literal(title), report(body).trim_end())
}

/// Projects the remaining legacy human reports without changing native formats.
pub(crate) fn legacy_report(text: &str) -> String {
    let boxed = text.starts_with('╭');
    let mut output = String::new();
    let mut fence: Option<(char, usize)> = None;
    for line in text.lines() {
        let line = if boxed {
            if line.starts_with('╭') {
                let title = line
                    .trim_start_matches(['╭', '─', ' '])
                    .trim_end_matches(['─', ' ']);
                output.push_str(&format!("## {}\n\n", literal(title)));
                continue;
            }
            if line.starts_with('╰') {
                continue;
            }
            line.trim_start_matches('│').trim()
        } else {
            line
        };
        if !output.is_empty() && !output.ends_with('\n') {
            output.push_str("  \n");
        }
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().filter(|ch| matches!(ch, '`' | '~'));
        let count = marker.map_or(0, |marker| {
            trimmed.chars().take_while(|ch| *ch == marker).count()
        });
        if let Some((marker, length)) = fence {
            output.push_str(&sanitize(line));
            output.push('\n');
            if count >= length && trimmed.starts_with(marker) && trimmed[count..].trim().is_empty()
            {
                fence = None;
            }
            continue;
        }
        if count >= 3 {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&sanitize(line));
            output.push('\n');
            fence = marker.map(|marker| (marker, count));
            continue;
        }
        if line.chars().all(|ch| ch == '─' || ch.is_whitespace()) {
            output.push('\n');
        } else if let Some(item) = line.strip_prefix("• ") {
            output.push_str(&format!("- {}\n", literal(item)));
        } else if boxed && line.ends_with(':') {
            output.push_str(&format!(
                "\n### {}\n\n",
                literal(line.trim_end_matches(':'))
            ));
        } else if let Some((key, value)) = line.split_once(':')
            && !key.is_empty()
            && key.len() < 60
            && key
                .chars()
                .all(|ch| ch.is_ascii_alphabetic() || matches!(ch, ' ' | '-' | '_'))
        {
            output.push_str(&format!(
                "**{}:** {}  \n",
                literal(key.trim()),
                sanitize(value.trim())
            ));
        } else {
            output.push_str(&sanitize(line));
            if boxed {
                output.push_str("  \n");
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_json_is_readable_and_metadata_is_retained() {
        let output = report(
            r##"{"report":"# Findings\n\n| A | B |\n|---|---|\n| x | y |","needs_decision":true,"other":{"tokens":42}}"##,
        );
        assert!(output.contains("# Findings\n\n| A | B |"));
        assert!(output.contains("**needs decision:** true"));
        assert!(output.contains("**tokens:** 42"));
        assert!(!output.contains("\\n"));
    }

    #[test]
    fn fences_and_cells_cannot_be_broken_by_untrusted_text() {
        assert!(fence("```\n# no escape", "text").starts_with("````text\n"));
        assert_eq!(literal("a|b\n# x"), "a\\|b<br>\\# x");
        assert!(!report("\x1b]52;c;secret\x07").contains('\x1b'));
    }

    #[test]
    fn legacy_projection_preserves_authored_fences_verbatim() {
        let source = "Result:\n\n```yaml\nkey: value\n  nested: true\n```\n\nDone.";
        let output = legacy_report(source);
        assert!(
            output.contains("```yaml\nkey: value\n  nested: true\n```\n"),
            "{output}"
        );
    }

    #[test]
    fn legacy_prose_metadata_and_bullets_have_separate_lines() {
        let output = legacy_report("Ready\nregistry dependencies: 365\nNext\n• choice");
        assert!(
            output.contains("Ready  \n**registry dependencies:** 365"),
            "{output}"
        );
        assert!(output.contains("Next  \n- choice"), "{output}");
    }
}
