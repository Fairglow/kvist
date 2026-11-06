//! Explicitly approved, read-only Cargo workspace/path-dependency build context.

use super::rust_environment::{Fingerprint, check_budget, fingerprint, inspect};
use super::*;
use std::collections::BTreeSet;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

pub(super) struct CargoContext {
    pub grants: Vec<SandboxGrant>,
    pub tracked: Vec<(PathBuf, Fingerprint)>,
    pub directory: String,
    metadata: BTreeSet<PathBuf>,
}

fn failure(config: &SandboxConfig, reason: impl Into<String>) -> KvistError {
    KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: reason.into(),
    }
}

fn manifest(config: &SandboxConfig, root: &Path) -> Result<toml::Value> {
    let path = root.join("Cargo.toml");
    let metadata = inspect(config, &path)?;
    if !metadata.is_file() || metadata.len() > 1 << 20 {
        return Err(failure(
            config,
            "Cargo build context requires regular manifests no larger than 1 MiB",
        ));
    }
    let bytes = read_metadata(config, &path, fingerprint(&metadata))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| failure(config, "Cargo build manifests must be UTF-8"))?;
    toml::from_str(text).map_err(|e| {
        failure(
            config,
            format!("invalid Cargo build manifest `{}`: {e}", path.display()),
        )
    })
}

fn read_metadata(config: &SandboxConfig, path: &Path, expected: Fingerprint) -> Result<Vec<u8>> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| sandbox_error(config, "open Cargo metadata", e))?;
    let before = file
        .metadata()
        .map_err(|e| sandbox_error(config, "inspect Cargo metadata", e))?;
    if !before.is_file()
        || before.nlink() != 1
        || before.len() > 1 << 20
        || fingerprint(&before) != expected
    {
        return Err(failure(
            config,
            "Cargo metadata must be stable regular single-link files no larger than 1 MiB",
        ));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take((1 << 20) + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| sandbox_error(config, "read bounded Cargo metadata", e))?;
    let after = file
        .metadata()
        .map_err(|e| sandbox_error(config, "reinspect Cargo metadata", e))?;
    if bytes.len() > 1 << 20 || fingerprint(&after) != expected {
        return Err(failure(config, "Cargo metadata changed while reading"));
    }
    Ok(bytes)
}

fn dependency_paths(
    config: &SandboxConfig,
    value: &toml::Value,
    paths: &mut Vec<String>,
    acquisition: bool,
) -> Result<()> {
    if let Some(table) = value.as_table() {
        for dependency in table.values().filter_map(toml::Value::as_table) {
            if acquisition && let Some(repository) = dependency.get("git") {
                let repository = repository
                    .as_str()
                    .ok_or_else(|| failure(config, "Cargo Git repository must be a string"))?;
                let revision = dependency.get("rev").and_then(toml::Value::as_str)
                    .ok_or_else(|| failure(config, "configured Git acquisition requires an approved full immutable revision; branch/tag-only declarations are unsupported"))?;
                approved_git(config, repository, revision)?;
            }
            if let Some(path) = dependency.get("path") {
                paths.push(
                    path.as_str()
                        .ok_or_else(|| failure(config, "Cargo dependency path must be a string"))?
                        .to_owned(),
                );
            }
        }
    }
    Ok(())
}

fn approved_git(config: &SandboxConfig, repository: &str, revision: &str) -> Result<()> {
    if config.acquisition.additional_sources.iter().any(|source| {
        matches!(source, crate::config::PackageSource::CargoGit {
            repository: approved_repository, revision: approved_revision
        } if repository == approved_repository && revision == approved_revision)
    }) {
        Ok(())
    } else {
        Err(failure(
            config,
            "Cargo Git source is not an exact approved HTTPS repository/revision; unsupported transports cannot be acquired; no host fallback",
        ))
    }
}

fn acquisition_lock_sources(config: &SandboxConfig, bytes: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| failure(config, "acquisition Cargo.lock must be UTF-8"))?;
    let lock: toml::Value =
        toml::from_str(text).map_err(|_| failure(config, "cannot parse acquisition Cargo.lock"))?;
    if let Some(packages) = lock.get("package").and_then(toml::Value::as_array) {
        for package in packages {
            if let Some(source) = package
                .get("source")
                .and_then(toml::Value::as_str)
                .and_then(|source| source.strip_prefix("git+"))
            {
                let (repository, revision) = source.rsplit_once('#').ok_or_else(|| {
                    failure(
                        config,
                        "locked Git acquisition requires an immutable revision",
                    )
                })?;
                let repository = repository
                    .split_once('?')
                    .map_or(repository, |(repository, _)| repository);
                approved_git(config, repository, revision)?;
            }
        }
    }
    Ok(())
}

fn local_root(
    config: &SandboxConfig,
    project: &Path,
    base: &Path,
    relative: &str,
) -> Result<PathBuf> {
    let path = base.join(relative);
    let root = path
        .canonicalize()
        .map_err(|e| sandbox_error(config, "resolve declared Cargo dependency", e))?;
    // Inspect the original path as well, so canonicalization cannot authorize links.
    let mut original = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                original.pop();
            }
            std::path::Component::CurDir => {}
            part => {
                original.push(part.as_os_str());
                inspect(config, &original)?;
            }
        }
    }
    if !root.starts_with(project)
        || relative.contains('\0')
        || !inspect(config, &original)?.is_dir()
    {
        return Err(failure(
            config,
            "Cargo dependency roots must be non-link directories inside the approved project",
        ));
    }
    if root
        .strip_prefix(project)
        .map_err(|_| failure(config, "Cargo dependency escapes the project"))?
        .components()
        .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
    {
        return Err(failure(
            config,
            "Cargo dependencies cannot expose hidden operational roots",
        ));
    }
    Ok(root)
}

fn members(
    config: &SandboxConfig,
    project: &Path,
    base: &Path,
    pattern: &str,
    started: Instant,
) -> Result<Vec<PathBuf>> {
    let segments: Vec<_> = pattern.split('/').collect();
    if pattern.starts_with('/')
        || segments.iter().any(|segment| {
            segment.is_empty() || (*segment != "*" && segment.contains(['*', '?', '[', ']']))
        })
    {
        return Err(failure(
            config,
            "Cargo workspace members support literal paths or whole-directory * segments only",
        ));
    }
    let mut roots = vec![base.to_owned()];
    let mut entries = 0_usize;
    for segment in segments {
        check_budget(config, started)?;
        let mut next = Vec::new();
        for root in roots {
            if segment == "*" {
                for entry in fs::read_dir(&root)
                    .map_err(|e| sandbox_error(config, "expand Cargo workspace member", e))?
                {
                    let entry = entry.map_err(|e| {
                        sandbox_error(config, "read Cargo workspace member entry", e)
                    })?;
                    check_budget(config, started)?;
                    entries += 1;
                    if entries > 100_000 {
                        return Err(failure(
                            config,
                            "Cargo workspace expansion exceeds 100000 entries",
                        ));
                    }
                    let name = entry.file_name();
                    let name = name
                        .to_str()
                        .ok_or_else(|| failure(config, "Cargo member names must be UTF-8"))?;
                    let kind = entry
                        .file_type()
                        .map_err(|e| sandbox_error(config, "inspect Cargo member entry", e))?;
                    if !name.starts_with('.') && kind.is_symlink() {
                        return Err(failure(
                            config,
                            "Cargo workspace members must not use links",
                        ));
                    }
                    if kind.is_dir() && !name.starts_with('.') {
                        next.push(local_root(config, project, &root, name)?);
                    }
                    if next.len() > 128 {
                        return Err(failure(
                            config,
                            "Cargo workspace expansion exceeds 128 roots",
                        ));
                    }
                }
            } else {
                next.push(local_root(config, project, &root, segment)?);
            }
        }
        roots = next;
    }
    roots.sort();
    Ok(roots)
}

fn excluded(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "target"
                | "node_modules"
                | "vendor"
                | "vendored"
                | "REQUIREMENTS.md"
                | "CONTRACT.md"
                | "DESIGN.md"
                | "TODOS.yaml"
                | "IMPL.md"
        )
}

struct SourceBounds {
    path_bytes: usize,
    started: Instant,
}

fn track(
    config: &SandboxConfig,
    path: &Path,
    tracked: &mut Vec<(PathBuf, Fingerprint)>,
    bounds: &mut SourceBounds,
    depth: usize,
    filter: bool,
) -> Result<Vec<PathBuf>> {
    check_budget(config, bounds.started)?;
    bounds.path_bytes += path.as_os_str().len();
    if depth > 128 || tracked.len() >= 100_000 || bounds.path_bytes > 32 << 20 {
        return Err(failure(
            config,
            "Cargo source context exceeds its entry/path/depth bounds",
        ));
    }
    let metadata = inspect(config, path)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(failure(
            config,
            "Cargo build source must contain only regular non-link files/directories",
        ));
    }
    tracked.push((path.into(), fingerprint(&metadata)));
    if metadata.is_dir() {
        let mut complete = true;
        let mut selected = Vec::new();
        for entry in fs::read_dir(path)
            .map_err(|e| sandbox_error(config, "inspect Cargo build source", e))?
        {
            let entry =
                entry.map_err(|e| sandbox_error(config, "read Cargo build source entry", e))?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| failure(config, "Cargo resource names must be UTF-8"))?;
            if filter && excluded(name) {
                complete = false;
                continue;
            }
            let child = entry.path();
            let resources = track(config, &child, tracked, bounds, depth + 1, filter)?;
            complete &= resources.len() == 1 && resources[0] == child;
            selected.extend(resources);
        }
        if !complete {
            if selected.len() > 256 {
                return Err(failure(config, "filtered Cargo context exceeds 256 mounts"));
            }
            return Ok(selected);
        }
    }
    Ok(vec![path.to_owned()])
}

pub(super) fn prepare(
    config: &SandboxConfig,
    project: &Path,
    component: &Path,
    started: Instant,
) -> Result<CargoContext> {
    prepare_with_policy(config, project, component, started, true)
}

fn prepare_with_policy(
    config: &SandboxConfig,
    project: &Path,
    component: &Path,
    started: Instant,
    authoring: bool,
) -> Result<CargoContext> {
    let project = project
        .canonicalize()
        .map_err(|e| sandbox_error(config, "resolve Cargo build project", e))?;
    let component = component
        .canonicalize()
        .map_err(|e| sandbox_error(config, "resolve Cargo build component", e))?;
    inspect(config, &project)?;
    inspect(config, &component)?;
    if !component.starts_with(&project) {
        return Err(failure(
            config,
            "Cargo component must be inside its approved project",
        ));
    }
    let mut pending = BTreeSet::from([component.clone()]);
    match fs::symlink_metadata(project.join("Cargo.toml")) {
        Ok(_) => {
            pending.insert(project.clone());
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(sandbox_error(config, "inspect workspace Cargo manifest", e)),
    }
    let mut roots = BTreeMap::new();
    while let Some(root) = pending.pop_first() {
        check_budget(config, started)?;
        if roots.contains_key(&root) {
            continue;
        }
        if roots.len() >= 128 {
            return Err(failure(config, "Cargo build context exceeds 128 crates"));
        }
        let value = manifest(config, &root)?;
        if authoring
            && root != component
            && AUTHORING_WRITABLE_ROOTS
                .iter()
                .any(|name| root.starts_with(component.join(name)))
        {
            return Err(failure(
                config,
                "Cargo provider roots must not overlap writable component implementation/test roots",
            ));
        }
        let mut paths = Vec::new();
        for key in [
            "dependencies",
            "dev-dependencies",
            "build-dependencies",
            "replace",
        ] {
            if let Some(table) = value.get(key) {
                dependency_paths(config, table, &mut paths, !authoring)?;
            }
        }
        if let Some(targets) = value.get("target").and_then(toml::Value::as_table) {
            for target in targets.values() {
                for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
                    if let Some(table) = target.get(key) {
                        dependency_paths(config, table, &mut paths, !authoring)?;
                    }
                }
            }
        }
        if let Some(patches) = value.get("patch").and_then(toml::Value::as_table) {
            for table in patches.values() {
                dependency_paths(config, table, &mut paths, !authoring)?;
            }
        }
        if let Some(workspace) = value.get("workspace") {
            if let Some(table) = workspace.get("dependencies") {
                dependency_paths(config, table, &mut paths, !authoring)?;
            }
            let mut exclusions = BTreeSet::new();
            if let Some(exclude) = workspace.get("exclude").and_then(toml::Value::as_array) {
                for pattern in exclude {
                    let pattern = pattern.as_str().ok_or_else(|| {
                        failure(config, "Cargo workspace exclude must contain strings")
                    })?;
                    exclusions.extend(members(config, &project, &root, pattern, started)?);
                }
            }
            if let Some(list) = workspace.get("members").and_then(toml::Value::as_array) {
                for pattern in list {
                    let pattern = pattern.as_str().ok_or_else(|| {
                        failure(config, "Cargo workspace members must contain strings")
                    })?;
                    for member in members(config, &project, &root, pattern, started)? {
                        if !exclusions.contains(&member)
                            && !roots.contains_key(&member)
                            && member != root
                        {
                            pending.insert(member);
                            if pending.len() + roots.len() + 1 > 128 {
                                return Err(failure(
                                    config,
                                    "Cargo build context exceeds 128 crates",
                                ));
                            }
                        }
                    }
                }
            }
        }
        for path in paths {
            let dependency = local_root(config, &project, &root, &path)?;
            if dependency != root && !roots.contains_key(&dependency) {
                pending.insert(dependency);
                if pending.len() + roots.len() + 1 > 128 {
                    return Err(failure(config, "Cargo build context exceeds 128 crates"));
                }
            }
        }
        roots.insert(root, value);
    }
    let mut candidates = BTreeSet::new();
    let mut metadata = BTreeSet::new();
    for (root, value) in &roots {
        candidates.insert(root.join("Cargo.toml"));
        metadata.insert(root.join("Cargo.toml"));
        if root.join("Cargo.lock").exists() {
            candidates.insert(root.join("Cargo.lock"));
            metadata.insert(root.join("Cargo.lock"));
        }
        if value.get("package").is_none() {
            continue;
        }
        for entry in fs::read_dir(root)
            .map_err(|e| sandbox_error(config, "select Cargo build resources", e))?
        {
            let entry =
                entry.map_err(|e| sandbox_error(config, "read Cargo build resource entry", e))?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| failure(config, "Cargo resource names must be UTF-8"))?;
            let path = entry.path();
            if excluded(name)
                || roots
                    .keys()
                    .any(|other| other != root && other.starts_with(&path))
                    && !matches!(name, "src" | "tests")
            {
                continue;
            }
            candidates.insert(path);
        }
    }
    let mut tracked = Vec::new();
    let mut bounds = SourceBounds {
        path_bytes: 0,
        started,
    };
    let mut selected = BTreeSet::new();
    for path in candidates {
        if selected
            .iter()
            .any(|resource: &PathBuf| path.starts_with(resource))
        {
            continue;
        }
        let local_writable_root = AUTHORING_WRITABLE_ROOTS
            .iter()
            .any(|name| path == component.join(name));
        selected.extend(track(
            config,
            &path,
            &mut tracked,
            &mut bounds,
            0,
            !local_writable_root,
        )?);
        if selected.len() > 256 {
            return Err(failure(config, "filtered Cargo context exceeds 256 mounts"));
        }
    }
    let mut grants = Vec::new();
    for path in selected {
        let relative = path
            .strip_prefix(&project)
            .map_err(|_| failure(config, "Cargo source escapes project"))?;
        grants.push(SandboxGrant {
            source: path.to_string_lossy().into_owned(),
            destination: format!("/rust/project/{}", relative.display()),
            access: "read-only",
            purpose: "context",
            identity: digest_label(path.to_string_lossy().as_bytes()),
        });
    }
    let relative = component
        .strip_prefix(project)
        .map_err(|_| failure(config, "Cargo component escapes project"))?;
    let directory = if relative.as_os_str().is_empty() {
        "/rust/project".into()
    } else {
        format!("/rust/project/{}", relative.display())
    };
    Ok(CargoContext {
        grants,
        tracked,
        directory,
        metadata,
    })
}

/// Stage bounded manifests/lock bytes and empty target files, never source contents.
pub(crate) fn stage_acquisition_workspace(
    config: &SandboxConfig,
    project: &Path,
    destination: &Path,
) -> Result<String> {
    let started = Instant::now();
    crate::acquisition::validate_acquisition_config(&config.acquisition)
        .map_err(|e| failure(config, format!("invalid acquisition source policy: {e}")))?;
    let project = project
        .canonicalize()
        .map_err(|e| sandbox_error(config, "resolve acquisition project", e))?;
    let context = prepare_with_policy(config, &project, &project, started, false)?;
    let mut lock_identity = None;
    for (source, expected) in &context.tracked {
        check_budget(config, started)?;
        let metadata = inspect(config, source)?;
        if fingerprint(&metadata) != *expected {
            return Err(failure(
                config,
                "Cargo inputs changed before acquisition staging",
            ));
        }
        let relative = source
            .strip_prefix(&project)
            .map_err(|_| failure(config, "Cargo acquisition inputs escape the project"))?;
        let target = destination.join(relative);
        if metadata.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|e| sandbox_error(config, "stage Cargo target directory", e))?;
            continue;
        }
        fs::create_dir_all(
            target
                .parent()
                .ok_or_else(|| failure(config, "Cargo target has no parent"))?,
        )
        .map_err(|e| sandbox_error(config, "stage Cargo metadata parent", e))?;
        let bytes = if context.metadata.contains(source) {
            read_metadata(config, source, *expected)?
        } else {
            Vec::new()
        };
        if relative == Path::new("Cargo.lock") {
            acquisition_lock_sources(config, &bytes)?;
            lock_identity = Some(crate::vendoring::lockfile_digest(&bytes));
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| sandbox_error(config, "create private Cargo metadata/target", e))?;
        file.write_all(&bytes)
            .map_err(|e| sandbox_error(config, "write private Cargo metadata/target", e))?;
    }
    for (path, expected) in &context.tracked {
        check_budget(config, started)?;
        if fingerprint(&inspect(config, path)?) != *expected {
            return Err(failure(
                config,
                "Cargo inputs changed during acquisition staging",
            ));
        }
    }
    let identity = lock_identity
        .ok_or_else(|| failure(config, "Cargo acquisition requires a regular Cargo.lock"))?;
    let mut registries = toml::Table::new();
    for source in &config.acquisition.additional_sources {
        if let crate::config::PackageSource::CargoRegistry {
            name, index_origin, ..
        } = source
        {
            let registry = toml::Table::from_iter([(
                "index".into(),
                toml::Value::String(index_origin.clone()),
            )]);
            if registries
                .insert(name.clone(), toml::Value::Table(registry))
                .is_some()
            {
                return Err(failure(config, "duplicate approved Cargo registry name"));
            }
        }
    }
    if !registries.is_empty() {
        let cargo = destination.join(".cargo");
        fs::create_dir(&cargo)
            .map_err(|e| sandbox_error(config, "stage approved registry settings", e))?;
        let contents = toml::to_string(&toml::Table::from_iter([(
            "registries".into(),
            toml::Value::Table(registries),
        )]))
        .map_err(|e| failure(config, format!("serialize approved registry settings: {e}")))?;
        fs::write(cargo.join("config.toml"), contents)
            .map_err(|e| sandbox_error(config, "write approved registry settings", e))?;
    }
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SandboxConfig {
        SandboxConfig {
            runner: "/not-invoked".into(),
            backend: "/not-invoked".into(),
            environment_allowlist: Vec::new(),
            acquisition: crate::config::AcquisitionConfig::default(),
        }
    }

    #[test]
    fn acquisition_snapshot_preserves_metadata_but_not_source_or_host_configuration() {
        let project = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fs::create_dir(project.path().join("src")).unwrap();
        fs::create_dir(project.path().join(".cargo")).unwrap();
        let manifest = "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n";
        fs::write(project.path().join("Cargo.toml"), manifest).unwrap();
        fs::write(project.path().join("Cargo.lock"), "version=4\n").unwrap();
        fs::write(project.path().join("src/lib.rs"), "PRIVATE_SOURCE_CONTENT").unwrap();
        fs::write(
            project.path().join(".cargo/config.toml"),
            "[alias]\nbuild='dangerous'\n",
        )
        .unwrap();
        fs::write(project.path().join("REQUIREMENTS.md"), "PRIVATE_INTENT").unwrap();
        let mut config = config();
        config
            .acquisition
            .additional_sources
            .push(crate::config::PackageSource::CargoRegistry {
                name: "approved".into(),
                index_origin: "https://registry.example.invalid/index/".into(),
                download_origin: "https://registry.example.invalid/crates/".into(),
            });
        let identity =
            stage_acquisition_workspace(&config, project.path(), destination.path()).unwrap();
        assert_eq!(identity, crate::vendoring::lockfile_digest(b"version=4\n"));
        assert_eq!(
            fs::read_to_string(destination.path().join("Cargo.toml")).unwrap(),
            manifest
        );
        assert!(
            fs::read(destination.path().join("src/lib.rs"))
                .unwrap()
                .is_empty()
        );
        assert!(!destination.path().join("REQUIREMENTS.md").exists());
        let settings: toml::Value = toml::from_str(
            &fs::read_to_string(destination.path().join(".cargo/config.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["registries"]["approved"]["index"].as_str(),
            Some("https://registry.example.invalid/index/")
        );
        assert!(settings.get("alias").is_none());
        assert_eq!(
            fs::read_to_string(project.path().join("src/lib.rs")).unwrap(),
            "PRIVATE_SOURCE_CONTENT"
        );
    }

    #[test]
    fn acquisition_snapshot_rejects_links_and_missing_lockfiles() {
        for linked in [false, true] {
            let project = tempfile::tempdir().unwrap();
            let destination = tempfile::tempdir().unwrap();
            fs::create_dir(project.path().join("src")).unwrap();
            fs::write(
                project.path().join("Cargo.toml"),
                "[package]\nname='fixture'\nversion='0.1.0'\n",
            )
            .unwrap();
            if linked {
                fs::write(project.path().join("Cargo.lock"), "version=4\n").unwrap();
                std::os::unix::fs::symlink("/etc/passwd", project.path().join("src/lib.rs"))
                    .unwrap();
            } else {
                fs::write(project.path().join("src/lib.rs"), "").unwrap();
            }
            assert!(
                stage_acquisition_workspace(&config(), project.path(), destination.path()).is_err()
            );
        }
    }

    #[test]
    fn acquisition_rejects_unapproved_git_and_native_transports_before_fetch() {
        let revision = "a".repeat(40);
        for (repository, approved) in [
            ("git://internal.example.invalid/private", false),
            ("ssh://internal.example.invalid/private", false),
            ("https://unapproved.example.invalid/private", false),
            ("https://git.example.invalid/dependency.git", true),
        ] {
            let project = tempfile::tempdir().unwrap();
            let destination = tempfile::tempdir().unwrap();
            fs::create_dir(project.path().join("src")).unwrap();
            fs::write(project.path().join("src/lib.rs"), "").unwrap();
            fs::write(project.path().join("Cargo.toml"), format!(
                "[package]\nname='fixture'\nversion='0.1.0'\n[dependencies]\nexternal={{git='{repository}',rev='{revision}'}}\n"
            )).unwrap();
            fs::write(project.path().join("Cargo.lock"), format!(
                "version=4\n[[package]]\nname='external'\nversion='0.1.0'\nsource='git+{repository}?rev={revision}#{revision}'\n"
            )).unwrap();
            let mut config = config();
            config
                .acquisition
                .additional_sources
                .push(crate::config::PackageSource::CargoGit {
                    repository: "https://git.example.invalid/dependency.git".into(),
                    revision: revision.clone(),
                });
            let result = stage_acquisition_workspace(&config, project.path(), destination.path());
            assert_eq!(result.is_ok(), approved, "{repository}: {result:?}");
        }
    }

    #[test]
    fn acquisition_rejects_native_git_hidden_in_lock_or_target_patch_tables() {
        let revision = "a".repeat(40);
        for section in [
            "",
            "[target.'cfg(unix)'.build-dependencies]",
            "[workspace.dependencies]",
            "[patch.crates-io]",
            "[replace]",
        ] {
            let project = tempfile::tempdir().unwrap();
            let destination = tempfile::tempdir().unwrap();
            fs::create_dir(project.path().join("src")).unwrap();
            fs::write(project.path().join("src/lib.rs"), "").unwrap();
            let dependency = if section.is_empty() {
                String::new()
            } else {
                format!(
                    "{section}\nexternal={{git='git://internal.example.invalid/private',rev='{revision}'}}\n"
                )
            };
            fs::write(
                project.path().join("Cargo.toml"),
                format!("[package]\nname='fixture'\nversion='0.1.0'\n{dependency}"),
            )
            .unwrap();
            fs::write(project.path().join("Cargo.lock"), if section.is_empty() {
                format!("version=4\n[[package]]\nname='external'\nversion='0.1.0'\nsource='git+git://internal.example.invalid/private#{revision}'\n")
            } else { "version=4\n".into() }).unwrap();
            assert!(
                stage_acquisition_workspace(&config(), project.path(), destination.path()).is_err(),
                "{section}"
            );
        }
    }

    #[test]
    fn clean_subtrees_collapse_before_mount_bounds_and_provider_aliases_fail() {
        let project = tempfile::tempdir().unwrap();
        fs::create_dir(project.path().join("src")).unwrap();
        fs::write(
            project.path().join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        )
        .unwrap();
        for index in 0..257 {
            fs::write(project.path().join(format!("src/file{index}.rs")), "").unwrap();
        }
        let context = prepare(&config(), project.path(), project.path(), Instant::now()).unwrap();
        assert_eq!(context.grants.len(), 2);
        fs::create_dir(project.path().join("src/provider")).unwrap();
        fs::write(
            project.path().join("src/provider/Cargo.toml"),
            "[package]\nname='provider'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(project.path().join("Cargo.toml"), "[package]\nname='fixture'\nversion='0.1.0'\n[dependencies]\nprovider={path='src/provider'}\n").unwrap();
        assert!(
            prepare(&config(), project.path(), project.path(), Instant::now())
                .err()
                .unwrap()
                .to_string()
                .contains("overlap writable")
        );
    }

    #[test]
    fn declared_sources_cannot_escape_use_links_or_hidden_roots() {
        let config = SandboxConfig {
            runner: "/not-invoked".into(),
            backend: "/not-invoked".into(),
            environment_allowlist: Vec::new(),
            acquisition: crate::config::AcquisitionConfig::default(),
        };
        let parent = tempfile::tempdir().unwrap();
        let project = parent.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::create_dir(project.join("regular")).unwrap();
        fs::create_dir(project.join(".private")).unwrap();
        symlink(project.join("regular"), project.join("linked")).unwrap();
        let outside = parent.path().join("outside");
        fs::create_dir(&outside).unwrap();
        assert!(local_root(&config, &project, &project, "../outside").is_err());
        assert!(local_root(&config, &project, &project, "linked").is_err());
        assert!(local_root(&config, &project, &project, ".private").is_err());
        assert!(members(&config, &project, &project, "**", Instant::now()).is_err());
        assert!(members(&config, &project, &project, "regular?", Instant::now()).is_err());
        assert!(members(&config, &project, &project, "*", Instant::now()).is_err());
    }
}
