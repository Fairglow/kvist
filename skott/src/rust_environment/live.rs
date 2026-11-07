//! Cheap topology observations, not mounted-content integrity checks.

use super::*;

pub(crate) struct Cache {
    enabled: bool,
    environment: Option<RustEnvironment>,
    topology: Option<Topology>,
    selected_root: Option<PathBuf>,
}

impl Cache {
    pub(crate) fn new(enabled: bool, environment: Option<RustEnvironment>) -> Self {
        let topology = environment
            .as_ref()
            .map(|environment| environment.resources.topology.clone());
        let selected_root = environment
            .as_ref()
            .map(|environment| environment.resources.root.clone());
        Self {
            enabled,
            environment,
            topology,
            selected_root,
        }
    }

    pub(crate) fn refresh(
        &mut self,
        workspace: &Path,
        cancellation: &CancellationToken,
    ) -> Result<(Option<RustEnvironment>, Option<String>)> {
        crate::executor::check_cancelled(cancellation)?;
        if !self.enabled {
            return Ok((None, None));
        }
        let mut topology = Topology::capture(workspace)?;
        if let Some(root) = &self.selected_root
            && !root.starts_with(workspace)
            && !workspace.starts_with(root)
        {
            topology.observe_root(root)?;
        }
        if self.topology.as_ref() == Some(&topology) {
            return Ok((self.environment.clone(), None));
        }
        let was_unavailable = self.environment.is_none();
        let previous_root = self.selected_root.clone();
        match RustEnvironment::resolve_observed(workspace, cancellation, |root| {
            self.selected_root = Some(root.to_owned());
        }) {
            Ok(environment) => {
                let diagnostic = if was_unavailable {
                    Some(format!(
                        "skott: Rust resources are available for this invocation: {}\n",
                        environment.diagnostic()
                    ))
                } else if !environment.resources.issues.is_empty() {
                    Some(format!(
                        "skott: {}\n",
                        environment.resources.issues.join("\n")
                    ))
                } else {
                    None
                };
                self.topology = Some(environment.resources.topology.clone());
                self.environment = Some(environment.clone());
                Ok((Some(environment), diagnostic))
            }
            Err(error) => {
                crate::executor::check_cancelled(cancellation)?;
                self.topology = if previous_root == self.selected_root {
                    Some(topology)
                } else {
                    None
                };
                self.environment = None;
                tracing::warn!(%error, "live Rust resources unavailable; independent tools remain usable");
                Ok((
                    None,
                    Some(format!(
                        "skott: Rust integration unavailable: {error}. Independent source/workspace tools remain usable. Repair or provision the selected resource on the host; relevant host changes trigger another preparation without restart or automatic replay.\n"
                    )),
                ))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Topology(Vec<(PathBuf, Option<Fingerprint>)>);

impl Topology {
    pub(super) fn capture(workspace: &Path) -> Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| failure("HOME is required to discover live Rust resources"))?;
        let paths = [
            PathBuf::from("/usr/bin/rustup"),
            PathBuf::from("/usr/bin/cargo"),
            PathBuf::from("/usr/bin/rustc"),
            workspace.join("rust-toolchain"),
            workspace.join("rust-toolchain.toml"),
            workspace.join(".kvist"),
            workspace.join(".kvist/vendored"),
            workspace.join(".kvist/sandbox-cargo"),
            workspace.join(".kvist/sandbox-cargo/config.toml"),
            home.join(".rustup"),
            home.join(".rustup/settings.toml"),
            home.join(".rustup/toolchains"),
            home.join(".cargo/bin"),
            home.join(".cargo/bin/rustup"),
        ];
        let mut observations = Vec::with_capacity(paths.len());
        let mut blocked = Vec::<PathBuf>::new();
        for path in paths {
            if blocked.iter().any(|parent| path.starts_with(parent)) {
                observations.push((path, None));
                continue;
            }
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        blocked.push(path.clone());
                    }
                    Some(fingerprint(&metadata))
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound
                            | std::io::ErrorKind::NotADirectory
                            | std::io::ErrorKind::PermissionDenied
                    ) =>
                {
                    None
                }
                Err(error) => {
                    return Err(io_error(
                        "observe live Rust resource topology",
                        Some(&path.to_string_lossy()),
                        error,
                    ));
                }
            };
            observations.push((path, metadata));
        }

        Ok(Self(observations))
    }

    pub(super) fn observe_root(&mut self, root: &Path) -> Result<()> {
        let mut blocked = false;
        for path in [
            root.to_owned(),
            root.join("bin"),
            root.join("bin/cargo"),
            root.join("bin/rustc"),
        ] {
            let metadata = if blocked {
                None
            } else {
                match fs::symlink_metadata(&path) {
                    Ok(metadata) => {
                        if metadata.file_type().is_symlink() {
                            blocked = true;
                        }
                        Some(fingerprint(&metadata))
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::NotFound
                                | std::io::ErrorKind::NotADirectory
                                | std::io::ErrorKind::PermissionDenied
                        ) =>
                    {
                        None
                    }
                    Err(error) => {
                        return Err(io_error(
                            "observe selected Rust registration topology",
                            None,
                            error,
                        ));
                    }
                }
            };
            self.0.push((path, metadata));
        }
        Ok(())
    }
}
