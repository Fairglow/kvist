//! Strict, path-free unified hunks. All matching is exact; no fuzz or fallback.

use super::{MAX_FILE_BYTES, failure};
use crate::error::Result;

struct Range {
    start: usize,
    count: usize,
}

impl Range {
    fn parse(value: &str, prefix: char) -> Result<Self> {
        let value = value.strip_prefix(prefix).ok_or_else(invalid)?;
        let (start, count) = value.split_once(',').unwrap_or((value, "1"));
        let number = |value: &str| {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            value.parse::<usize>().map_err(|_| invalid())
        };
        let start = number(start)?;
        let count = number(count)?;
        if start > MAX_FILE_BYTES || count > MAX_FILE_BYTES || (start == 0 && count != 0) {
            return Err(invalid());
        }
        Ok(Self { start, count })
    }

    fn index(&self) -> usize {
        if self.count == 0 {
            self.start
        } else {
            self.start - 1
        }
    }
}

fn invalid() -> crate::error::Error {
    failure(
        "invalid unified patch; use exact ordered @@ -old[,count] +new[,count] @@ hunks without file headers",
    )
}

fn header(line: &str) -> Result<(Range, Range)> {
    let line = line.strip_suffix('\n').ok_or_else(invalid)?;
    let rest = line.strip_prefix("@@ ").ok_or_else(invalid)?;
    let (ranges, section) = rest.split_once(" @@").ok_or_else(invalid)?;
    if !section.is_empty() && !section.starts_with(' ') {
        return Err(invalid());
    }
    let (old, new) = ranges.split_once(' ').ok_or_else(invalid)?;
    Ok((Range::parse(old, '-')?, Range::parse(new, '+')?))
}

fn push(output: &mut String, text: &str) -> Result<()> {
    if !output.is_empty() && !output.ends_with('\n') {
        return Err(failure(
            "patch leaves content after a missing-final-newline line",
        ));
    }
    if output.len().saturating_add(text.len()) > MAX_FILE_BYTES {
        return Err(failure("patched result exceeds the file byte bound"));
    }
    output.push_str(text);
    Ok(())
}

pub(super) fn apply(original: &str, patch: &str) -> Result<String> {
    if patch.is_empty() || !patch.ends_with('\n') {
        return Err(invalid());
    }
    let source: Vec<&str> = original.split_inclusive('\n').collect();
    let lines: Vec<&str> = patch.split_inclusive('\n').collect();
    let mut output = String::new();
    let mut source_index = 0;
    let mut output_lines = 0;
    let mut index = 0;
    let mut changed = false;
    while index < lines.len() {
        let (old, new) = header(lines[index])?;
        index += 1;
        if old.index() < source_index || old.index() > source.len() {
            return Err(failure(
                "patch hunks overlap, are out of order or exceed the preimage",
            ));
        }
        for line in &source[source_index..old.index()] {
            push(&mut output, line)?;
            output_lines += 1;
        }
        source_index = old.index();
        if new.index() != output_lines {
            return Err(failure(
                "patch new coordinates do not match the resulting file",
            ));
        }
        let mut removed = 0;
        let mut added = 0;
        while index < lines.len() && !lines[index].starts_with("@@ ") {
            let line = lines[index];
            let prefix = line.as_bytes()[0];
            if !matches!(prefix, b' ' | b'-' | b'+') {
                return Err(invalid());
            }
            let mut content = &line[1..];
            index += 1;
            if lines.get(index) == Some(&"\\ No newline at end of file\n") {
                content = content.strip_suffix('\n').ok_or_else(invalid)?;
                if content.is_empty() {
                    return Err(invalid());
                }
                index += 1;
            }
            if matches!(prefix, b' ' | b'-') {
                if source.get(source_index) != Some(&content) {
                    return Err(failure(
                        "patch context/removal does not match the exact preimage",
                    ));
                }
                source_index += 1;
                removed += 1;
            }
            if matches!(prefix, b' ' | b'+') {
                push(&mut output, content)?;
                output_lines += 1;
                added += 1;
            }
            changed |= prefix != b' ';
            if removed > old.count || added > new.count {
                return Err(failure("patch hunk body exceeds its declared counts"));
            }
        }
        if removed != old.count || added != new.count {
            return Err(failure(
                "patch hunk body does not match its declared counts",
            ));
        }
    }
    for line in &source[source_index..] {
        push(&mut output, line)?;
    }
    if !changed || output == original {
        return Err(failure("patch must change the file"));
    }
    Ok(output)
}
