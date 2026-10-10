//! Bounded installed-toolchain enumeration and runtime-layout drift evidence.

use super::*;

#[derive(Debug)]
pub(super) struct Installation {
    pub name: String,
    pub root: PathBuf,
    pub destination: String,
}

#[derive(Debug)]
pub(super) struct Inventory {
    pub installations: Vec<Installation>,
    pub fingerprints: Vec<(PathBuf, Fingerprint)>,
}

pub(super) fn resolve(
    home: &Path,
    workspace: &Path,
    selected: &str,
    rustup: &Path,
    preparation: &Preparation<'_>,
    executables: &mut Vec<TrackedExecutable>,
) -> Result<Inventory> {
    let parent = home.join(".rustup/toolchains");
    let parent_identity = fingerprint(&inspect_path(&parent)?);
    let mut names = query_rustup(rustup, home, &["toolchain", "list"], preparation)?
        .lines()
        .map(|line| {
            line.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    if names.is_empty() || names.len() > 128 || names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(failure(
            "Rust inventory must contain 1..128 distinct installed toolchains",
        ));
    }
    if !names.iter().any(|name| name == selected) {
        return Err(failure(
            "selected Rust installation is absent from the host inventory",
        ));
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&parent)
        .map_err(|e| io_error("inspect Rust installation inventory", None, e))?
    {
        preparation.check()?;
        let entry = entry.map_err(|e| io_error("read Rust installation inventory", None, e))?;
        entries.push(
            entry
                .file_name()
                .into_string()
                .map_err(|_| failure("installed Rust names must be UTF-8"))?,
        );
        if entries.len() > 128 {
            return Err(failure("Rust installation directory exceeds 128 entries"));
        }
    }
    entries.sort();
    if entries != names {
        return Err(failure(
            "rustup inventory differs from the installation directory; repair or finish host provisioning before startup",
        ));
    }
    let mut inventory = Inventory {
        installations: Vec::new(),
        fingerprints: vec![(parent.clone(), parent_identity)],
    };
    let mut retained_bytes = 0;
    for (index, name) in names.into_iter().enumerate() {
        preparation.check()?;
        channel(&name)?;
        let root = parent.join(&name);
        if root.starts_with(workspace) || workspace.starts_with(&root) {
            return Err(failure(
                "installed Rust inventory overlaps the writable workspace",
            ));
        }
        if !inspect_path(&root)?.is_dir() {
            return Err(failure(
                "every installed Rust toolchain must be a non-link directory; repair it on the host",
            ));
        }
        if name != selected {
            for executable in ["cargo", "rustc", "rustdoc"] {
                executables.push(track_executable(
                    root.join("bin").join(executable),
                    preparation,
                )?);
            }
            for executable in ["rustfmt", "cargo-fmt", "cargo-clippy", "clippy-driver"] {
                let path = root.join("bin").join(executable);
                match fs::symlink_metadata(&path) {
                    Ok(_) => executables.push(track_executable(path, preparation)?),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(io_error("inspect installed Rust companion", None, error));
                    }
                }
            }
            for path in native_library_layout(&root)? {
                let (identity, fingerprint) = hash_file(&path, preparation)?;
                executables.push(TrackedExecutable {
                    path,
                    identity,
                    fingerprint,
                });
            }
        }
        inventory
            .fingerprints
            .push((root.clone(), fingerprint(&inspect_path(&root)?)));
        for path in [root.join("bin"), root.join("lib")] {
            track_tree(
                &path,
                &mut inventory.fingerprints,
                &mut retained_bytes,
                preparation,
            )?;
        }
        let destination = if name == selected {
            TOOLCHAIN_DEST.into()
        } else {
            format!("/rust/toolchains/{index}")
        };
        inventory.installations.push(Installation {
            name,
            root,
            destination,
        });
    }
    validate(&inventory, preparation)?;
    Ok(inventory)
}

fn track_tree(
    path: &Path,
    tracked: &mut Vec<(PathBuf, Fingerprint)>,
    retained_bytes: &mut usize,
    preparation: &Preparation<'_>,
) -> Result<()> {
    preparation.check()?;
    if path.components().count() > 128 {
        return Err(failure(
            "installed Rust runtime layout exceeds its depth bound",
        ));
    }
    *retained_bytes += path.as_os_str().len();
    if tracked.len() >= MAX_ENTRIES || *retained_bytes > 32 << 20 {
        return Err(failure(
            "installed Rust inventory exceeds 100,000 entries or 32 MiB of paths",
        ));
    }
    let metadata = inspect_path(path)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(failure(
            "installed Rust runtime layout contains a special entry",
        ));
    }
    tracked.push((path.to_owned(), fingerprint(&metadata)));
    if metadata.is_dir() {
        let mut entries = Vec::new();
        for entry in
            fs::read_dir(path).map_err(|e| io_error("enumerate installed Rust runtime", None, e))?
        {
            preparation.check()?;
            entries.push(
                entry
                    .map_err(|e| io_error("read installed Rust runtime entry", None, e))?
                    .path(),
            );
            if entries.len() > MAX_ENTRIES {
                return Err(failure(
                    "installed Rust runtime directory exceeds its entry bound",
                ));
            }
        }
        entries.sort();
        for entry in entries {
            track_tree(&entry, tracked, retained_bytes, preparation)?;
        }
        if fingerprint(&inspect_path(path)?) != fingerprint(&metadata) {
            return Err(failure(
                "installed Rust runtime changed while preparing; restart after provisioning",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate(inventory: &Inventory, preparation: &Preparation<'_>) -> Result<()> {
    for (path, expected) in &inventory.fingerprints {
        preparation.check()?;
        if fingerprint(&inspect_path(path)?) != *expected {
            return Err(failure(
                "installed Rust inventory or target layout drifted since startup; restart after host provisioning",
            ));
        }
    }
    Ok(())
}
