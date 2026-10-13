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
/// On Windows this includes reparse points, which covers junctions as well as
/// symbolic links.  A reparse point is deliberately rejected rather than
/// interpreted, because discovery has no containment or target policy.
pub(crate) fn is_link_like(metadata: &fs::Metadata) -> bool {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        // FILE_ATTRIBUTE_REPARSE_POINT. `MetadataExt` is sufficient and keeps
        // the policy independent of a Windows-specific dependency.
        metadata.file_attributes() & 0x0400 != 0
    }

    #[cfg(not(windows))]
    {
        false
    }
}
