use std::io;
use std::path::Path;

pub(crate) const SANDBOX_MARKER: &str = "/.galla-sandbox";

pub(crate) fn require_host() -> Result<(), String> {
    check_marker(Path::new(SANDBOX_MARKER))
}

fn check_marker(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(
            "already inside a Galla sandbox; a nested sandbox is unnecessary; run the intended command directly"
                .to_owned(),
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot inspect Galla sandbox marker: {error}")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn absent_marker_allows_host_but_any_existing_entry_rejects_nesting() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("marker");
        assert!(super::check_marker(&path).is_ok());
        std::fs::create_dir(&path).unwrap();
        assert!(
            super::check_marker(&path)
                .unwrap_err()
                .contains("unnecessary")
        );
        std::fs::remove_dir(&path).unwrap();
        std::os::unix::fs::symlink("missing", &path).unwrap();
        assert!(super::check_marker(&path).is_err());
    }
}
