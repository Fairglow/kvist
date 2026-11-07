//! Prompt-local observation state; file access still belongs to the executor.

use std::collections::BTreeMap;

use sav::{ToolIntent, loop_detection::ProgressEvidence};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::file_tools::{MAX_OUTPUT_BYTES, canonical_path, validate_digest};
use crate::sandbox::ToolOutcome;

const MAX_CURSORS: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    content: String,
    #[serde(deserialize_with = "nullable_digest")]
    sha256: Option<String>,
    page_sha256: String,
    offset: usize,
    #[serde(deserialize_with = "nullable_offset")]
    next_offset: Option<usize>,
    total_bytes: usize,
    #[serde(default, deserialize_with = "optional_number")]
    requested_offset: Option<usize>,
    #[serde(default, deserialize_with = "optional_text")]
    offset_adjustment: Option<String>,
}

fn optional_number<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<usize>, D::Error> {
    usize::deserialize(deserializer).map(Some)
}

fn optional_text<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

fn nullable_offset<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<usize>, D::Error> {
    Option::<usize>::deserialize(deserializer)
}

fn nullable_digest<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

fn page_digest(content: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(content.as_bytes())))
}

struct Cursor {
    offset: usize,
    sha256: Option<String>,
    page_offset: usize,
    page_bytes: usize,
    page_sha256: String,
    total_bytes: usize,
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
        json!({"content":"ab","sha256":null,"page_sha256":page_digest("ab"),
            "offset":0,"next_offset":2,"total_bytes":5})
    }

    #[test]
    fn page_local_digests_do_not_invent_a_full_revision_or_compare_different_pages() {
        use sha2::{Digest, Sha256};
        let digest =
            |content: &str| format!("sha256:{}", hex::encode(Sha256::digest(content.as_bytes())));
        let mut reads = ReadCursors::default();
        let first = intent(json!({"path":"/workspace/a","offset":0,"limit":2}));
        let local = |content: &str, offset: usize| {
            json!({
                "content":content,"sha256":null,"page_sha256":digest(content),
                "offset":offset,"next_offset":offset + 2,"total_bytes":1_u64 << 40
            })
        };
        assert_eq!(
            reads.observe(&first, &outcome(local("ab", 0))).unwrap(),
            ProgressEvidence::None
        );
        let next = reads
            .resolve(&intent(json!({"path":"/workspace/a","limit":2})))
            .unwrap();
        assert_eq!(next.arguments["offset"], 2);
        assert!(next.arguments.get("expected_sha256").is_none());
        assert_eq!(
            reads.observe(&next, &outcome(local("cd", 2))).unwrap(),
            ProgressEvidence::None
        );
        assert_eq!(
            reads.observe(&next, &outcome(local("xy", 2))).unwrap(),
            ProgressEvidence::StateChanged
        );
        let mut corrupt = local("xy", 2);
        corrupt["page_sha256"] = json!(digest("different"));
        assert!(reads.observe(&next, &outcome(corrupt)).is_err());
    }

    #[test]
    fn live_cursor_does_not_freeze_and_accepts_large_changed_adjusted_pages() {
        let mut reads = ReadCursors::default();
        let read = intent(json!({"path":"/workspace/a","limit":2}));
        let mut large = page();
        large["total_bytes"] = json!(3 * 1024 * 1024);
        reads.observe(&read, &outcome(large)).unwrap();
        let resolved = reads.resolve(&read).unwrap();
        assert_eq!(resolved.arguments["offset"], 2);
        assert!(resolved.arguments.get("expected_sha256").is_none());
        let changed = json!({
            "content":"", "sha256":null,"page_sha256":page_digest(""),
            "offset":1, "requested_offset":2, "offset_adjustment":"past_eof",
            "next_offset":null,"total_bytes":1
        });
        assert_eq!(
            reads.observe(&resolved, &outcome(changed)).unwrap(),
            ProgressEvidence::StateChanged
        );
        assert_eq!(reads.resolve(&read).unwrap().arguments["offset"], 1);
        let guarded = intent(
            json!({"path":"/workspace/a","expected_sha256":format!("sha256:{}", "c".repeat(64))}),
        );
        assert_eq!(
            reads.resolve(&guarded).unwrap().arguments["expected_sha256"],
            guarded.arguments["expected_sha256"]
        );
    }

    #[test]
    fn live_adjustment_metadata_must_describe_actual_requested_offsets() {
        let mut reads = ReadCursors::default();
        let read = intent(json!({"path":"/workspace/a","offset":2,"limit":2}));
        let valid = json!({
            "content":"z","sha256":null,"page_sha256":page_digest("z"),
            "offset":3,"requested_offset":2,"offset_adjustment":"utf8_boundary",
            "next_offset":null,"total_bytes":4
        });
        reads.observe(&read, &outcome(valid.clone())).unwrap();
        for (field, value) in [
            ("requested_offset", json!(1)),
            ("requested_offset", json!(null)),
            ("offset_adjustment", json!("past_eof")),
            ("offset_adjustment", json!("unknown")),
            ("offset", json!(7)),
            ("total_bytes", json!(2)),
        ] {
            let mut bad = valid.clone();
            bad[field] = value;
            assert!(reads.observe(&read, &outcome(bad)).is_err(), "{field}");
        }
    }

    #[test]
    fn failures_and_malformed_results_do_not_advance_the_cursor() {
        let mut reads = ReadCursors::default();
        let read = intent(json!({"path":"/workspace/a","limit":2}));
        reads.observe(&read, &outcome(page())).unwrap();
        let resolved = reads.resolve(&read).unwrap();
        assert_eq!(resolved.arguments["offset"], 2);
        reads.observe(&resolved, &ToolOutcome::rejected()).unwrap();
        for field in [
            "offset",
            "next_offset",
            "sha256",
            "page_sha256",
            "total_bytes",
            "content",
        ] {
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
            ("page_sha256", json!("invalid")),
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
    fn explicit_guards_are_preserved_and_offsets_override_continuation() {
        let mut reads = ReadCursors::default();
        let read = intent(json!({"path":"/workspace/a","limit":2}));
        reads.observe(&read, &outcome(page())).unwrap();
        let conflict = intent(
            json!({"path":"/workspace/a","expected_sha256":format!("sha256:{}", "b".repeat(64))}),
        );
        let resolved = reads.resolve(&conflict).unwrap();
        assert_eq!(
            resolved.arguments["expected_sha256"],
            conflict.arguments["expected_sha256"]
        );
        assert_eq!(resolved.arguments["offset"], 2);
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

    #[test]
    fn only_a_changed_prior_full_digest_is_state_change_not_a_first_read_or_new_page() {
        let mut reads = ReadCursors::default();
        let read =
            intent(json!({"path":"/workspace/a","offset":0,"limit":2,"include_sha256":true}));
        let mut full_page = page();
        full_page["sha256"] = json!(format!("sha256:{}", "a".repeat(64)));
        assert_eq!(
            reads.observe(&read, &outcome(full_page.clone())).unwrap(),
            ProgressEvidence::None
        );
        let next =
            intent(json!({"path":"/workspace/a","offset":2,"limit":2,"include_sha256":true}));
        let mut next_page = full_page.clone();
        next_page["offset"] = json!(2);
        next_page["content"] = json!("cd");
        next_page["page_sha256"] = json!(page_digest("cd"));
        next_page["next_offset"] = json!(4);
        assert_eq!(
            reads.observe(&next, &outcome(next_page)).unwrap(),
            ProgressEvidence::None
        );
        let mut changed = full_page;
        changed["sha256"] = json!(format!("sha256:{}", "b".repeat(64)));
        assert_eq!(
            reads.observe(&read, &outcome(changed.clone())).unwrap(),
            ProgressEvidence::StateChanged
        );
        assert_eq!(
            reads.observe(&read, &outcome(changed)).unwrap(),
            ProgressEvidence::None
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
            arguments.insert("offset".into(), json!(cursor.offset));
        }
        Ok(resolved)
    }

    pub(super) fn observe(
        &mut self,
        intent: &ToolIntent,
        outcome: &ToolOutcome,
    ) -> Result<ProgressEvidence> {
        if intent.name != "read_file" || outcome.failed() {
            return Ok(ProgressEvidence::None);
        }
        if outcome.stdout.len() > MAX_OUTPUT_BYTES {
            return Err(invalid("native read result exceeds the output byte bound"));
        }
        let page: Page = serde_json::from_slice(&outcome.stdout).map_err(|_| {
            invalid("invalid native read result; expected complete JSON page metadata")
        })?;
        if let Some(digest) = &page.sha256 {
            validate_digest(digest)?;
        }
        validate_digest(&page.page_sha256)?;
        let full_requested = intent.arguments.get("include_sha256") == Some(&json!(true))
            || intent.arguments.get("expected_sha256").is_some();
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
        let valid_offset = match (page.requested_offset, page.offset_adjustment.as_deref()) {
            (None, None) => page.offset as u64 == requested_offset,
            (Some(requested), Some("past_eof")) => {
                requested as u64 == requested_offset
                    && requested > page.total_bytes
                    && page.offset == page.total_bytes
            }
            (Some(requested), Some("utf8_boundary")) => {
                requested as u64 == requested_offset
                    && page.offset > requested
                    && page.offset - requested <= 3
            }
            _ => false,
        };
        if !valid_offset
            || page.sha256.is_some() != full_requested
            || page.page_sha256 != page_digest(&page.content)
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
        let progress = if self
            .0
            .get(path)
            .is_some_and(|cursor| {
                cursor.total_bytes != page.total_bytes
                    || matches!((&cursor.sha256, &page.sha256), (Some(before), Some(after)) if before != after)
                    || (cursor.page_offset == page.offset && cursor.page_bytes == page.content.len()
                        && cursor.page_sha256 != page.page_sha256)
            })
        {
            ProgressEvidence::StateChanged
        } else {
            ProgressEvidence::None
        };
        self.0.insert(
            path.into(),
            Cursor {
                offset: end,
                sha256: page.sha256,
                page_offset: page.offset,
                page_bytes: page.content.len(),
                page_sha256: page.page_sha256,
                total_bytes: page.total_bytes,
            },
        );
        Ok(progress)
    }
}
