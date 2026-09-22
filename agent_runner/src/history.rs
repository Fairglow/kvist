//! Durable session history for the interactive UI.
//!
//! Every session writes a human-readable transcript under the log directory
//! (`.agent-runner/runs` by default). This module reads that directory to build a
//! list of past sessions — enough to choose one — and loads a single transcript
//! for a read-only replay in the UI.
//!
//! Everything here treats the filesystem as untrusted input: directory listing
//! failures degrade to an empty list, non-matching or malformed files are
//! skipped, and each file read is size-bounded. Nothing reads secrets.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

/// Skip transcript files larger than this when building the history list, so a
/// very long session cannot exhaust memory while merely listing sessions.
const MAX_LISTED_BYTES: usize = 5 * 1024 * 1024;

/// One past session, enough to identify it and judge whether to replay it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEntry {
    /// The session identifier shown in the transcript header.
    pub id: String,
    /// Path to the transcript file to replay.
    pub path: PathBuf,
    /// Number of turns reached, when the finish line reported it.
    pub turns: Option<usize>,
    /// Total tokens observed, when the finish line reported it.
    pub tokens: Option<u64>,
    /// Final outcome (`true` ok, `false` cancelled/failed), when known.
    pub success: Option<bool>,
}

impl SessionEntry {
    /// A short human-readable summary for the history list.
    pub fn describe(&self) -> String {
        let turns = self
            .turns
            .map(|t| format!("{t} turn{}", if t == 1 { "" } else { "s" }))
            .unwrap_or_else(|| "??".to_owned());
        let tokens = self
            .tokens
            .map(|tok| format!("{tok} tok"))
            .unwrap_or_else(|| "??".to_owned());
        let status = match self.success {
            Some(true) => "ok",
            Some(false) => "cancelled/failed",
            None => "unknown",
        };
        format!("{turns}, {tokens}  ·  {status}")
    }
}

/// Parsed finish-metadata from a transcript's `== session … finished: … ==`
/// marker: the session id, turn count, token count, and final outcome.
type FinishMeta = (String, Option<usize>, Option<u64>, Option<bool>);

/// Parses the terminal `== session … finished: … ==` marker line into its parts.
///
/// The transcript writes the marker as
/// `== session {id} finished: {n} turns, {t} tokens, {s}s, {status} ==`, so the
/// id sits before ` finished: ` and the metrics follow it.
fn parse_finish(line: &str) -> Option<FinishMeta> {
    let inner = line.strip_suffix(" ==")?.strip_prefix("== session ")?;
    let (id, rest) = inner.split_once(" finished: ")?;
    if id.is_empty() {
        return None;
    }
    // Scan the metric tokens; each metric is "<number> <label>" separated by
    // commas/spaces, so the number immediately before a known label wins.
    let tokens: Vec<&str> = rest
        .split([',', ' '])
        .filter(|part| !part.is_empty())
        .collect();
    let mut turns = None;
    let mut total = None;
    let mut previous: Option<&str> = None;
    for part in &tokens {
        match *part {
            "turns" | "turn" => {
                if let Some(num) = previous.and_then(strip_number) {
                    turns = Some(num);
                }
            }
            "tokens" | "token" => {
                if let Some(num) = previous.and_then(strip_number) {
                    total = Some(num as u64);
                }
            }
            _ => {}
        }
        previous = Some(*part);
    }
    let success = match tokens
        .last()
        .map(|part| part.trim())
        .filter(|s| !s.is_empty())
    {
        Some("ok") => Some(true),
        Some(_) => Some(false),
        None => None,
    };
    Some((id.to_owned(), turns, total, success))
}

/// Strips a trailing non-numeric suffix so `1,` becomes `1` but `abc` yields
/// `None`.
fn strip_number(part: &str) -> Option<usize> {
    let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<usize>().ok()
}

/// Lists past sessions in the given log directory, newest first.
///
/// A listing failure (missing directory, permission error) yields an empty list
/// rather than an error, so the history view never blocks the UI.
pub fn list_sessions(log_dir: &Path) -> Vec<SessionEntry> {
    let Ok(read_dir) = fs::read_dir(log_dir) else {
        return Vec::new();
    };
    let mut sessions: Vec<SessionEntry> = read_dir
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy();
            if !(name.starts_with("session-") && name.ends_with(".log")) {
                return None;
            }
            let (id, turns, tokens, success) = parse_transcript_meta(&path)?;
            Some(SessionEntry {
                id,
                path,
                turns,
                tokens,
                success,
            })
        })
        .collect();
    // Sort newest first; the filename embeds a fixed-width microsecond stamp, so
    // a reverse string sort matches reverse chronological order.
    sessions.sort_by(|a, b| {
        b.path
            .file_name()
            .unwrap_or_default()
            .cmp(a.path.file_name().unwrap_or_default())
    });
    sessions
}

/// Reads the durable metadata (id + finish metrics) from a transcript file.
///
/// Every line is examined uniformly, so a single-line transcript or a finish
/// marker on the first line is not lost to a separate initial read.
fn parse_transcript_meta(path: &Path) -> Option<FinishMeta> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut finish: Option<FinishMeta> = None;
    let mut first_id: Option<String> = None;
    // Read lines until EOF or an unreadable (invalid-UTF8) line; either way the
    // scan ends gracefully instead of aborting the whole file.
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        // `read_line` retains the trailing newline; `BufRead::lines` does not, so
        // drop it here to keep the parsers' `strip_suffix` matching intact.
        line.truncate(line.trim_end_matches('\n').len());
        // The start marker carries the id too; keep it as a fallback when the
        // run never wrote a finish line (an interrupted session).
        if first_id.is_none()
            && let Some(id) = extract_id(&line)
        {
            first_id = Some(id);
        }
        if let Some(parsed) = parse_finish(&line) {
            finish = Some(parsed);
        }
    }
    finish.or_else(|| first_id.map(|id| (id, None, None, None)))
}

/// Extracts the session id from a `== session {id} started … ==` marker.
fn extract_id(line: &str) -> Option<String> {
    let inner = line.strip_suffix(" ==")?.strip_prefix("== session ")?;
    let (id, _) = inner.split_once(" started ")?;
    (!id.is_empty()).then(|| id.to_owned())
}

/// Loads the full transcript lines for a replay, size-bounded and UTF-8 lossy.
pub fn transcript_lines(path: &Path) -> io::Result<Vec<String>> {
    let mut file = File::open(path)?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)?;
    // Bound the replay so an unusually large transcript cannot exhaust memory.
    if buffer.len() > MAX_LISTED_BYTES {
        buffer.truncate(MAX_LISTED_BYTES);
    }
    Ok(buffer
        .split('\n')
        .map(|line| line.trim_end().to_owned())
        .filter(|line| !line.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_transcript(dir: &Path, name: &str, body: &str) {
        let mut file = File::create(dir.join(name)).expect("create transcript");
        file.write_all(body.as_bytes()).expect("write transcript");
    }

    #[test]
    fn parse_finish_extracts_id_turns_tokens_and_status() {
        let line = "== session abc-123 finished: 3 turns, 1234 tokens, 5s, ok ==";
        let (id, turns, tokens, success) = parse_finish(line).expect("parses finish");
        assert_eq!(id, "abc-123");
        assert_eq!(turns, Some(3));
        assert_eq!(tokens, Some(1234));
        assert_eq!(success, Some(true));
    }

    #[test]
    fn parse_finish_reports_failure_status_without_metrics() {
        let line = "== session xyz finished: cancelled/failed ==";
        let (id, turns, tokens, success) = parse_finish(line).expect("parses finish");
        assert_eq!(id, "xyz");
        assert_eq!(turns, None);
        assert_eq!(tokens, None);
        assert_eq!(success, Some(false));
    }

    #[test]
    fn parse_finish_rejects_non_markers() {
        assert!(parse_finish("not a marker line").is_none());
        assert!(parse_finish("== session only started ==").is_none());
    }

    #[test]
    fn list_sessions_orders_newest_first_and_skips_non_transcripts() {
        let dir = tempfile::tempdir().unwrap();
        // Timestamps are fixed-width microsecond stamps, so these sort cleanly.
        write_transcript(
            dir.path(),
            "session-1000000000.log",
            "== session old finished: 1 turns, 10 tokens, 1s, ok ==",
        );
        write_transcript(
            dir.path(),
            "session-3000000000.log",
            "== session new finished: 9 turns, 900 tokens, 9s, cancelled/failed ==",
        );
        // Non-transcript files must be ignored.
        write_transcript(dir.path(), "notes.txt", "ignore me");
        std::fs::create_dir_all(dir.path().join("session-2000000000")).unwrap();

        let sessions = list_sessions(dir.path());
        assert_eq!(sessions.len(), 2, "only transcript files are listed");
        assert_eq!(sessions[0].id, "new");
        assert_eq!(sessions[0].turns, Some(9));
        assert_eq!(sessions[0].success, Some(false));
        assert_eq!(sessions[1].id, "old");
        assert_eq!(sessions[1].success, Some(true));
    }

    #[test]
    fn list_sessions_is_empty_when_directory_missing() {
        let sessions = list_sessions(Path::new("/definitely/not/here-42"));
        assert!(sessions.is_empty());
    }

    #[test]
    fn transcript_lines_returns_trimmed_non_empty_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-1.log");
        write_transcript(
            dir.path(),
            "session-1.log",
            "== session s1 started 0s ago ==\n\n[turn 1] reasoning: why\n\n== session s1 finished: 1 turns, 2 tokens, 1s, ok ==\n",
        );
        let lines = transcript_lines(&path).expect("read transcript");
        assert_eq!(
            lines,
            vec![
                "== session s1 started 0s ago ==".to_owned(),
                "[turn 1] reasoning: why".to_owned(),
                "== session s1 finished: 1 turns, 2 tokens, 1s, ok ==".to_owned(),
            ]
        );
    }
}
