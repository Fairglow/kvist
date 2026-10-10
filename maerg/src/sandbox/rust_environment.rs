//! Installed-only Rust resources for protected authoring, not Cargo verification.

use super::*;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

pub(super) type Fingerprint = (u64, u64, u64, i64, i64, i64, i64);

pub(super) struct RustEnvironment {
    _staging: tempfile::TempDir,
    grants: Vec<SandboxGrant>,
    selected: String,
    pinned: bool,
    tracked: Vec<(PathBuf, Fingerprint)>,
    component: PathBuf,
    build_directory: String,
}

fn failure(config: &SandboxConfig, reason: impl Into<String>) -> KvistError {
    KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: reason.into(),
    }
}

pub(super) fn inspect(config: &SandboxConfig, path: &Path) -> Result<fs::Metadata> {
    if !path.is_absolute() || path.to_str().is_none() {
        return Err(failure(
            config,
            "Rust resources must have absolute UTF-8 paths",
        ));
    }
    let mut prefix = PathBuf::from("/");
    for part in path.components() {
        match part {
            std::path::Component::RootDir => continue,
            std::path::Component::Normal(name) => prefix.push(name),
            _ => return Err(failure(config, "Rust resource path must be canonical")),
        }
        let metadata = fs::symlink_metadata(&prefix)
            .map_err(|e| sandbox_error(config, "inspect installed Rust resource", e))?;
        if metadata.is_symlink() {
            return Err(failure(
                config,
                "Rust resources and ancestors must not be symbolic links",
            ));
        }
    }
    fs::symlink_metadata(path).map_err(|e| sandbox_error(config, "inspect Rust resource", e))
}

pub(super) fn fingerprint(metadata: &fs::Metadata) -> Fingerprint {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

pub(super) fn check_budget(config: &SandboxConfig, started: Instant) -> Result<()> {
    if started.elapsed() >= Duration::from_secs(30) {
        return Err(failure(
            config,
            "installed Rust preparation exceeded 30 seconds; narrow host material",
        ));
    }
    Ok(())
}

fn query(config: &SandboxConfig, home: &Path, args: &[&str], started: Instant) -> Result<String> {
    check_budget(config, started)?;
    let remaining = Duration::from_secs(30).saturating_sub(started.elapsed());
    let policy = sav::SupervisionPolicy {
        idle_timeout: remaining,
        attempt_timeout: Some(remaining),
        detect_loops: false,
        max_retries: 0,
        max_output_bytes: 16384,
    };
    let mut arguments = vec![
        "--ignore-environment".into(),
        format!("HOME={}", home.display()),
        "PATH=/usr/bin:/bin".into(),
        "RUSTUP_AUTO_INSTALL=0".into(),
        "/usr/bin/rustup".into(),
    ];
    arguments.extend(args.iter().map(|value| (*value).to_owned()));
    let output = sav::run_supervised_capture(&policy, |_| {
        Ok(sav::CommandSpec::new("/usr/bin/env", arguments.clone()).in_directory("/"))
    })
    .map_err(|e| {
        failure(
            config,
            format!("cannot query installed Rust; provision on the host: {e}"),
        )
    })?;
    String::from_utf8(output.stdout)
        .map_err(|_| failure(config, "installed Rust query must return UTF-8"))
}

fn track_tree(
    config: &SandboxConfig,
    path: &Path,
    tracked: &mut Vec<(PathBuf, Fingerprint)>,
    path_bytes: &mut usize,
    started: Instant,
    depth: usize,
) -> Result<()> {
    check_budget(config, started)?;
    *path_bytes += path.as_os_str().len();
    if tracked.len() >= 100_000 || *path_bytes > 32 << 20 || depth > 128 {
        return Err(failure(
            config,
            "installed Rust inventory exceeds its entry/path/depth bounds",
        ));
    }
    let metadata = inspect(config, path)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(failure(
            config,
            "installed Rust runtime contains a special entry",
        ));
    }
    tracked.push((path.into(), fingerprint(&metadata)));
    if metadata.is_dir() {
        for entry in fs::read_dir(path)
            .map_err(|e| sandbox_error(config, "enumerate installed Rust runtime", e))?
        {
            let entry =
                entry.map_err(|e| sandbox_error(config, "read installed Rust runtime entry", e))?;
            track_tree(
                config,
                &entry.path(),
                tracked,
                path_bytes,
                started,
                depth + 1,
            )?;
        }
    }
    Ok(())
}

impl RustEnvironment {
    pub(super) fn prepare(
        config: &SandboxConfig,
        project: &Path,
        component: &Path,
    ) -> Result<Self> {
        let started = Instant::now();
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| failure(config, "HOME is required to resolve installed Rust"))?;
        let parent = home.join(".rustup/toolchains");
        let parent_identity = fingerprint(&inspect(config, &parent)?);
        let rustup = Path::new("/usr/bin/rustup").canonicalize().map_err(|e| {
            sandbox_error(
                config,
                "resolve trusted system rustup; provision on the host",
                e,
            )
        })?;
        if !rustup.starts_with("/usr") || !inspect(config, &rustup)?.is_file() {
            return Err(failure(
                config,
                "Rust requires a trusted system rustup executable",
            ));
        }
        let mut pin = crate::toolchain::read_toolchain_pin(project)?;
        if !pin.pinned && component != project {
            pin = crate::toolchain::read_toolchain_pin(component)?;
        }
        let requested = if pin.pinned {
            pin.channel.clone()
        } else {
            query(config, &home, &["default"], started)?
                .split_whitespace()
                .next()
                .ok_or_else(|| failure(config, "host Rust has no installed default"))?
                .to_owned()
        };
        let cargo = query(
            config,
            &home,
            &["which", "--toolchain", &requested, "cargo"],
            started,
        )?;
        let selected = cargo_toolchain_from_path(cargo.trim(), &config.runner)?;
        if selected.root.parent() != Some(parent.as_path()) {
            return Err(failure(
                config,
                "selected Rust must be a standard installed toolchain, not a custom link",
            ));
        }
        let selected_name = selected
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| failure(config, "installed Rust name must be UTF-8"))?
            .to_owned();
        let mut names: Vec<_> = query(config, &home, &["toolchain", "list"], started)?
            .lines()
            .map(|line| {
                line.split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect();
        names.sort();
        if names.is_empty()
            || names.len() > 128
            || names.windows(2).any(|pair| pair[0] == pair[1])
            || !names.contains(&selected_name)
        {
            return Err(failure(
                config,
                "Rust inventory must contain 1..128 distinct installed names including the selection",
            ));
        }
        let entries = fs::read_dir(&parent)
            .map_err(|e| sandbox_error(config, "inspect host Rust inventory", e))?;
        let mut actual = Vec::new();
        for entry in entries {
            check_budget(config, started)?;
            actual.push(
                entry
                    .map_err(|e| sandbox_error(config, "read host Rust inventory", e))?
                    .file_name()
                    .into_string()
                    .map_err(|_| failure(config, "Rust names must be UTF-8"))?,
            );
            if actual.len() > 128 {
                return Err(failure(config, "host Rust inventory exceeds 128 entries"));
            }
        }
        actual.sort();
        if actual != names {
            return Err(failure(
                config,
                "rustup and host installation inventory differ; finish host provisioning",
            ));
        }
        let staging = tempfile::Builder::new()
            .prefix(".maerg-rust-")
            .tempdir_in(
                project
                    .parent()
                    .ok_or_else(|| failure(config, "Rust staging needs a workspace parent"))?,
            )
            .map_err(|e| sandbox_error(config, "stage private authoring Rust resources", e))?;
        fs::create_dir_all(staging.path().join("runtime/bin"))
            .and_then(|_| fs::create_dir_all(staging.path().join("rustup-home/toolchains")))
            .map_err(|e| sandbox_error(config, "create private Rust runtime", e))?;
        fs::write(
            staging.path().join("runtime/initialize"),
            sav::offline_rust::INITIALIZE,
        )
        .map_err(|e| sandbox_error(config, "write sandbox-only Rust initialization", e))?;
        for tool in sav::offline_rust::TOOLS {
            let path = staging.path().join("runtime/bin").join(tool.name());
            fs::write(&path, sav::offline_rust::wrapper(*tool))
                .and_then(|_| fs::set_permissions(&path, fs::Permissions::from_mode(0o500)))
                .map_err(|e| sandbox_error(config, "write fixed authoring Rust wrapper", e))?;
        }
        let mut grants = Vec::new();
        let mut tracked = vec![(parent.clone(), parent_identity)];
        let mut path_bytes = 0;
        for (index, name) in names.iter().enumerate() {
            if name.len() > 256
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                || name.starts_with('-')
                || !name.contains('-')
            {
                return Err(failure(config, "invalid installed Rust toolchain name"));
            }
            let root = parent.join(name);
            if root.starts_with(project) || project.starts_with(&root) {
                return Err(failure(config, "installed Rust overlaps the project"));
            }
            let metadata = inspect(config, &root)?;
            if !metadata.is_dir() {
                return Err(failure(
                    config,
                    "installed Rust root must be a non-link directory",
                ));
            }
            tracked.push((root.clone(), fingerprint(&metadata)));
            let manifest = root.join("lib/rustlib/multirust-channel-manifest.toml");
            let metadata = inspect(config, &manifest)?;
            if !metadata.is_file() || metadata.len() > 256 << 20 {
                return Err(failure(
                    config,
                    "installed Rust requires a regular rustup target/component manifest no larger than 256 MiB",
                ));
            }
            let mut hash = Sha256::new();
            for tool in ["cargo", "rustc", "rustdoc"] {
                let path = root.join("bin").join(tool);
                let metadata = inspect(config, &path)?;
                if !metadata.is_file() || !is_executable(&metadata) || metadata.len() > 256 << 20 {
                    return Err(failure(
                        config,
                        "installed Rust requires bounded regular compiler/Cargo/doc executables",
                    ));
                }
                let mut file = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
                    .open(&path)
                    .map_err(|e| sandbox_error(config, "hash installed Rust executable", e))?;
                let opened = file
                    .metadata()
                    .map_err(|e| sandbox_error(config, "inspect opened Rust executable", e))?;
                if fingerprint(&metadata) != fingerprint(&opened) || !opened.is_file() {
                    return Err(failure(config, "Rust executable changed while opening"));
                }
                let mut buffer = [0; 65536];
                let mut size = 0_u64;
                loop {
                    check_budget(config, started)?;
                    let count = file
                        .read(&mut buffer)
                        .map_err(|e| sandbox_error(config, "read installed Rust executable", e))?;
                    if count == 0 {
                        break;
                    }
                    size += count as u64;
                    if size > 256 << 20 {
                        return Err(failure(config, "Rust executable exceeds 256 MiB"));
                    }
                    hash.update(&buffer[..count]);
                }
                if fingerprint(&metadata) != fingerprint(&inspect(config, &path)?) {
                    return Err(failure(config, "Rust executable changed while hashing"));
                }
            }
            for child in ["bin", "lib"] {
                track_tree(
                    config,
                    &root.join(child),
                    &mut tracked,
                    &mut path_bytes,
                    started,
                    0,
                )?;
            }
            let destination = format!("/rust/toolchains/{index}");
            symlink(
                &destination,
                staging.path().join("rustup-home/toolchains").join(name),
            )
            .map_err(|e| sandbox_error(config, "register sandbox-native Rust installation", e))?;
            grants.push(SandboxGrant {
                source: root.to_string_lossy().into_owned(),
                destination,
                access: "read-only",
                purpose: "toolchain",
                identity: format!("sha256:{}", hex::encode(hash.finalize())),
            });
        }
        fs::write(
            staging.path().join("rustup-home/settings.toml"),
            format!(
                "version = \"12\"\ndefault_toolchain = \"{selected_name}\"\nprofile = \"minimal\"\n"
            ),
        )
        .map_err(|e| sandbox_error(config, "write generated private Rust settings", e))?;
        for resource in ["runtime", "rustup-home"] {
            grants.push(SandboxGrant {
                source: staging.path().join(resource).to_string_lossy().into_owned(),
                destination: format!("/rust/{resource}"),
                access: "read-only",
                purpose: "toolchain",
                identity: digest_label(
                    format!("kvist/authoring-rust/{resource}/{selected_name}").as_bytes(),
                ),
            });
        }
        let vendor = project.join(".kvist/vendored");
        let vendor = match fs::symlink_metadata(&vendor) {
            Ok(_) => {
                if !inspect(config, &vendor)?.is_dir() {
                    return Err(failure(config, "vendor must be a non-link directory"));
                }
                vendor
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let empty = staging.path().join("vendor");
                fs::create_dir(&empty).map_err(|e| {
                    sandbox_error(config, "create empty offline authoring vendor", e)
                })?;
                empty
            }
            Err(e) => return Err(sandbox_error(config, "inspect authoring vendor", e)),
        };
        grants.push(SandboxGrant {
            source: vendor.to_string_lossy().into_owned(),
            destination: "/rust/vendor".into(),
            access: "read-only",
            purpose: "toolchain",
            identity: digest_label(b"offline-authoring-vendor"),
        });
        for name in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "rust-toolchain",
        ] {
            let path = component.join(name);
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    if !inspect(config, &path)?.is_file() {
                        return Err(failure(
                            config,
                            "local Rust metadata must be a regular non-link file",
                        ));
                    }
                    if inspect(config, &path)?.len() > 1 << 20 {
                        return Err(failure(config, "local Rust metadata exceeds 1 MiB"));
                    }
                    let mut bytes = Vec::new();
                    File::open(&path)
                        .and_then(|file| file.take((1 << 20) + 1).read_to_end(&mut bytes))
                        .map_err(|e| {
                            sandbox_error(config, "read bounded local Rust metadata", e)
                        })?;
                    if bytes.len() > 1 << 20 {
                        return Err(failure(config, "local Rust metadata grew beyond 1 MiB"));
                    }
                    grants.push(SandboxGrant {
                        source: path.to_string_lossy().into_owned(),
                        destination: format!("/workspace/component/{name}"),
                        access: "read-only",
                        purpose: "context",
                        identity: digest_label(&bytes),
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(sandbox_error(config, "inspect local Rust metadata", e)),
            }
        }
        let context = super::cargo_context::prepare(config, project, component, started)?;
        grants.extend(context.grants);
        tracked.extend(context.tracked);
        Ok(Self {
            _staging: staging,
            grants,
            selected: selected_name,
            pinned: pin.pinned,
            tracked,
            component: component
                .canonicalize()
                .map_err(|e| sandbox_error(config, "resolve authoring Rust component", e))?,
            build_directory: context.directory,
        })
    }

    pub(super) fn apply(
        &self,
        config: &SandboxConfig,
        grants: &mut Vec<SandboxGrant>,
        environment: &mut BTreeMap<String, String>,
        argv: &mut Vec<String>,
    ) -> Result<()> {
        for (path, expected) in &self.tracked {
            if fingerprint(&inspect(config, path)?) != *expected {
                return Err(failure(
                    config,
                    "installed Rust inventory changed during preparation; restart after host provisioning",
                ));
            }
        }
        for resource in &self.grants {
            let source = Path::new(&resource.source);
            if grants.iter().any(|grant| {
                grant.access == "read-write" && {
                    let writable = Path::new(&grant.source);
                    let local_build_alias = resource.purpose == "context"
                        && grant.purpose == "authoring"
                        && source.starts_with(&self.component);
                    !local_build_alias
                        && (source.starts_with(writable) || writable.starts_with(source))
                }
            }) {
                return Err(failure(
                    config,
                    "Rust resources overlap writable authoring/scratch grants",
                ));
            }
        }
        grants.extend(self.grants.iter().map(|grant| SandboxGrant {
            source: grant.source.clone(),
            destination: grant.destination.clone(),
            access: grant.access,
            purpose: grant.purpose,
            identity: grant.identity.clone(),
        }));
        environment.retain(|name, _| {
            !name.starts_with("CARGO_")
                && !name.starts_with("RUSTUP_")
                && !matches!(
                    name.as_str(),
                    "RUSTC" | "RUSTDOC" | "RUSTFLAGS" | "RUSTDOCFLAGS"
                )
        });
        for (name, value) in [
            ("PATH", "/rust/runtime/bin:/usr/bin:/bin:/usr/sbin:/sbin"),
            ("HOME", "/tmp"),
            ("RUSTUP_HOME", "/tmp/rustup-home"),
            ("RUSTUP_AUTO_INSTALL", "0"),
            ("CARGO_HOME", "/tmp/cargo-home"),
            ("CARGO_TARGET_DIR", "/tmp/target"),
            ("CARGO_NET_OFFLINE", "true"),
            ("RUSTC", "/rust/runtime/bin/rustc"),
            ("RUSTDOC", "/rust/runtime/bin/rustdoc"),
        ] {
            environment.insert(name.into(), value.into());
        }
        environment.remove("RUSTUP_TOOLCHAIN");
        environment.insert("KVIST_CARGO_WORKDIR".into(), self.build_directory.clone());
        environment.insert(
            "KVIST_COMPONENT_BUILD_DIR".into(),
            self.build_directory.clone(),
        );
        environment.insert("RUSTFMT".into(), "/rust/runtime/bin/rustfmt".into());
        if self.pinned {
            environment.insert("RUSTUP_TOOLCHAIN".into(), self.selected.clone());
        }
        let mut wrapped = vec![
            "/usr/bin/bash".into(),
            "-c".into(),
            "source /rust/runtime/initialize; exec \"$@\"".into(),
            "maerg".into(),
        ];
        wrapped.append(argv);
        *argv = wrapped;
        validate_request_inputs(config, &argv[0], &argv[1..], environment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SandboxConfig {
        SandboxConfig {
            runner: "/runner-not-invoked".into(),
            backend: "/backend-not-invoked".into(),
            environment_allowlist: Vec::new(),
            acquisition: crate::config::AcquisitionConfig::default(),
        }
    }

    #[test]
    fn resources_reject_links_noncanonical_paths_and_special_entries() {
        let staging = tempfile::tempdir().unwrap();
        let root = staging.path().canonicalize().unwrap();
        fs::create_dir(root.join("directory")).unwrap();
        fs::write(root.join("directory/file"), b"resource").unwrap();
        symlink(root.join("directory"), root.join("link")).unwrap();
        assert!(inspect(&config(), &root.join("link/file")).is_err());
        assert!(inspect(&config(), &root.join("directory/../directory/file")).is_err());
        assert!(inspect(&config(), Path::new("relative")).is_err());
        nix::unistd::mkfifo(&root.join("fifo"), nix::sys::stat::Mode::S_IRUSR).unwrap();
        assert!(
            track_tree(
                &config(),
                &root.join("fifo"),
                &mut Vec::new(),
                &mut 0,
                Instant::now(),
                0
            )
            .is_err()
        );
    }

    #[test]
    fn writable_alias_and_resource_drift_are_rejected_before_dispatch() {
        let staging = tempfile::tempdir().unwrap();
        let path = staging.path().canonicalize().unwrap();
        let resource = path.join("resource");
        fs::create_dir(&resource).unwrap();
        let environment = RustEnvironment {
            grants: vec![SandboxGrant {
                source: resource.to_string_lossy().into_owned(),
                destination: "/rust/vendor".into(),
                access: "read-only",
                purpose: "toolchain",
                identity: digest_label(b"resource"),
            }],
            tracked: vec![(
                resource.clone(),
                fingerprint(&inspect(&config(), &resource).unwrap()),
            )],
            _staging: staging,
            selected: "stable-x86_64-unknown-linux-gnu".into(),
            pinned: false,
            component: path.join("component"),
            build_directory: "/rust/project/component".into(),
        };
        let mut grants = vec![SandboxGrant {
            source: path.to_string_lossy().into_owned(),
            destination: "/workspace/component/src".into(),
            access: "read-write",
            purpose: "authoring",
            identity: digest_label(b"writable"),
        }];
        let mut variables = BTreeMap::new();
        let mut argv = vec!["/usr/bin/true".into()];
        assert!(
            environment
                .apply(&config(), &mut grants, &mut variables, &mut argv)
                .is_err()
        );
        fs::write(resource.join("changed"), b"drift").unwrap();
        assert!(
            environment
                .apply(&config(), &mut Vec::new(), &mut variables, &mut argv)
                .is_err()
        );
        assert!(variables.is_empty());
        assert_eq!(argv, ["/usr/bin/true"]);
    }
}
