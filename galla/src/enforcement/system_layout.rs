//! Live read-only System views. Names do not grant host privileges.

use std::fs;
use std::path::{Path, PathBuf};

/// Check if a symlink target is within the allowed read-only system layout.
fn is_within_system_layout(host_root: &Path, target: &Path) -> bool {
    let allowed = ["usr", "lib", "lib64", "bin", "sbin", "etc/alternatives"];

    let relative = match target.strip_prefix(host_root) {
        Ok(rel) => rel,
        Err(_) => return false,
    };

    for dir in allowed {
        if relative.starts_with(dir) {
            return true;
        }
    }

    false
}

/// Resolve a symlink target relative to its source location.
fn resolve_symlink_target(source: &Path, target: &Path) -> PathBuf {
    if target.is_absolute() {
        target.to_path_buf()
    } else {
        source
            .parent()
            .unwrap_or(Path::new("/"))
            .join(target)
    }
}

/// Validate a symlink target and return an error if it escapes the system layout.
fn validate_symlink_target(
    host_root: &Path,
    source: &Path,
    target: &Path,
) -> Result<(), String> {
    let resolved = resolve_symlink_target(source, target);

    // Check for recursive symlinks by following the chain with a bounded depth.
    let mut current = source.to_path_buf();
    let mut visited = Vec::new();
    let mut found_loop = false;

    for _ in 0..100 {
        if visited.contains(&current) {
            found_loop = true;
            break;
        }
        visited.push(current.clone());

        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let link_target = fs::read_link(&current)
                    .map_err(|error| format!("cannot read link: {error}"))?;
                current = resolve_symlink_target(&current, &link_target);
            }
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(format!("cannot inspect path: {error}"));
            }
        }
    }

    if found_loop {
        return Err(format!(
            "recursive symlink detected at `{}`",
            source.display()
        ));
    }

    // Check if target is within system layout
    if !is_within_system_layout(host_root, &resolved) {
        return Err(format!(
            "symlink target `{}` escapes read-only system layout",
            target.display()
        ));
    }

    Ok(())
}

fn add_entry(
    arguments: &mut Vec<String>,
    host_root: &Path,
    source: &Path,
    destination: &str,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "cannot inspect System path `{}`: {error}",
                source.display()
            ));
        }
    };
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source)
            .map_err(|error| format!("cannot read System link `{}`: {error}", source.display()))?;

        validate_symlink_target(host_root, source, &target)?;

        arguments.extend([
            "--symlink".to_owned(),
            target
                .to_str()
                .ok_or_else(|| format!("System link `{}` is not UTF-8", source.display()))?
                .to_owned(),
            destination.to_owned(),
        ]);
    } else if metadata.is_dir() || metadata.is_file() {
        arguments.extend([
            "--ro-bind".to_owned(),
            source
                .to_str()
                .ok_or_else(|| format!("System path `{}` is not UTF-8", source.display()))?
                .to_owned(),
            destination.to_owned(),
        ]);
    } else {
        return Err(format!(
            "System path `{}` is not a directory, regular file or link",
            source.display()
        ));
    }
    Ok(())
}

pub(crate) fn add_host_system_layout(arguments: &mut Vec<String>) -> Result<(), String> {
    add_layout(arguments, Path::new("/"))
}

fn add_layout(arguments: &mut Vec<String>, host: &Path) -> Result<(), String> {
    for relative in ["usr", "lib", "lib64", "bin", "sbin", "etc/alternatives"] {
        add_entry(arguments, host, &host.join(relative), &format!("/{relative}"))?;
    }
    arguments.extend(["--cap-drop".to_owned(), "ALL".to_owned()]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn live_layout_keeps_admin_aliases_read_only_without_enumerating_or_mutating_host() {
        let host = tempfile::tempdir().unwrap();
        fs::create_dir_all(host.path().join("usr/bin")).unwrap();
        fs::write(host.path().join("usr/bin/tool"), "compiler").unwrap();
        symlink("bin", host.path().join("usr/sbin")).unwrap();
        symlink("usr/sbin", host.path().join("sbin")).unwrap();
        let mut arguments = Vec::new();
        add_layout(&mut arguments, host.path()).unwrap();
        assert!(
            arguments
                .windows(3)
                .any(|args| args[0] == "--ro-bind" && args[2] == "/usr")
        );
        assert!(
            arguments
                .windows(3)
                .any(|args| args == ["--symlink", "usr/sbin", "/sbin"])
        );
        assert!(!arguments.iter().any(|arg| arg == "--tmpfs"));
        assert_eq!(
            fs::read_link(host.path().join("usr/sbin")).unwrap(),
            Path::new("bin")
        );
        assert_eq!(
            fs::read_to_string(host.path().join("usr/bin/tool")).unwrap(),
            "compiler"
        );
    }

    #[test]
    fn valid_symlink_within_system_layout_is_accepted() {
        let host = tempfile::tempdir().unwrap();
        fs::create_dir_all(host.path().join("usr/bin")).unwrap();
        fs::write(host.path().join("usr/bin/tool"), "compiler").unwrap();
        // Create a symlink within the read-only system layout
        symlink("../usr/bin/tool", host.path().join("usr/sbin/tool")).unwrap();
        let mut arguments = Vec::new();
        let result = add_entry(
            &mut arguments,
            host.path(),
            &host.path().join("usr/sbin/tool"),
            "/usr/sbin/tool",
        );
        assert!(
            result.is_ok(),
            "valid symlink within system layout should be accepted; got {:?}",
            result
        );
    }

    #[test]
    fn symlink_target_escaping_system_directories_is_rejected() {
        let host = tempfile::tempdir().unwrap();
        fs::create_dir_all(host.path().join("usr/bin")).unwrap();
        fs::create_dir_all(host.path().join("etc")).unwrap();
        // Create a symlink that escapes to a writable location (e.g., /tmp)
        symlink("/tmp", host.path().join("etc/escape")).unwrap();
        let mut arguments = Vec::new();
        // The symlink target /tmp is not within the read-only system layout,
        // so it should be rejected to prevent escape.
        let result = add_entry(
            &mut arguments,
            host.path(),
            &host.path().join("etc/escape"),
            "/etc/escape",
        );
        assert!(
            result.is_err(),
            "symlink escaping system directories should be rejected; got {:?}",
            result
        );
    }

    #[test]
    fn symlink_target_pointing_to_writable_grant_is_rejected() {
        let host = tempfile::tempdir().unwrap();
        fs::create_dir_all(host.path().join("usr/bin")).unwrap();
        fs::create_dir_all(host.path().join("etc")).unwrap();
        // Create a symlink that points to a writable grant (e.g., /home)
        symlink("/home", host.path().join("etc/writable")).unwrap();
        let mut arguments = Vec::new();
        // The symlink target /home is a writable grant, not a read-only system
        // directory, so it should be rejected.
        let result = add_entry(
            &mut arguments,
            host.path(),
            &host.path().join("etc/writable"),
            "/etc/writable",
        );
        assert!(
            result.is_err(),
            "symlink pointing to writable grant should be rejected; got {:?}",
            result
        );
    }

    #[test]
    fn recursive_symlink_in_writable_grant_is_detected() {
        let host = tempfile::tempdir().unwrap();
        fs::create_dir_all(host.path().join("usr/bin")).unwrap();
        fs::create_dir_all(host.path().join("etc")).unwrap();
        // Create a recursive symlink
        symlink("recursive", host.path().join("etc/recursive")).unwrap();
        let mut arguments = Vec::new();
        // The recursive symlink should be detected and rejected to prevent
        // infinite loops and potential escape.
        let result = add_entry(
            &mut arguments,
            host.path(),
            &host.path().join("etc/recursive"),
            "/etc/recursive",
        );
        assert!(
            result.is_err(),
            "recursive symlink should be rejected; got {:?}",
            result
        );
    }

    #[test]
    fn symlink_target_outside_host_root_is_rejected() {
        let host = tempfile::tempdir().unwrap();
        fs::create_dir_all(host.path().join("usr/bin")).unwrap();
        fs::create_dir_all(host.path().join("etc")).unwrap();
        // Create a symlink that points outside the host root
        symlink("/outside", host.path().join("etc/outside")).unwrap();
        let mut arguments = Vec::new();
        // The symlink target is outside the host root, so it should be rejected.
        let result = add_entry(
            &mut arguments,
            host.path(),
            &host.path().join("etc/outside"),
            "/etc/outside",
        );
        assert!(
            result.is_err(),
            "symlink pointing outside host root should be rejected; got {:?}",
            result
        );
    }
}
