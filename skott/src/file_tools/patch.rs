//! Strict, path-free unified hunks. All matching is exact; no fuzz or fallback.

use super::{
    failure,
    stream::{Output, Source},
};
use crate::error::Result;
use std::io::BufRead;

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
        if (start == 0 && count != 0) || start.checked_add(count).is_none() {
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

fn push(output: &mut Output<'_>, text: &str) -> Result<()> {
    if !output.final_newline && !text.is_empty() {
        return Err(failure(
            "patch leaves content after a missing-final-newline line",
        ));
    }
    output.push(text.as_bytes())
}

fn copy_line(source: &mut Source, output: &mut Output<'_>) -> Result<bool> {
    let mut copied = false;
    loop {
        let bytes = source
            .reader
            .fill_buf()
            .map_err(|e| crate::error::io_error("stream patch line", None, e))?;
        if bytes.is_empty() {
            return Ok(copied);
        }
        if !copied && !output.final_newline {
            return Err(failure(
                "patch leaves content after a missing-final-newline line",
            ));
        }
        let end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1);
        let count = end.unwrap_or(bytes.len());
        output.push(&bytes[..count])?;
        source.consume(count)?;
        copied = true;
        if end.is_some() {
            return Ok(true);
        }
    }
}

fn exact_line(source: &mut Source, content: &str) -> Result<()> {
    let mut offset = 0;
    while offset < content.len() {
        let bytes = source
            .reader
            .fill_buf()
            .map_err(|e| crate::error::io_error("compare patch line", None, e))?;
        let count = bytes.len().min(content.len() - offset);
        if count == 0 || bytes[..count] != content.as_bytes()[offset..offset + count] {
            return Err(failure(
                "patch context/removal does not match the exact preimage",
            ));
        }
        source.consume(count)?;
        offset += count;
    }
    if !content.ends_with('\n')
        && !source
            .reader
            .fill_buf()
            .map_err(|e| crate::error::io_error("inspect patch line end", None, e))?
            .is_empty()
    {
        return Err(failure(
            "patch context/removal does not match the exact preimage",
        ));
    }
    Ok(())
}

pub(super) fn apply(source: &mut Source, output: &mut Output<'_>, patch: &str) -> Result<()> {
    if patch.is_empty() || !patch.ends_with('\n') {
        return Err(invalid());
    }
    let lines: Vec<&str> = patch.split_inclusive('\n').collect();
    let mut source_index = 0;
    let mut output_lines = 0;
    let mut index = 0;
    let mut changed = false;
    while index < lines.len() {
        let (old, new) = header(lines[index])?;
        index += 1;
        if old.index() < source_index {
            return Err(failure(
                "patch hunks overlap, are out of order or exceed the preimage",
            ));
        }
        while source_index < old.index() {
            if !copy_line(source, output)? {
                return Err(failure(
                    "patch hunks overlap, are out of order or exceed the preimage",
                ));
            }
            source_index += 1;
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
                exact_line(source, content)?;
                source_index += 1;
                removed += 1;
            }
            if matches!(prefix, b' ' | b'+') {
                push(output, content)?;
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
    while copy_line(source, output)? {}
    if !changed {
        return Err(failure("patch must change the file"));
    }
    Ok(())
}
