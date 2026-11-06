//! Installed toolchain selection shared by provisioning and sandbox preparation.

use super::*;

pub(super) struct Selection {
    pub rustup: PathBuf,
    pub home: PathBuf,
    pub selected: String,
    pub cargo: PathBuf,
    pub root: PathBuf,
    pub installed_name: String,
}

pub(super) fn resolve(
    workspace: &Path,
    pin: Option<&PinToolchain>,
    preparation: &Preparation<'_>,
) -> Result<Selection> {
    preparation.check()?;
    let rustup = Path::new("/usr/bin/rustup")
        .canonicalize()
        .map_err(|error| {
            io_error(
                "resolve system rustup; provision Rust on the host",
                None,
                error,
            )
        })?;
    if !rustup.starts_with("/usr")
        || !inspect_path(&rustup)?.is_file()
        || rustup.starts_with(workspace)
    {
        return Err(failure(
            "rustup must be a trusted system executable outside the writable workspace",
        ));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| failure("HOME is required to resolve installed host Rust"))?
        .canonicalize()
        .map_err(|error| io_error("canonicalize HOME for Rust resolution", None, error))?;
    if !inspect_path(&home)?.is_dir() || home.starts_with(workspace) {
        return Err(failure(
            "host HOME must be a canonical non-link directory outside the writable workspace",
        ));
    }
    let selected = match pin {
        Some(pin) => pin.channel.clone(),
        None => query_rustup(&rustup, &home, &["show", "active-toolchain"], preparation)?
            .split_whitespace()
            .next()
            .ok_or_else(|| failure("rustup returned no installed default selection"))?
            .to_owned(),
    };
    channel(&selected)?;
    let text = query_rustup(
        &rustup,
        &home,
        &["which", "--toolchain", &selected, "cargo"],
        preparation,
    )
    .map_err(|error| match error {
        Error::Cancelled => Error::Cancelled,
        error => failure(format!(
            "Rust `{selected}` is unavailable; provision it on the host before startup: {error}"
        )),
    })?;
    let cargo = PathBuf::from(text.trim());
    let root = cargo
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| failure("rustup returned an invalid concrete Cargo path"))?
        .to_owned();
    let installed_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| failure("installed Rust selection name must be UTF-8"))?
        .to_owned();
    channel(&installed_name)?;
    if root.parent() != Some(home.join(".rustup/toolchains").as_path())
        || cargo != root.join("bin/cargo")
        || root.starts_with(workspace)
        || !inspect_path(&cargo)?.is_file()
    {
        return Err(failure(
            "Cargo must be an exact installed toolchain executable outside the writable workspace, not a custom linked or substituted root",
        ));
    }
    Ok(Selection {
        rustup,
        home,
        selected,
        cargo,
        root,
        installed_name,
    })
}
