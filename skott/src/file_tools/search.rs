//! Bounded literal scans with separate byte-work and collected-result cursors.

use super::*;
use std::collections::VecDeque;
use std::io::{Seek, SeekFrom};

struct Line {
    number: Option<u64>,
    relative: u64,
    start: u64,
    prefix: Vec<u8>,
    bytes: u64,
    ended: bool,
    partial: bool,
}

impl Line {
    fn new(number: Option<u64>, relative: u64, start: u64, partial: bool) -> Self {
        Self {
            number,
            relative,
            start,
            prefix: Vec::new(),
            bytes: 0,
            ended: false,
            partial,
        }
    }

    fn add(&mut self, byte: u8) {
        if byte == b'\n' {
            self.ended = true;
            return;
        }
        self.bytes += 1;
        if self.prefix.len() < MAX_LINE_BYTES {
            self.prefix.push(byte);
        }
    }

    fn preview(&self, path: &str, offset: u64) -> Value {
        let valid = match std::str::from_utf8(&self.prefix) {
            Ok(text) => text,
            Err(error) => std::str::from_utf8(&self.prefix[..error.valid_up_to()]).unwrap_or(""),
        };
        let mut value = json!({
            "path":path, "line":self.number, "match_byte_offset":offset,
            "content":valid, "truncated":self.partial || !self.ended || (valid.len() as u64) < self.bytes
        });
        if self.number.is_none() {
            value["line_in_scan"] = json!(self.relative);
        }
        if self.partial {
            value["preview_offset"] = json!(self.start);
        }
        value
    }
}

#[derive(Default)]
struct Scan {
    next: Option<Value>,
    prefix_bytes: usize,
    effective_offset: u64,
}

fn prefixes(pattern: &[u8]) -> Vec<usize> {
    let mut result = vec![0; pattern.len()];
    for index in 1..pattern.len() {
        let mut matched = result[index - 1];
        while matched > 0 && pattern[index] != pattern[matched] {
            matched = result[matched - 1];
        }
        if pattern[index] == pattern[matched] {
            matched += 1;
        }
        result[index] = matched;
    }
    result
}

fn search_file(
    mut file: File,
    path: &str,
    relative: &str,
    args: &SearchArgs,
    state: &mut WalkState,
    matches: &mut Vec<Value>,
    scan: &mut Scan,
) -> Result<()> {
    let metadata = file
        .metadata()
        .map_err(|e| io_error("inspect ordinary search file", None, e))?;
    if !metadata.is_file() {
        return Err(failure(
            "ordinary search file changed to an unsupported entry",
        ));
    }
    if args
        .file_pattern
        .as_ref()
        .is_some_and(|pattern| !relative.contains(pattern))
    {
        state.excluded_files += 1;
        return Ok(());
    }
    let requested = args.scan_offset.unwrap_or(0);
    let mut offset = requested.min(metadata.len());
    scan.effective_offset = offset;
    let budget = MAX_SCAN_BYTES - state.scanned_bytes;
    if budget == 0
        || (metadata.len().saturating_sub(offset) > budget as u64 && budget < MAX_QUERY_BYTES + 4)
    {
        scan.next = Some(json!({"path":path,"offset":offset}));
        state.scan_limit_reached = true;
        return Ok(());
    }
    let mut line = Line::new(Some(1), 1, 0, false);
    let mut bytes = [0_u8; stream::BUFFER_BYTES];
    let mut utf8 = stream::Utf8::default();
    if offset <= MAX_SCAN_BYTES as u64 {
        let mut remaining = offset;
        let mut prefix_position = 0_u64;
        while remaining > 0 {
            let bound = bytes.len().min(remaining as usize);
            let count = stream::read(&mut file, &mut bytes[..bound])?;
            if count == 0 {
                break;
            }
            scan.prefix_bytes += count;
            remaining -= count as u64;
            if bytes[..count].contains(&0) || utf8.feed(&bytes[..count]).is_err() {
                state.skipped_binary += 1;
                return Ok(());
            }
            for byte in &bytes[..count] {
                line.add(*byte);
                prefix_position += 1;
                if *byte == b'\n' {
                    line = Line::new(line.number.map(|n| n + 1), 1, prefix_position, false);
                }
            }
        }
        offset = prefix_position;
        scan.effective_offset = offset;
    } else {
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| io_error("seek resumed search", None, e))?;
        line = Line::new(None, 1, offset, true);
    }
    let pattern = args.query.as_bytes();
    let prefixes = prefixes(pattern);
    let first_match = matches.len();
    let mut history = VecDeque::new();
    let mut current_hit = None;
    let mut last_hit_line = None;
    let mut matched = 0;
    let mut position = offset;
    let mut consumed = 0;
    let mut tail = VecDeque::with_capacity(MAX_QUERY_BYTES + 4);
    let mut eof = false;
    // For an arbitrary continuation in the middle of a UTF-8 scalar, move
    // forward to the next boundary and report the actual beginning.
    let mut beginning = true;
    while consumed < budget {
        let bound = bytes.len().min(budget - consumed);
        let count = stream::read(&mut file, &mut bytes[..bound])?;
        if count == 0 {
            eof = true;
            break;
        }
        consumed += count;
        state.scanned_bytes += count;
        let mut begin = 0;
        if beginning {
            if offset > MAX_SCAN_BYTES as u64 {
                while begin < count && bytes[begin] & 0xc0 == 0x80 {
                    begin += 1;
                }
                if begin > 0 {
                    position += begin as u64;
                    scan.effective_offset = position;
                    line.start = position;
                }
            }
            beginning = false;
        }
        if bytes[begin..count].contains(&0) || utf8.feed(&bytes[begin..count]).is_err() {
            state.skipped_binary += 1;
            matches.truncate(first_match);
            return Ok(());
        }
        for byte in &bytes[begin..count] {
            tail.push_back(*byte);
            if tail.len() > pattern.len() + 3 {
                tail.pop_front();
            }
            line.add(*byte);
            while matched > 0 && *byte != pattern[matched] {
                matched = prefixes[matched - 1];
            }
            if *byte == pattern[matched] {
                matched += 1;
            }
            position += 1;
            if matched == pattern.len() {
                let hit_offset = position - pattern.len() as u64;
                let hit_line = if hit_offset >= line.start {
                    &line
                } else {
                    history
                        .iter()
                        .rev()
                        .find(|prior: &&Line| prior.start <= hit_offset)
                        .ok_or_else(|| failure("search line context unavailable"))?
                };
                if last_hit_line != Some(hit_line.start) {
                    if matches.len() >= MAX_ENTRIES {
                        return Err(failure(
                            "search match count exceeds the result bound (4096 matching lines); narrow path, query or literal file_pattern",
                        ));
                    }
                    matches.push(hit_line.preview(path, hit_offset));
                    if hit_line.start == line.start {
                        current_hit = Some((matches.len() - 1, hit_offset));
                    }
                    last_hit_line = Some(hit_line.start);
                }
                matched = 0;
            }
            if *byte == b'\n' {
                if let Some((index, offset)) = current_hit.take() {
                    matches[index] = line.preview(path, offset);
                }
                let next = Line::new(
                    line.number.map(|n| n + 1),
                    line.relative + 1,
                    position,
                    false,
                );
                history.push_back(std::mem::replace(&mut line, next));
            }
            while history.front().is_some_and(|prior| {
                prior.start + prior.bytes < position.saturating_sub(pattern.len() as u64)
            }) {
                history.pop_front();
            }
        }
    }
    // A one-byte lookahead distinguishes exact-budget EOF from partial work.
    if !eof {
        eof = stream::read(&mut file, &mut bytes[..1])? == 0;
    }
    if eof && utf8.finish().is_err() {
        state.skipped_binary += 1;
        matches.truncate(first_match);
        return Ok(());
    }
    line.ended = eof;
    if let Some((index, offset)) = current_hit {
        matches[index] = line.preview(path, offset);
    }
    if !eof {
        let overlap = (pattern.len() - 1)
            .max(utf8.pending_bytes())
            .min(tail.len());
        let mut rewind = overlap;
        while rewind > 0 && rewind < tail.len() && tail[tail.len() - rewind] & 0xc0 == 0x80 {
            rewind += 1;
        }
        let next = position
            .saturating_sub(rewind as u64)
            .max(scan.effective_offset);
        scan.next = Some(json!({"path":path,"offset":next}));
        state.scan_limit_reached = true;
    }
    Ok(())
}

pub(super) fn search(args: &SearchArgs) -> Result<Value> {
    let mut state = WalkState::default();
    let mut matches = Vec::<Value>::new();
    let mut scan = Scan::default();
    let scope = match search_scope(&args.path)? {
        SearchScope::Directory(root) => {
            if args.scan_offset.is_some() {
                return Err(failure(
                    "scan_offset requires one regular-file path; use next_scan.path or narrow the scope",
                ));
            }
            walk(
                &root,
                &args.path,
                0,
                args.include_generated,
                &mut state,
                &mut |fd, name, path, state| {
                    let opened = openat(
                        fd,
                        name,
                        OFlag::O_RDONLY | OFlag::O_NONBLOCK | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(|e| failure(format!("cannot read ordinary search file: {e}")))?;
                    let relative = path
                        .strip_prefix(&args.path)
                        .unwrap_or(path)
                        .trim_start_matches('/');
                    search_file(
                        File::from(opened),
                        path,
                        relative,
                        args,
                        state,
                        &mut matches,
                        &mut scan,
                    )
                },
            )?;
            "directory"
        }
        SearchScope::File(file) => {
            state.visited = 1;
            search_file(
                file,
                &args.path,
                args.path.rsplit('/').next().unwrap_or(&args.path),
                args,
                &mut state,
                &mut matches,
                &mut scan,
            )?;
            "file"
        }
    };
    matches.sort_by(|a, b| {
        a["path"].as_str().cmp(&b["path"].as_str()).then_with(|| {
            a["match_byte_offset"]
                .as_u64()
                .cmp(&b["match_byte_offset"].as_u64())
        })
    });
    fitted_page(&matches, args.offset, args.limit, |page, next| {
        json!({
            "matches":page,"offset":args.offset,"next_offset":next,"total":matches.len(),
            "skipped_symlinks":state.skipped_symlinks,"skipped_binary":state.skipped_binary,
            "skipped_generated":state.skipped_generated,"skipped_oversized":0,
            "skipped_special":state.skipped_special,"excluded_files":state.excluded_files,
            "visited_entries":state.visited,"scanned_bytes":state.scanned_bytes,
            "scan_limit_reached":state.scan_limit_reached,"next_scan":scan.next,
            "scan_offset":args.scan_offset.unwrap_or(0),"effective_scan_offset":if scope == "file" {Some(scan.effective_offset)} else {None},
            "prefix_scanned_bytes":scan.prefix_bytes,
            "next_step":if state.scan_limit_reached {Some("Partial search: 8 MiB scan budget reached. Resume next_scan.path with scan_offset=next_scan.offset and offset=0; offset/next_offset paginate collected matches only. A directory continuation covers that file, not later siblings; narrow path or file_pattern to inspect remaining files.")} else {None},
            "complete":state.complete() && args.scan_offset.unwrap_or(0) == 0,
            "scope":scope
        })
    })
}
