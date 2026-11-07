use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use crate::error::{KvistError, Result};

fn path_error(path: &Path, reason: impl Into<String>) -> KvistError {
    KvistError::Io {
        operation: "prepare offline configuration",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, reason.into()),
    }
}

fn ensure_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(metadata) => Err(path_error(
            path,
            if metadata.file_type().is_symlink() {
                "configuration directory is a symbolic link"
            } else {
                "configuration directory is not a directory"
            },
        )),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|source| KvistError::Io {
                operation: "create offline configuration directory",
                path: path.to_path_buf(),
                source,
            })
        }
        Err(source) => Err(KvistError::Io {
            operation: "inspect offline configuration directory",
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn prepare(
    project_dir: &Path,
    directory: &str,
    filename: &str,
    contents: &str,
) -> Result<PathBuf> {
    let state_dir = project_dir.join(".kvist");
    ensure_directory(&state_dir)?;
    let config_dir = state_dir.join(directory);
    ensure_directory(&config_dir)?;
    let path = config_dir.join(filename);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(path_error(&path, "configuration file is a symbolic link"));
            }
            if !metadata.is_file() {
                let kind = metadata.file_type();
                let observed = if kind.is_dir() {
                    "a directory"
                } else if kind.is_fifo() {
                    "a FIFO"
                } else if kind.is_socket() {
                    "a socket"
                } else if kind.is_block_device() {
                    "a block device"
                } else if kind.is_char_device() {
                    "a character device"
                } else {
                    "an unknown special entry"
                };
                return Err(path_error(
                    &path,
                    format!("configuration file is {observed}"),
                ));
            }
            if metadata.nlink() != 1 {
                return Err(path_error(
                    &path,
                    format!("configuration file has {} hard links", metadata.nlink()),
                ));
            }
            match crate::file_io::read_text_bounded(&path, contents.len()) {
                Ok(current) if current == contents => return Ok(config_dir),
                Ok(_) => {}
                Err(source)
                    if matches!(
                        source.kind(),
                        io::ErrorKind::FileTooLarge | io::ErrorKind::InvalidData
                    ) => {}
                Err(source) => {
                    return Err(KvistError::Io {
                        operation: "read offline configuration",
                        path,
                        source,
                    });
                }
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect offline configuration",
                path,
                source,
            });
        }
    }
    crate::file_io::replace_file_atomically(&path, contents)?;
    Ok(config_dir)
}
