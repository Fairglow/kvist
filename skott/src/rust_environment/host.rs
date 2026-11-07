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

pub(super) fn operator_selector(path: &Path, workspace: &Path) -> Result<bool> {
    for parent in path.ancestors().filter_map(Path::parent) {
        if parent.starts_with(workspace) {
            return Ok(false);
        }
        match parent.canonicalize() {
            Ok(parent) if parent.starts_with(workspace) => return Ok(false),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(io_error(
                    "resolve host resource selector ownership",
                    None,
                    error,
                ));
            }
        }
    }
    Ok(true)
}

pub(super) fn select_rustup(candidates: &[PathBuf], workspace: &Path) -> Result<PathBuf> {
    for candidate in candidates {
        if !operator_selector(candidate, workspace)? {
            continue;
        }
        let rustup = match candidate.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(io_error(
                    "resolve trusted host Rustup launcher",
                    None,
                    error,
                ));
            }
        };
        if rustup.starts_with(workspace) {
            continue;
        }
        let metadata = inspect_path(&rustup)?;
        if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
            return Ok(rustup);
        }
    }
    Err(failure(
        "no ordinary executable Rustup launcher outside the write scope; provision a system or host-home installation, then retry the affected Rust command",
    ))
}

pub(super) fn resolve(
    workspace: &Path,
    pin: Option<&PinToolchain>,
    preparation: &Preparation<'_>,
) -> Result<Selection> {
    preparation.check()?;
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
    let rustup = select_rustup(
        &[
            PathBuf::from("/usr/bin/rustup"),
            home.join(".cargo/bin/rustup"),
        ],
        workspace,
    )?;
    if !operator_selector(&home.join(".rustup/toolchains"), workspace)? {
        return Err(failure(
            "host Rust registration selectors are writable in this scope; use a workspace outside the host installation selectors for Rust operations. Independent source tools remain usable",
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
            "Rust `{selected}` is unavailable; provision it on the host, then retry the affected Rust command without restarting independent workspace work: {error}"
        )),
    })?;
    let cargo = PathBuf::from(text.trim());
    let reported_root = cargo
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| failure("rustup returned an invalid concrete Cargo path"))?
        .to_owned();
    let registration_parent = home
        .join(".rustup/toolchains")
        .canonicalize()
        .map_err(|error| io_error("resolve installed registration parent", None, error))?;
    let reported_parent = reported_root
        .parent()
        .map(Path::canonicalize)
        .transpose()
        .map_err(|error| {
            io_error(
                "resolve reported installed registration parent",
                None,
                error,
            )
        })?;
    let installed_name = if reported_parent.as_ref() == Some(&registration_parent) {
        reported_root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| failure("installed Rust selection name must be UTF-8"))?
            .to_owned()
    } else {
        selected.clone()
    };
    channel(&installed_name)?;
    let root = registered_root(&home, workspace, &installed_name)?;
    let cargo = cargo
        .canonicalize()
        .map_err(|error| io_error("resolve installed Cargo executable", None, error))?;
    if cargo != root.join("bin/cargo") || !inspect_path(&cargo)?.is_file() {
        return Err(failure(
            "Cargo must be an ordinary executable in the selected host registration outside the writable workspace; no unregistered host root is granted",
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
