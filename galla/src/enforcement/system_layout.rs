//! Read-only System views excluding standard administrative directories.

use std::fs;
use std::path::{Path, PathBuf};

const MAX_DIRECTORY_ENTRIES: usize = 1024;

fn inaccessible_directory(arguments: &mut Vec<String>, destination: &str) {
    arguments.extend(
        [
            "--perms",
            "000",
            "--tmpfs",
            destination,
            "--remount-ro",
            destination,
        ]
        .map(str::to_owned),
    );
}

fn filtered_directory(
    arguments: &mut Vec<String>,
    source: &Path,
    destination: &str,
) -> Result<(), String> {
    let mut children = Vec::<PathBuf>::new();
    match fs::symlink_metadata(source) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            for entry in fs::read_dir(source).map_err(|error| {
                format!(
                    "cannot enumerate System directory `{}`: {error}",
                    source.display()
                )
            })? {
                let entry = entry.map_err(|error| {
                    format!(
                        "cannot inspect System directory `{}`: {error}",
                        source.display()
                    )
                })?;
                if children.len() == MAX_DIRECTORY_ENTRIES {
                    return Err(format!(
                        "System directory `{}` exceeds {MAX_DIRECTORY_ENTRIES} entries",
                        source.display()
                    ));
                }
                children.push(entry.path());
            }
        }
        Ok(_) => {
            return Err(format!(
                "filtered System directory `{}` must be a non-link directory",
                source.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "cannot inspect System directory `{}`: {error}",
                source.display()
            ));
        }
    }
    children.sort();
    arguments.extend(["--perms", "0755", "--tmpfs", destination].map(str::to_owned));
    for child in children {
        let name = child
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("System entry `{}` is not UTF-8", child.display()))?;
        if name == "sbin" || (destination == "/usr" && name == "local") {
            continue;
        }
        add_entry(arguments, &child, &format!("{destination}/{name}"))?;
    }
    if destination == "/usr" {
        filtered_directory(arguments, &source.join("local"), "/usr/local")?;
    }
    inaccessible_directory(arguments, &format!("{destination}/sbin"));
    arguments.extend(["--remount-ro".to_owned(), destination.to_owned()]);
    Ok(())
}

fn add_entry(arguments: &mut Vec<String>, source: &Path, destination: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        format!(
            "cannot inspect System entry `{}`: {error}",
            source.display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source)
            .map_err(|error| format!("cannot read System link `{}`: {error}", source.display()))?;
        let target = target
            .to_str()
            .ok_or_else(|| format!("System link `{}` has a non-UTF-8 target", source.display()))?;
        arguments.extend([
            "--symlink".to_owned(),
            target.to_owned(),
            destination.to_owned(),
        ]);
    } else if metadata.is_dir() || metadata.is_file() {
        let source = source
            .to_str()
            .ok_or_else(|| format!("System source `{}` is not UTF-8", source.display()))?;
        arguments.extend([
            "--ro-bind".to_owned(),
            source.to_owned(),
            destination.to_owned(),
        ]);
    } else {
        return Err(format!(
            "System entry `{}` is not a directory, regular file or link",
            source.display()
        ));
    }
    Ok(())
}

pub(super) fn add_host_system_layout(arguments: &mut Vec<String>) -> Result<(), String> {
    add_layout(arguments, Path::new("/"))
}

fn add_layout(arguments: &mut Vec<String>, host: &Path) -> Result<(), String> {
    filtered_directory(arguments, &host.join("usr"), "/usr")?;
    for relative in ["lib", "lib64", "bin", "etc/alternatives"] {
        let source = host.join(relative);
        match fs::symlink_metadata(&source) {
            Ok(_) => add_entry(arguments, &source, &format!("/{relative}"))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot inspect System path `{}`: {error}",
                    source.display()
                ));
            }
        }
    }
    inaccessible_directory(arguments, "/sbin");
    arguments.extend(["--cap-drop".to_owned(), "ALL".to_owned()]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn filtered_layout_preserves_bins_without_following_admin_aliases_or_mutating_host() {
        let host = tempfile::tempdir().unwrap();
        fs::create_dir_all(host.path().join("usr/bin")).unwrap();
        fs::create_dir_all(host.path().join("usr/local/bin")).unwrap();
        fs::create_dir_all(host.path().join("usr/local/sbin")).unwrap();
        fs::write(host.path().join("usr/bin/tool"), "compiler").unwrap();
        symlink("bin", host.path().join("usr/sbin")).unwrap();
        symlink("usr/sbin", host.path().join("sbin")).unwrap();
        let mut arguments = Vec::new();
        add_layout(&mut arguments, host.path()).unwrap();
        for destination in ["/sbin", "/usr/sbin", "/usr/local/sbin"] {
            assert!(arguments.windows(6).any(|args| args
                == [
                    "--perms",
                    "000",
                    "--tmpfs",
                    destination,
                    "--remount-ro",
                    destination
                ]));
            assert!(!arguments.windows(3).any(|args| {
                (args[0] == "--ro-bind" || args[0] == "--symlink") && args[2] == destination
            }));
        }
        assert!(
            arguments
                .windows(3)
                .any(|args| args[0] == "--ro-bind" && args[2] == "/usr/bin")
        );
        assert!(
            arguments
                .windows(3)
                .any(|args| args[0] == "--ro-bind" && args[2] == "/usr/local/bin")
        );
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
    fn missing_local_root_is_namespace_only_and_linked_roots_fail_closed() {
        let host = tempfile::tempdir().unwrap();
        fs::create_dir(host.path().join("usr")).unwrap();
        add_layout(&mut Vec::new(), host.path()).unwrap();
        assert!(!host.path().join("usr/local").exists());
        symlink("bin", host.path().join("usr/local")).unwrap();
        assert!(add_layout(&mut Vec::new(), host.path()).is_err());
    }

    #[test]
    fn system_directory_enumeration_is_bounded() {
        let host = tempfile::tempdir().unwrap();
        fs::create_dir(host.path().join("usr")).unwrap();
        for index in 0..=MAX_DIRECTORY_ENTRIES {
            fs::write(host.path().join(format!("usr/{index}")), "").unwrap();
        }
        assert!(
            add_layout(&mut Vec::new(), host.path())
                .unwrap_err()
                .contains("exceeds")
        );
        fs::remove_file(host.path().join(format!("usr/{MAX_DIRECTORY_ENTRIES}"))).unwrap();
        add_layout(&mut Vec::new(), host.path()).unwrap();
    }
}
