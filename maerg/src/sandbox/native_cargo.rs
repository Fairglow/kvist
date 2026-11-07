use super::*;

#[path = "native_cargo/aliases.rs"]
mod aliases;

pub(super) const REGISTRY: &str = "/workspace/cargo-registry";
pub(super) const GIT: &str = "/workspace/cargo-git";
pub(super) const HOME: &str = "/workspace/scratch/cargo-home";
pub(super) const VERIFY_ARGS: [&str; 3] = ["test", "--offline", "--locked"];
const METADATA_ARGS: [&str; 6] = [
    "metadata",
    "--offline",
    "--locked",
    "--format-version",
    "1",
    "--all-features",
];

#[derive(Clone, Copy)]
pub(super) enum OfflineCommand {
    Test,
    Metadata,
}

impl OfflineCommand {
    pub(super) fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::Test => &VERIFY_ARGS,
            Self::Metadata => &METADATA_ARGS,
        }
    }
}

pub(super) enum Phase<'a> {
    Acquisition {
        sources: &'a [SandboxAllowedSource],
        lockfile_identity: &'a str,
    },
    Verification {
        command: OfflineCommand,
        lockfile_identity: &'a str,
    },
}

pub(super) struct Request<'a> {
    pub config: &'a SandboxConfig,
    pub runner_identity: &'a str,
    pub backend: &'a BackendIdentity,
    pub policy_identity: &'a str,
    pub toolchain_identity: &'a str,
    pub toolchain_root: &'a Path,
    pub cargo: &'a Path,
    pub runtime: &'a Path,
    pub scratch: &'a Path,
    pub workspace: &'a Path,
    pub registry: Option<&'a Path>,
    pub git: Option<&'a Path>,
    pub phase: Phase<'a>,
}

pub(super) fn data(config: &SandboxConfig, home: &Path, child: &str) -> Result<Option<PathBuf>> {
    let path = home.join(child);
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            if !rust_environment::inspect(config, &path)?.is_dir() {
                return Err(KvistError::SandboxUnavailable {
                    runner: config.runner.clone(),
                    reason: format!(
                        "native Cargo data `{}` must be a non-link directory",
                        path.display()
                    ),
                });
            }
            Ok(Some(path))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(sandbox_path_error(
            config,
            "inspect native Cargo data",
            &path,
            error,
        )),
    }
}

impl Request<'_> {
    pub(super) fn encode(self) -> Result<Vec<u8>> {
        let source =
            |path: &Path, context: &'static str| canonical_source_str(self.config, path, context);
        let toolchain = source(self.toolchain_root, "resolve native Cargo toolchain")?;
        let cargo = source(self.cargo, "resolve native Cargo executable")?;
        let relative = Path::new(&cargo)
            .strip_prefix(&toolchain)
            .map_err(|error| KvistError::SandboxUnavailable {
                runner: self.config.runner.clone(),
                reason: format!("Cargo `{cargo}` is not beneath `{toolchain}`: {error}"),
            })?;
        let cargo = format!("{CARGO_TOOLCHAIN_DEST}/{}", relative.display());
        let workspace = source(self.workspace, "resolve native Cargo workspace")?;
        let scratch = source(self.scratch, "resolve private Cargo scratch")?;
        let runtime = source(self.runtime, "resolve private Cargo runtime")?;
        let acquisition = matches!(self.phase, Phase::Acquisition { .. });
        let destination = if acquisition {
            ACQUISITION_LOCKFILE_DEST
        } else {
            CARGO_WORKING_DIRECTORY
        };
        let scratch_identity = digest_label(scratch.as_bytes());
        let workspace_identity = match &self.phase {
            Phase::Acquisition {
                lockfile_identity, ..
            }
            | Phase::Verification {
                lockfile_identity, ..
            } => (*lockfile_identity).to_owned(),
        };
        let mut grants = vec![
            SandboxGrant {
                source: toolchain,
                destination: CARGO_TOOLCHAIN_DEST.into(),
                access: "read-only",
                purpose: "toolchain",
                identity: self.toolchain_identity.into(),
            },
            SandboxGrant {
                source: runtime.clone(),
                destination: CARGO_RUNTIME_BIN_DEST.into(),
                access: "read-only",
                purpose: "runtime",
                identity: digest_label(runtime.as_bytes()),
            },
            SandboxGrant {
                source: scratch,
                destination: CARGO_SCRATCH_DEST.into(),
                access: "read-write",
                purpose: "scratch",
                identity: scratch_identity.clone(),
            },
            SandboxGrant {
                source: workspace,
                destination: destination.into(),
                access: if acquisition {
                    "read-write"
                } else {
                    "read-only"
                },
                purpose: if acquisition {
                    "lockfile"
                } else {
                    "verification"
                },
                identity: workspace_identity.clone(),
            },
        ];
        let mut endpoint = |path: Option<&Path>, destination: &str, purpose| {
            path.map(|path| -> Result<SandboxCacheEndpoint> {
                let source = source(path, "resolve native Cargo data endpoint")?;
                let identity = digest_label(source.as_bytes());
                grants.push(SandboxGrant {
                    source,
                    destination: destination.into(),
                    access: if acquisition {
                        "read-write"
                    } else {
                        "read-only"
                    },
                    purpose,
                    identity: identity.clone(),
                });
                Ok(SandboxCacheEndpoint {
                    destination: destination.into(),
                    identity,
                })
            })
            .transpose()
        };
        let registry = endpoint(self.registry, REGISTRY, "registry")?;
        let git = endpoint(self.git, GIT, "dependency-cache")?;
        let mut environment = BTreeMap::from([
            ("HOME".into(), format!("{CARGO_SCRATCH_DEST}/home")),
            ("PATH".into(), CARGO_RUNTIME_BIN_DEST.into()),
            ("CARGO_HOME".into(), HOME.into()),
            (
                "CARGO_TARGET_DIR".into(),
                format!("{CARGO_SCRATCH_DEST}/target"),
            ),
        ]);
        let (phase, network, lockfile, arguments) = match self.phase {
            Phase::Acquisition { sources, .. } => {
                environment.insert("CARGO_NET_GIT_FETCH_WITH_CLI".into(), "false".into());
                (
                    ExecutionPhase::DependencyAcquisition,
                    SandboxNetwork {
                        mode: "package-sources",
                        allowed_sources: sources.to_vec(),
                    },
                    Some(SandboxLockfileWorkspace {
                        destination: destination.into(),
                        before_identity: workspace_identity,
                    }),
                    vec![cargo.clone(), "fetch".into(), "--locked".into()],
                )
            }
            Phase::Verification { command, .. } => {
                environment.insert("CARGO_NET_OFFLINE".into(), "true".into());
                let mut arguments = vec![cargo.clone()];
                arguments.extend(
                    command
                        .arguments()
                        .iter()
                        .map(|argument| (*argument).to_owned()),
                );
                (
                    ExecutionPhase::Verification,
                    SandboxNetwork {
                        mode: "deny",
                        allowed_sources: Vec::new(),
                    },
                    None,
                    arguments,
                )
            }
        };
        let command =
            serde_json::to_vec(&arguments).map_err(|error| KvistError::SandboxUnavailable {
                runner: self.config.runner.clone(),
                reason: format!("cannot encode native Cargo command: {error}"),
            })?;
        let mounts =
            serde_json::to_vec(&grants).map_err(|error| KvistError::SandboxUnavailable {
                runner: self.config.runner.clone(),
                reason: format!("cannot encode native Cargo grants: {error}"),
            })?;
        let request = SandboxRequest {
            protocol: REQUEST_PROTOCOL,
            protocol_version: PROTOCOL_VERSION,
            phase: phase.wire(),
            argv: &arguments,
            working_directory: destination,
            environment: &environment,
            network,
            resources: SandboxResources {
                wall_time_ms: DEFAULT_WALL_TIME_MS,
                max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
                max_processes: DEFAULT_MAX_PROCESSES,
                max_files: DEFAULT_MAX_FILES,
                max_file_bytes: DEFAULT_MAX_FILE_BYTES,
                max_scratch_bytes: DEFAULT_MAX_SCRATCH_BYTES,
                max_cache_bytes: Some(DEFAULT_MAX_CARGO_CACHE_BYTES),
            },
            identities: SandboxIdentities {
                runner: self.runner_identity.into(),
                backend: SandboxBackend {
                    kind: self.backend.kind.clone(),
                    path: self.backend.path.clone(),
                    digest: self.backend.digest.clone(),
                },
                policy: self.policy_identity.into(),
                toolchain: self.toolchain_identity.into(),
                command: digest_label(&command),
                mount_plan: digest_label(&mounts),
            },
            grants: &grants,
            toolchain: SandboxToolchain::Cargo {
                identity: self.toolchain_identity.into(),
                root: CARGO_TOOLCHAIN_DEST.into(),
                cargo,
            },
            cache: Some(SandboxCache {
                cargo_home: HOME.into(),
                registry: REGISTRY.into(),
                git: GIT.into(),
                native: Some(SandboxNativeCache {
                    registry,
                    git,
                    registries: aliases::selected(self.config, self.workspace)?,
                }),
                writable: None,
                approved: None,
                lockfile,
                promotion: None,
            }),
            scratch: Some(SandboxScratch {
                destination: CARGO_SCRATCH_DEST,
                identity: scratch_identity,
            }),
        };
        serde_json::to_vec(&request).map_err(|error| KvistError::SandboxUnavailable {
            runner: self.config.runner.clone(),
            reason: format!("cannot encode native Cargo request: {error}"),
        })
    }
}

#[cfg(test)]
#[path = "native_cargo/tests.rs"]
mod tests;
