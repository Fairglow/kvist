//! Bounded, local Markdown locator resolution for task planning and briefs.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// Summarize only the selected prerequisite's newest attempt, never its raw logs.
pub(crate) fn retained_verification(
    journal: &str,
    task_id: &str,
) -> Result<Option<Value>, serde_json::Error> {
    let mut latest = None;
    for line in journal
        .lines()
        .rev()
        .filter(|line| !line.is_empty())
        .take(1024)
    {
        let event: Value = serde_json::from_str(line)?;
        let Some(attempt) = event["attempt_id"].as_str().filter(|id| {
            id.strip_prefix("attempt-").is_some_and(|number| {
                number.len() == 4 && number.bytes().all(|b| b.is_ascii_digit())
            })
        }) else {
            continue;
        };
        if event["task_id"] != task_id || event["schema_version"] != 1 {
            continue;
        }
        if latest.get_or_insert_with(|| attempt.to_owned()).as_str() != attempt {
            break;
        }
        if event["phase"] == "verification-finished" {
            if event["result"] != "success" {
                break;
            }
            return Ok(Some(json!({
                "attempt_id":attempt,
                "observed": match event["expected_test_result"].as_str() {
                    Some("red") => "approved regression failure",
                    Some("green") => "approved regression success",
                    _ => "approved verification success",
                },
                "note":"Historical observation, not a check of current source or human acceptance."
            })));
        }
    }
    Ok(None)
}

pub(crate) struct LocatorIndex<'a> {
    sources: BTreeMap<&'a str, BTreeMap<String, Option<usize>>>,
}

impl<'a> LocatorIndex<'a> {
    pub(crate) fn new(documents: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let sources = documents
            .into_iter()
            .map(|(name, contents)| {
                let mut headings = BTreeMap::new();
                let mut clauses = BTreeMap::new();
                for (index, line) in crate::markdown_structure::scan(contents)
                    .into_iter()
                    .enumerate()
                {
                    if let Some(level) = line.heading {
                        let heading = line.text.trim_start()[level..]
                            .trim()
                            .trim_end_matches('#')
                            .trim();
                        insert(&mut headings, slug(heading), index + 1);
                    } else if !line.code && !line.comment {
                        let text = line
                            .text
                            .trim()
                            .trim_start_matches("- ")
                            .trim_start_matches('*');
                        if let Some((id, _)) = text.split_once(':') {
                            let id = id.trim().trim_matches(['*', '`']);
                            if id.contains('-')
                                && id.bytes().all(|b| {
                                    b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-'
                                })
                            {
                                insert(&mut clauses, slug(id), index + 1);
                            }
                        }
                    }
                }
                for (id, line) in clauses {
                    headings.entry(id).or_insert(line);
                }
                (name, headings)
            })
            .collect();
        Self { sources }
    }

    pub(crate) fn resolve(&self, reference: &str) -> Option<usize> {
        let (document, locator) = reference.split_once('#')?;
        let locator = slug(locator);
        if locator.is_empty()
            || reference.contains(['\n', '\r'])
            || reference.matches('#').count() != 1
        {
            return None;
        }
        self.sources.get(document)?.get(&locator).copied().flatten()
    }
}

fn insert(index: &mut BTreeMap<String, Option<usize>>, id: String, line: usize) {
    index
        .entry(id)
        .and_modify(|line| *line = None)
        .or_insert(Some(line));
}

fn slug(text: &str) -> String {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_real_headings_and_labels_but_not_ambiguous_or_inert_text() {
        let index = LocatorIndex::new([(
            "REQUIREMENTS.md",
            "## Acceptance and traceability\n\nREQ-TEST: Visible.\n\n```\n## Example\n```\n\n### Duplicate\n### Duplicate\n",
        )]);
        assert_eq!(
            index.resolve("REQUIREMENTS.md#Acceptance-and-traceability"),
            Some(1)
        );
        assert_eq!(index.resolve("REQUIREMENTS.md#REQ-TEST"), Some(3));
        for invalid in [
            "REQUIREMENTS.md#Example",
            "REQUIREMENTS.md#Duplicate",
            "../REQUIREMENTS.md#REQ-TEST",
        ] {
            assert_eq!(index.resolve(invalid), None);
        }
    }

    #[test]
    fn prerequisite_projection_uses_only_latest_exact_attempt_and_omits_raw_logs() {
        let observed = json!({
            "schema_version":1, "attempt_id":"attempt-0001", "task_id":"local-tests",
            "phase":"verification-finished", "result":"success", "expected_test_result":"red",
            "stdout":"raw output"
        })
        .to_string();
        let shown = retained_verification(&observed, "local-tests")
            .unwrap()
            .unwrap();
        assert_eq!(shown["observed"], "approved regression failure");
        assert!(shown.get("stdout").is_none());
        assert!(
            retained_verification(&observed, "other-task")
                .unwrap()
                .is_none()
        );
        let later = json!({"schema_version":1, "attempt_id":"attempt-0002",
            "task_id":"local-tests", "phase":"prepared"})
        .to_string();
        assert!(
            retained_verification(&format!("{observed}\n{later}\n"), "local-tests")
                .unwrap()
                .is_none()
        );
    }
}
