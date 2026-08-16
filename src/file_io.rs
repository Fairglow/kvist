//! Filesystem helpers with no-clobber atomic write semantics.

use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

use tempfile::NamedTempFile;

use crate::{KvistError, Result};

/// Writes UTF-8 content through a same-directory temporary file without ever
/// replacing an existing destination.
pub(crate) fn write_new_file_atomically(destination: &Path, contents: &str) -> Result<()> {
    let Some(parent) = destination.parent() else {
        return Err(KvistError::Io {
            operation: "determine file parent",
            path: destination.to_path_buf(),
            source: io::Error::other("destination has no parent"),
        });
    };
    let mut temporary_file = NamedTempFile::new_in(parent).map_err(|source| KvistError::Io {
        operation: "create temporary file",
        path: parent.to_path_buf(),
        source,
    })?;

    temporary_file
        .write_all(contents.as_bytes())
        .map_err(|source| KvistError::Io {
            operation: "write temporary file",
            path: destination.to_path_buf(),
            source,
        })?;
    temporary_file
        .as_file()
        .sync_all()
        .map_err(|source| KvistError::Io {
            operation: "sync temporary file",
            path: destination.to_path_buf(),
            source,
        })?;

    temporary_file
        .persist_noclobber(destination)
        .map(|_| ())
        .map_err(|error| KvistError::Io {
            operation: "persist generated file without overwriting",
            path: destination.to_path_buf(),
            source: error.error,
        })
}

/// Replaces a file through a synchronized same-directory temporary file.
pub(crate) fn replace_file_atomically(destination: &Path, contents: &str) -> Result<()> {
    let Some(parent) = destination.parent() else {
        return Err(KvistError::Io {
            operation: "determine file parent",
            path: destination.to_path_buf(),
            source: io::Error::other("destination has no parent"),
        });
    };
    let mut temporary_file = NamedTempFile::new_in(parent).map_err(|source| KvistError::Io {
        operation: "create temporary file",
        path: parent.to_path_buf(),
        source,
    })?;
    temporary_file
        .write_all(contents.as_bytes())
        .map_err(|source| KvistError::Io {
            operation: "write temporary file",
            path: destination.to_path_buf(),
            source,
        })?;
    temporary_file
        .as_file()
        .sync_all()
        .map_err(|source| KvistError::Io {
            operation: "sync temporary file",
            path: destination.to_path_buf(),
            source,
        })?;
    temporary_file
        .persist(destination)
        .map_err(|error| KvistError::Io {
            operation: "replace file atomically",
            path: destination.to_path_buf(),
            source: error.error,
        })?;
    sync_directory(parent)
}

/// Synchronizes a directory after a durable entry change where supported.
pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| KvistError::Io {
                operation: "sync parent directory",
                path: path.to_path_buf(),
                source,
            })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
