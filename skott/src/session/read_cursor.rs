//! Prompt-local observation state; file access still belongs to the executor.

use std::collections::BTreeMap;

use sav::ToolIntent;
use serde::Deserialize;
use serde_json::json;

use crate::error::{Error, Result};
use crate::file_tools::{MAX_FILE_BYTES, MAX_OUTPUT_BYTES, canonical_path, validate_digest};
use crate::sandbox::ToolOutcome;

const MAX_CURSORS: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    content: String,
    sha256: String,
    offset: usize,
    #[serde(deserialize_with = "nullable_offset")]
    next_offset: Option<usize>,
    total_bytes: usize,
}

fn nullable_offset<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<usize>, D::Error> {
    Option::<usize>::deserialize(deserializer)
}

struct Cursor {
    offset: usize,
    sha256: String,
    eof: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(arguments: serde_json::Value) -> ToolIntent {
        ToolIntent {
            id: "read".into(),
            provider_id: None,
            name: "read_file".into(),
            arguments,
        }
    }

    fn outcome(page: serde_json::Value) -> ToolOutcome {
        ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: serde_json::to_vec(&page).unwrap(),
            stderr: vec![],
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        }
    }

    fn page() -> serde_json::Value {
        json!({"content":"ab","sha256":format!("sha256:{}", "a".repeat(64)),
            "offset":0,"next_offset":2,"total_bytes":5})
    }

    #[test]
    fn failures_and_malformed_results_do_not_advance_the_cursor() {
        let mut reads = ReadCursors::default();
        let read = intent(json!({"path":"/workspace/a","limit":2}));
        reads.observe(&read, &outcome(page())).unwrap();
        let resolved = reads.resolve(&read).unwrap();
        assert_eq!(resolved.arguments["offset"], 2);
        reads.observe(&resolved, &ToolOutcome::rejected()).unwrap();
        for field in ["offset", "next_offset", "sha256", "total_bytes", "content"] {
            let mut malformed = page();
            malformed.as_object_mut().unwrap().remove(field);
            assert!(
                reads.observe(&resolved, &outcome(malformed)).is_err(),
                "{field}"
            );
        }
        for (field, value) in [
            ("offset", json!(1)),
            ("next_offset", json!(4)),
            ("total_bytes", json!(1)),
            ("content", json!("too long")),
            ("sha256", json!("invalid")),
        ] {
            let mut malformed = page();
            malformed[field] = value;
            assert!(
                reads.observe(&resolved, &outcome(malformed)).is_err(),
                "{field}"
            );
        }
        assert_eq!(reads.resolve(&read).unwrap().arguments["offset"], 2);
    }

    #[test]
    fn digest_conflicts_fail_and_explicit_offsets_override_continuation() {
        let mut reads = ReadCursors::default();
        let read = intent(json!({"path":"/workspace/a","limit":2}));
        reads.observe(&read, &outcome(page())).unwrap();
        let conflict = intent(
            json!({"path":"/workspace/a","expected_sha256":format!("sha256:{}", "b".repeat(64))}),
        );
        assert!(
            reads
                .resolve(&conflict)
                .unwrap_err()
                .to_string()
                .contains("offset: 0")
        );
        let reset = intent(json!({"path":"/workspace/a","offset":0}));
        assert_eq!(reads.resolve(&reset).unwrap(), reset);
        let changed_limit = intent(json!({"path":"/workspace/a","limit":1}));
        assert_eq!(
            reads.resolve(&changed_limit).unwrap().arguments["offset"],
            2
        );
    }

    #[test]
    fn distinct_path_state_is_bounded_before_dispatch() {
        let mut reads = ReadCursors::default();
        for index in 0..MAX_CURSORS {
            reads
                .observe(
                    &intent(json!({"path":format!("/workspace/{index}")})),
                    &outcome(page()),
                )
                .unwrap();
        }
        assert!(
            reads
                .resolve(&intent(json!({"path":"/workspace/another"})))
                .is_err()
        );
        assert!(
            reads
                .resolve(&intent(json!({"path":"/workspace/0"})))
                .is_ok()
        );
    }
}

#[derive(Default)]
pub(super) struct ReadCursors(BTreeMap<String, Cursor>);

fn invalid(reason: &str) -> Error {
    Error::ToolRender {
        tool: "read_file".into(),
        reason: reason.into(),
    }
}

impl ReadCursors {
    pub(super) fn repeatable(&self, intent: &ToolIntent) -> bool {
        let Some(path) = intent.arguments.get("path").and_then(|v| v.as_str()) else {
            return false;
        };
        let Some(cursor) = self.0.get(path) else {
            return true;
        };
        if !cursor.eof {
            return true;
        }
        // Explicit earlier offsets deliberately request fresh observations.
        intent.arguments.get("offset").is_some_and(|offset| {
            offset
                .as_u64()
                .is_some_and(|offset| offset < cursor.offset as u64)
        })
    }

    pub(super) fn resolve(&self, intent: &ToolIntent) -> Result<ToolIntent> {
        if intent.name != "read_file" {
            return Ok(intent.clone());
        }
        let mut resolved = intent.clone();
        let arguments = resolved
            .arguments
            .as_object_mut()
            .ok_or_else(|| invalid("arguments must be an object"))?;
        let path = arguments
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid("path must be a string"))?;
        canonical_path(path)?;
        if !self.0.contains_key(path) && self.0.len() >= MAX_CURSORS {
            return Err(invalid(
                "prompt read cursor bound (4096 distinct paths) exceeded",
            ));
        }
        if !arguments.contains_key("offset")
            && let Some(cursor) = self.0.get(path)
        {
            if arguments
                .get("expected_sha256")
                .is_some_and(|value| value != &json!(cursor.sha256))
            {
                return Err(invalid(
                    "continuation digest conflicts with the last page; restart with explicit offset: 0",
                ));
            }
            arguments.insert("offset".into(), json!(cursor.offset));
            arguments.insert("expected_sha256".into(), json!(cursor.sha256));
        }
        Ok(resolved)
    }

    pub(super) fn observe(&mut self, intent: &ToolIntent, outcome: &ToolOutcome) -> Result<()> {
        if intent.name != "read_file" || outcome.failed() {
            return Ok(());
        }
        if outcome.stdout.len() > MAX_OUTPUT_BYTES {
            return Err(invalid("native read result exceeds the output byte bound"));
        }
        let page: Page = serde_json::from_slice(&outcome.stdout).map_err(|_| {
            invalid("invalid native read result; expected complete JSON page metadata")
        })?;
        validate_digest(&page.sha256)?;
        let requested_offset = intent
            .arguments
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let requested_limit = intent
            .arguments
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(4096);
        let end = page
            .offset
            .checked_add(page.content.len())
            .ok_or_else(|| invalid("native read result offset overflow"))?;
        if page.total_bytes > MAX_FILE_BYTES
            || page.offset as u64 != requested_offset
            || page.content.len() as u64 > requested_limit
            || end > page.total_bytes
            || (end < page.total_bytes && (end == page.offset || page.next_offset != Some(end)))
            || (end == page.total_bytes && page.next_offset.is_some())
            || intent
                .arguments
                .get("expected_sha256")
                .is_some_and(|value| value != &json!(page.sha256))
        {
            return Err(invalid(
                "inconsistent native read page boundaries or digest",
            ));
        }
        let path = intent
            .arguments
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid("read result has no request path"))?;
        self.0.insert(
            path.into(),
            Cursor {
                offset: end,
                sha256: page.sha256,
                eof: page.next_offset.is_none(),
            },
        );
        Ok(())
    }
}
