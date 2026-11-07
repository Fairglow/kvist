//! Installed registration topology. Mounted contents remain live.

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
    pub diagnostics: Vec<String>,
}

pub(super) fn resolve(
    home: &Path,
    workspace: &Path,
    selected: &str,
    rustup: &Path,
    preparation: &Preparation<'_>,
) -> Result<Inventory> {
    let parent = home.join(".rustup/toolchains");
    let mut diagnostics = Vec::new();
    let listing = match query_rustup(rustup, home, &["toolchain", "list"], preparation) {
        Ok(listing) => listing,
        Err(error) => {
            preparation.check()?;
            diagnostics.push(format!("optional installed-registration listing unavailable: {error}; the selected compiler remains usable. Select another installed name in the project pin to include it on the next invocation"));
            String::new()
        }
    };
    let mut names = listing
        .lines()
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect::<Vec<_>>();
    names.push(selected.to_owned());
    names.sort();
    names.dedup();
    names.sort_by_key(|name| (name != selected, name.clone()));
    if names.len() > 128 {
        diagnostics.push("installed registrations exceed this invocation's mount-plan budget; the selected compiler and 127 other registrations remain available. Select an omitted host registration in the project pin to include it next time; no installation/restart is needed".into());
        names.truncate(128);
    }
    let mut installations = Vec::with_capacity(names.len());
    for (index, name) in names.into_iter().enumerate() {
        preparation.check()?;
        if let Err(error) = channel(&name) {
            if name == selected {
                return Err(error);
            }
            diagnostics.push(format!("optional registration `{name}` is unusable: {error}; selected compiler remains available"));
            continue;
        }
        let root = parent.join(&name);
        match fs::symlink_metadata(&root) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && name != selected => {
                tracing::warn!(%name, "installed registration disappeared during discovery; other tools remain usable");
                continue;
            }
            Err(error) if name != selected => {
                diagnostics.push(format!("optional registration `{name}` could not be inspected: {error}; selected compiler remains available"));
                continue;
            }
            Err(error) => {
                return Err(io_error(
                    "inspect installed Rust registration",
                    Some(&root.to_string_lossy()),
                    error,
                ));
            }
        }
        let root = match registered_root(home, workspace, &name) {
            Ok(root) => root,
            Err(error) if name != selected => {
                diagnostics.push(format!("optional registration `{name}` is unusable: {error}; selected compiler remains available"));
                continue;
            }
            Err(error) => return Err(error),
        };
        installations.push(Installation {
            destination: if name == selected {
                TOOLCHAIN_DEST.into()
            } else {
                format!("/rust/toolchains/{index}")
            },
            name,
            root,
        });
    }
    if !installations
        .iter()
        .any(|installation| installation.name == selected)
    {
        return Err(failure(
            "selected Rust installation is unavailable; provision it on the host, then retry the affected command",
        ));
    }
    Ok(Inventory {
        installations,
        diagnostics,
    })
}
