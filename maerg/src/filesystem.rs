//! Small cross-platform checks for filesystem objects that must not be followed.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Normalizes a relative or absolute path by removing redundant `.` (current directory) segments.
pub fn normalize_relative_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    if components.is_empty() {
        PathBuf::from(".")
    } else {
        components.iter().collect()
    }
}

/// Returns whether metadata identifies a link-like object.
///
/// Discovery rejects links rather than interpreting them, because discovery
/// has no containment or target policy.
pub(crate) fn is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}
