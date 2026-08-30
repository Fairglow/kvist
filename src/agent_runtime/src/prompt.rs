use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, BufRead, IsTerminal, Read, Write},
    os::unix::fs::{FileTypeExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Command,
};

use crate::{Error, Result, split_raw_command};

/// Maximum prompt size accepted from every source.
pub const MAX_PROMPT_BYTES: u64 = 1024 * 1024;

/// Resolves one prompt from a value, file, editor, or standard input.
pub fn resolve_prompt(
    prompt: Option<String>,
    file: Option<&Path>,
    use_editor: bool,
) -> Result<String> {
    if let Some(prompt) = prompt {
        return validate_prompt(prompt);
    }
    if let Some(path) = file {
        if path == Path::new("-") {
            return read_standard_input();
        }
        return read_file(path);
    }
    if use_editor {
        return edit_prompt();
    }

    let stdin = io::stdin();
    if !stdin.is_terminal() {
        return read_bounded(stdin.lock(), "standard input");
    }

    eprint!("No prompt supplied. Open an editor? [Y/n]: ");
    io::stderr().flush().map_err(|source| Error::Io {
        operation: "flush prompt input request",
        path: PathBuf::from("stderr"),
        source,
    })?;
    let mut reader = stdin.lock();
    let mut choice = String::new();
    reader.read_line(&mut choice).map_err(|source| Error::Io {
        operation: "read prompt input choice",
        path: PathBuf::from("stdin"),
        source,
    })?;
    if !matches!(choice.trim().to_ascii_lowercase().as_str(), "n" | "no") {
        return edit_prompt();
    }

    eprintln!("Enter the prompt, then press Ctrl-D:");
    read_bounded(reader, "standard input")
}

fn read_standard_input() -> Result<String> {
    read_bounded(io::stdin().lock(), "standard input")
}

fn read_file(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect prompt file",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(Error::InvalidPromptInput {
            reason: format!(
                "prompt file `{}` must be a regular non-link file",
                path.display()
            ),
        });
    }

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|source| Error::Io {
            operation: "open prompt file without following links",
            path: path.to_path_buf(),
            source,
        })?;
    let opened_metadata = file.metadata().map_err(|source| Error::Io {
        operation: "inspect opened prompt file",
        path: path.to_path_buf(),
        source,
    })?;
    if opened_metadata.file_type().is_symlink()
        || opened_metadata.file_type().is_socket()
        || !opened_metadata.file_type().is_file()
    {
        return Err(Error::InvalidPromptInput {
            reason: format!(
                "prompt file `{}` must remain a regular non-link file",
                path.display()
            ),
        });
    }
    if opened_metadata.len() > MAX_PROMPT_BYTES {
        return Err(Error::InvalidPromptInput {
            reason: format!(
                "prompt file `{}` exceeds the {MAX_PROMPT_BYTES}-byte limit",
                path.display()
            ),
        });
    }

    read_bounded(file, &format!("prompt file `{}`", path.display()))
}

fn read_bounded(reader: impl Read, source_name: &str) -> Result<String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_PROMPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| Error::Io {
            operation: "read prompt input",
            path: PathBuf::from(source_name),
            source,
        })?;
    if bytes.len() as u64 > MAX_PROMPT_BYTES {
        return Err(Error::InvalidPromptInput {
            reason: format!("prompt from {source_name} exceeds the {MAX_PROMPT_BYTES}-byte limit"),
        });
    }
    let prompt = String::from_utf8(bytes).map_err(|_| Error::InvalidPromptInput {
        reason: format!("prompt from {source_name} must be valid UTF-8"),
    })?;
    validate_prompt(prompt)
}

fn validate_prompt(prompt: String) -> Result<String> {
    if prompt.len() as u64 > MAX_PROMPT_BYTES {
        return Err(Error::InvalidPromptInput {
            reason: format!("prompt exceeds the {MAX_PROMPT_BYTES}-byte limit"),
        });
    }
    if prompt.trim().is_empty() {
        return Err(Error::InvalidPromptInput {
            reason: "prompt must not be empty".to_owned(),
        });
    }
    Ok(prompt)
}

fn edit_prompt() -> Result<String> {
    let directory = tempfile::tempdir().map_err(|source| Error::Io {
        operation: "create prompt editor directory",
        path: env::temp_dir(),
        source,
    })?;
    let prompt_path = directory.path().join("prompt.md");
    fs::write(&prompt_path, "").map_err(|source| Error::Io {
        operation: "create prompt editor file",
        path: prompt_path.clone(),
        source,
    })?;

    let editor = env::var_os("VISUAL")
        .or_else(|| env::var_os("EDITOR"))
        .unwrap_or_else(|| "vi".into())
        .into_string()
        .map_err(|_| Error::InvalidPromptInput {
            reason: "VISUAL or EDITOR must be valid UTF-8".to_owned(),
        })?;
    let (program, mut arguments) = if Path::new(&editor).is_file() {
        (editor, Vec::new())
    } else {
        split_raw_command(&editor)?
    };
    arguments.push(
        prompt_path
            .to_str()
            .ok_or_else(|| Error::InvalidPromptInput {
                reason: "temporary prompt path must be valid UTF-8".to_owned(),
            })?
            .to_owned(),
    );

    let status = Command::new(&program)
        .args(&arguments)
        .status()
        .map_err(|source| Error::Io {
            operation: "open prompt editor",
            path: PathBuf::from(&program),
            source,
        })?;
    if !status.success() {
        return Err(Error::InvalidPromptInput {
            reason: format!("editor `{program}` exited with status {status}"),
        });
    }

    read_file(&prompt_path)
}
