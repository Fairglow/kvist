//! Live read-only System views. Names do not grant host privileges.

use std::fs;
use std::path::Path;

fn add_entry(arguments: &mut Vec<String>, source: &Path, destination: &str) -> Result<(), String> {
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
        add_entry(arguments, &host.join(relative), &format!("/{relative}"))?;
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
}
