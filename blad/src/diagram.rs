use std::io::{self, Write};
use std::time::Duration;

use crate::mermaid::{self, DiagramControl, DiagramError};
use crate::{DiagramCharset, MermaidMode, RenderOptions};

/// Cooperative cancellation for a controlled Markdown render.
///
/// Clones share permanent cancellation state. Hosts may retain a clone in
/// another execution context; rendering itself does not start any threads.
#[derive(Clone, Default)]
pub struct RenderCancellation {
    inner: DiagramControl,
}

impl RenderCancellation {
    /// Create an uncancelled control without a caller deadline.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request permanent cancellation for this control and its clones.
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    /// Whether explicit cancellation was requested; this excludes deadlines.
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// Install a cooperative deadline relative to this call.
    ///
    /// An earlier ancestor or backend deadline still applies. Checkpoints,
    /// not preemption, enforce deadlines, so this is not a wall-clock bound.
    pub fn with_deadline(self, duration: Duration) -> Self {
        Self {
            inner: self.inner.with_deadline(duration),
        }
    }

    pub(crate) fn checkpoint(&self) -> io::Result<()> {
        self.inner.checkpoint().map_err(|error| {
            io::Error::new(
                io::ErrorKind::Interrupted,
                format!("Markdown rendering cancelled: {error}"),
            )
        })
    }
}

pub(crate) fn write(
    source: &str,
    writer: &mut impl Write,
    options: &RenderOptions,
    width: Option<usize>,
    cancellation: Option<&RenderCancellation>,
) -> io::Result<()> {
    if options.mermaid == MermaidMode::Source {
        writeln!(writer, "Mermaid diagram (source):")?;
        return text(writer, source);
    }
    let ascii = options.diagram_charset == DiagramCharset::Ascii;
    let result = if let Some(control) = cancellation {
        mermaid::render_controlled(source, ascii, width, &control.inner)
    } else {
        mermaid::render(source, ascii, width)
    };
    match result {
        Ok(rendered) => text(writer, &rendered),
        Err(error @ DiagramError::Cancelled(_)) => {
            Err(io::Error::new(io::ErrorKind::Interrupted, error))
        }
        Err(error) => {
            let error = error.to_string();
            writeln!(
                writer,
                "[Mermaid diagram: {}]",
                crate::markdown::inert_source(&error)
            )?;
            let mut end = source.len().min(65_536);
            while !source.is_char_boundary(end) {
                end -= 1;
            }
            text(writer, &source[..end])?;
            if end < source.len() {
                writeln!(writer, "[... {} source bytes omitted]", source.len() - end)?;
            }
            Ok(())
        }
    }
}

fn text(writer: &mut impl Write, source: &str) -> io::Result<()> {
    let safe = crate::markdown::inert_source(source);
    writer.write_all(safe.as_bytes())?;
    if !safe.ends_with('\n') {
        writeln!(writer)?;
    }
    Ok(())
}
