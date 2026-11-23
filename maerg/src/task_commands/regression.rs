//! Explicitly approved Rust regression evidence, separate from process success.

use std::collections::BTreeMap;

pub(super) fn matches_result(stdout: &str, expected: &[String], red: bool) -> bool {
    let mut cases = BTreeMap::new();
    let mut failed_cases = 0_usize;
    let mut failed_reported = 0_usize;
    let mut summaries = 0;
    for line in stdout.lines() {
        if let Some(summary) = line.strip_prefix("test result: ") {
            let Some((result, counts)) = summary.split_once(". ") else {
                return false;
            };
            let Some(count) = counts
                .split(';')
                .nth(1)
                .map(str::trim)
                .and_then(|part| part.strip_suffix(" failed"))
                .and_then(|count| count.parse::<usize>().ok())
            else {
                return false;
            };
            if !matches!((result, count), ("ok", 0) | ("FAILED", 1..)) {
                return false;
            }
            failed_reported = failed_reported.saturating_add(count);
            summaries += 1;
        }
        let Some((name, result)) = line
            .strip_prefix("test ")
            .and_then(|s| s.rsplit_once(" ... "))
        else {
            continue;
        };
        failed_cases += usize::from(result == "FAILED");
        if expected.iter().any(|case| case == name) && cases.insert(name, result).is_some() {
            return false;
        }
    }
    let wanted = if red { "FAILED" } else { "ok" };
    summaries > 0
        && failed_cases == failed_reported
        && failed_cases == if red { expected.len() } else { 0 }
        && expected
            .iter()
            .all(|name| cases.get(name.as_str()) == Some(&wanted))
}

#[cfg(test)]
mod tests;
