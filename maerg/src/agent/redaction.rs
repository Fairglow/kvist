use std::io::{self, Write};

/// Streaming longest-match redaction at UTF-8 boundaries.
pub(super) struct StreamRedactor {
    values: Vec<String>,
    carry: String,
}

impl StreamRedactor {
    pub(super) fn new(values: &[String]) -> Self {
        Self {
            values: values
                .iter()
                .filter(|value| !value.is_empty())
                .cloned()
                .collect(),
            carry: String::new(),
        }
    }

    pub(super) fn absorb(&mut self, delta: &str) -> String {
        self.carry.push_str(delta);
        self.scan(false)
    }

    fn scan(&mut self, final_chunk: bool) -> String {
        let mut output = String::new();
        let mut cursor = 0;
        while cursor < self.carry.len() {
            let suffix = &self.carry[cursor..];
            if !final_chunk
                && self
                    .values
                    .iter()
                    .any(|value| value.len() > suffix.len() && value.starts_with(suffix))
            {
                break;
            }
            if let Some(length) = self
                .values
                .iter()
                .filter(|value| suffix.starts_with(value.as_str()))
                .map(String::len)
                .max()
            {
                output.push_str("[REDACTED]");
                cursor += length;
            } else if let Some(character) = suffix.chars().next() {
                output.push(character);
                cursor += character.len_utf8();
            }
        }
        self.carry.drain(..cursor);
        output
    }

    pub(super) fn finish(&mut self) -> String {
        self.scan(true)
    }

    pub(super) fn emit_to_stdout(text: &str) {
        if !text.is_empty() {
            let mut stdout = io::stdout().lock();
            let _ = stdout.write_all(text.as_bytes());
            let _ = stdout.flush();
        }
    }
}
