//! Transcript writer for Blad.
//!
//! Coordinates writing streaming chunks to both the terminal and durable
//! artifacts (session files), managing truncation state.

use std::io;
use std::time::Instant;

use crate::session_manager::SessionManager;

/// Transcript writer that handles streaming and durable writes.
pub struct TranscriptWriter {
    session: SessionManager,
    current_sequence: Option<usize>,
    current_content: String,
    current_kind: String,
    current_disposition: Option<String>,
    last_flush: Instant,
}

impl TranscriptWriter {
    /// Create a new transcript writer.
    pub fn new(session: SessionManager) -> Self {
        Self {
            session,
            current_sequence: None,
            current_content: String::new(),
            current_kind: String::new(),
            current_disposition: None,
            last_flush: Instant::now(),
        }
    }

    /// Start a new message entry.
    pub fn start_message(&mut self, kind: &str, disposition: Option<&str>) -> io::Result<usize> {
        // Flush any pending message
        if let Some(sequence) = self.current_sequence {
            self.flush_message(sequence, false)?;
        }

        self.current_kind = kind.to_string();
        self.current_disposition = disposition.map(|s| s.to_string());
        self.current_content.clear();
        self.current_sequence = None;

        Ok(0)
    }

    /// Append streaming content to the current message.
    pub fn append(&mut self, content: &str) -> io::Result<()> {
        if let Some(sequence) = self.current_sequence {
            // Subsequent chunks: append to existing entry
            self.current_content.push_str(content);
            self.session
                .update_message(sequence, &self.current_content, false)?;
        } else {
            // First chunk: create the entry
            self.current_content.push_str(content);
            let sequence = self.session.add_message(
                &self.current_kind,
                self.current_disposition.as_deref(),
                &self.current_content,
                false,
            )?;
            self.current_sequence = Some(sequence);
        }

        // Periodic flush to terminal
        if self.last_flush.elapsed().as_millis() > 100 {
            self.last_flush = Instant::now();
        }

        Ok(())
    }

    /// Flush the current message as complete.
    pub fn flush_message(&mut self, sequence: usize, complete: bool) -> io::Result<()> {
        let content = &self.current_content;
        self.session.update_message(sequence, content, complete)?;

        self.current_sequence = None;
        self.current_content.clear();
        self.current_kind.clear();
        self.current_disposition = None;

        Ok(())
    }

    /// Get the session manager.
    pub fn session(&self) -> &SessionManager {
        &self.session
    }

    /// Complete the session.
    pub fn complete_session(&mut self) -> io::Result<()> {
        if let Some(sequence) = self.current_sequence {
            self.flush_message(sequence, true)?;
        }
        self.session.complete()
    }

    /// Cancel the session.
    pub fn cancel_session(&mut self) -> io::Result<()> {
        if let Some(sequence) = self.current_sequence {
            self.flush_message(sequence, true)?;
        }
        self.session.cancel()
    }
}
