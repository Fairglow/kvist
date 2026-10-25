use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use crate::viewer_reload::read_document_source;

pub(crate) struct OpenFile {
    pub path: Option<PathBuf>,
    pub content: String,
    pub position: Position,
}

#[derive(Default)]
pub(crate) struct Position {
    pub y: usize,
    pub x: usize,
}

pub(crate) fn load_files(paths: &[PathBuf]) -> io::Result<Vec<OpenFile>> {
    let mut files: Vec<OpenFile> = Vec::new();
    for path in paths {
        let canonical = if path == Path::new("-") {
            None
        } else {
            Some(std::fs::canonicalize(path).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("cannot open {}: {error}", path.display()),
                )
            })?)
        };
        if files.iter().any(|file| file.path == canonical) {
            continue;
        }
        let content = if let Some(path) = &canonical {
            read_document_source(path)
        } else if io::stdin().is_terminal() {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "'-' requires redirected stdin; supply a file path instead",
            ))
        } else {
            crate::file_input::read_bounded(io::stdin().lock(), 16 * 1024 * 1024).and_then(
                |bytes| {
                    String::from_utf8(bytes)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
                },
            )
        }
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot read {}: {error}", path.display()),
            )
        })?;
        files.push(OpenFile {
            path: canonical,
            content,
            position: Position::default(),
        });
    }
    if files.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "at least one input is required",
        ));
    }
    Ok(files)
}

pub(crate) fn tab_title(files: &[OpenFile], active: usize) -> String {
    let mut title = "=== Blad: ".to_string();
    for (index, file) in files.iter().enumerate() {
        if index > 0 {
            title.push_str(" | ");
        }
        let label = file.path.as_deref().and_then(Path::file_name).map_or_else(
            || "Markdown".to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        if index == active {
            title.push_str(&format!("[{label}]"));
        } else {
            title.push_str(&format!(" {label} "));
        }
    }
    title
}

pub(crate) fn edit_file(path: &Path) -> io::Result<String> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nvim".to_string());
    let args: Vec<&str> = editor.split_whitespace().collect();
    let Some(program) = args.first() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot open editor: EDITOR is empty",
        ));
    };
    let status = std::process::Command::new(program)
        .args(&args[1..])
        .arg(path)
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "editor exited with status: {status}"
        )));
    }
    read_document_source(path)
}
