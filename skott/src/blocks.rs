//! Shared streamed-text block buffering.
//!
//! Model output arrives as small fragments. A block is either a paragraph
//! (terminated by a blank line, `\n\n`) or a fenced code block (terminated by
//! its matching closer). Lone newlines are soft breaks kept in the buffer
//! until a flush trigger, so a streamed sentence flows on rows rather than one
//! short row per token. Fences are matched so a code block is never split
//! across flushes even when it contains blank lines.
//!
//! Both the stand-alone TUI and the embedded Kvist log drive their rendering
//! from this buffer, so the two variants buffer and flush identically.

/// A buffer that accumulates streamed text and splits it into complete
/// Markdown blocks.
#[derive(Debug, Clone, Default)]
pub struct BlockBuffer {
    pending: String,
}

impl BlockBuffer {
    /// Creates an empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `text` to the buffer, normalizing CRLF and CR to LF.
    pub fn push(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.pending.push_str(&normalized);
    }

    /// Returns all completed blocks in the buffer, keeping the incomplete
    /// remainder. A block is complete when it ends at a blank line or a
    /// closed fence.
    pub fn flush_completed(&mut self) -> Vec<String> {
        let mut blocks = Vec::new();
        while let Some(end) = next_block_end(&self.pending) {
            let block = self.pending[..end].to_owned();
            let tail = self.pending[end..].to_owned();
            blocks.push(block);
            self.pending = tail;
        }
        blocks
    }

    /// Takes the entire buffer, including any incomplete remainder.
    /// Returns `None` when the buffer is empty.
    pub fn flush_all(&mut self) -> Option<String> {
        if self.pending.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending))
        }
    }

    /// Clears the buffer.
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// Whether the buffer holds any pending text.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// Returns the byte length of the next complete Markdown block in `pending`, or
/// `None` when the pending buffer holds no *closed* block yet (only an open
/// paragraph without a blank line, or an unmatched code fence). The caller
/// removes that prefix before flushing the next block.
///
/// A paragraph block ends at the next blank line (`\n\n`); a fenced code block
/// ends at a matching closer fence. Blank lines between the opening fence and
/// its closer are part of the code, never a paragraph boundary.
pub fn next_block_end(pending: &str) -> Option<usize> {
    let stripped = pending.trim_start_matches('\n');
    let lead = pending.len() - stripped.len();
    if stripped.is_empty() {
        // Only trailing newlines remain; nothing to flush.
        return None;
    }
    let first_line_end = stripped.find('\n').unwrap_or(stripped.len());
    let first_line = &stripped[..first_line_end];
    if let Some((ch, len, _info)) = fence_split(first_line) {
        // Scan for a matching closer fence; only fence characters, no content.
        let mut rest = stripped.get(first_line_end + 1..)?;
        let mut consumed = first_line_end + 1;
        loop {
            let line_end = rest.find('\n').unwrap_or(rest.len());
            let line = &rest[..line_end];
            consumed += line_end;
            if let Some((closer, clo_len, rest_str)) = fence_split(line)
                && closer == ch
                && clo_len >= len
                && rest_str.trim().is_empty()
            {
                return Some(lead + consumed);
            }
            if line_end >= rest.len() {
                // Reached the end without a matching closer.
                return None;
            }
            consumed += 1; // the newline after this line
            rest = &rest[line_end + 1..];
        }
    } else {
        // A paragraph: flush up to and including the next blank line.
        stripped.find("\n\n").map(|pos| lead + pos + 2)
    }
}

/// Recognises a code fence opener or closer and returns its character, run
/// length, and the (possibly empty) info string that follows the run. Opening
/// fences allow an info string (`rust`, `python 3`, ...); closers must contain
/// only fence characters.
fn fence_split(line: &str) -> Option<(char, usize, &str)> {
    let trimmed = line.trim_start();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let ch = chars[0];
    if ch != '`' && ch != '~' {
        return None;
    }
    let run = chars.iter().take_while(|c| **c == ch).count();
    if run < 3 {
        return None;
    }
    Some((ch, run, &trimmed[run..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer_has_no_blocks() {
        let mut buf = BlockBuffer::new();
        assert!(buf.is_empty());
        assert!(buf.flush_completed().is_empty());
        assert!(buf.flush_all().is_none());
    }

    #[test]
    fn incomplete_paragraph_stays_pending() {
        let mut buf = BlockBuffer::new();
        buf.push("hello world");
        assert!(buf.flush_completed().is_empty());
        assert!(!buf.is_empty());
        assert_eq!(buf.flush_all(), Some("hello world".to_owned()));
        assert!(buf.is_empty());
    }

    #[test]
    fn blank_line_terminates_a_paragraph() {
        let mut buf = BlockBuffer::new();
        buf.push("first paragraph\n\nsecond paragraph");
        let blocks = buf.flush_completed();
        assert_eq!(blocks, vec!["first paragraph\n\n".to_owned()]);
        // The second paragraph is still pending (no trailing blank line).
        assert!(!buf.is_empty());
        assert_eq!(buf.flush_all(), Some("second paragraph".to_owned()));
    }

    #[test]
    fn multiple_paragraphs_flush_in_order() {
        let mut buf = BlockBuffer::new();
        buf.push("one\n\ntwo\n\nthree");
        let blocks = buf.flush_completed();
        assert_eq!(blocks, vec!["one\n\n".to_owned(), "two\n\n".to_owned()]);
        assert_eq!(buf.flush_all(), Some("three".to_owned()));
    }

    #[test]
    fn lone_newlines_are_soft_breaks_not_boundaries() {
        let mut buf = BlockBuffer::new();
        buf.push("line one\nline two\nline three");
        assert!(buf.flush_completed().is_empty());
        assert_eq!(
            buf.flush_all(),
            Some("line one\nline two\nline three".to_owned())
        );
    }

    #[test]
    fn closed_fence_flushes_as_one_block() {
        let mut buf = BlockBuffer::new();
        buf.push("```rust\nfn main() {}\n```");
        let blocks = buf.flush_completed();
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("fn main() {}"));
        assert!(buf.is_empty());
    }

    #[test]
    fn open_fence_stays_pending() {
        let mut buf = BlockBuffer::new();
        buf.push("```rust\nfn main() {");
        assert!(buf.flush_completed().is_empty());
        assert!(!buf.is_empty());
    }

    #[test]
    fn fence_with_blank_lines_inside_is_one_block() {
        let mut buf = BlockBuffer::new();
        buf.push("```\n\n\n```\n");
        let blocks = buf.flush_completed();
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains('\n'));
    }

    #[test]
    fn paragraph_after_fence() {
        let mut buf = BlockBuffer::new();
        buf.push("```sh\necho hi\n```\n\ntrailing text");
        let blocks = buf.flush_completed();
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("echo hi"));
        // The trailing paragraph is still pending.
        assert!(!buf.is_empty());
    }

    #[test]
    fn crlf_is_normalized() {
        let mut buf = BlockBuffer::new();
        buf.push("line one\r\nline two\r\n\r\nnext");
        let blocks = buf.flush_completed();
        assert_eq!(blocks, vec!["line one\nline two\n\n".to_owned()]);
        assert_eq!(buf.flush_all(), Some("next".to_owned()));
    }

    #[test]
    fn clear_empties_the_buffer() {
        let mut buf = BlockBuffer::new();
        buf.push("some text");
        buf.clear();
        assert!(buf.is_empty());
        assert!(buf.flush_all().is_none());
    }

    #[test]
    fn leading_newlines_before_fence() {
        for lead in ["\n", "\n\n", "\n\n\n"] {
            let block = format!("{lead}```rust\nfn main() {{}}\n```");
            assert_eq!(next_block_end(&block), Some(block.len()));
        }
    }

    #[test]
    fn incomplete_fence_openers_wait() {
        for opener in ["```", "```rust", "~~~", "~~~text", "\n\n```rust"] {
            assert_eq!(next_block_end(opener), None);
        }
    }
}
