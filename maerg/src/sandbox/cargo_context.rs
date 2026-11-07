//! Explicitly approved, read-only Cargo workspace/path-dependency build context.

use super::rust_environment::{Fingerprint, check_budget, fingerprint, inspect, resource_kind};
use super::*;
use std::collections::BTreeSet;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

#[cfg(test)]
#[path = "cargo_context/diagnostic_tests.rs"]
mod diagnostic_tests;

#[cfg(test)]
#[path = "cargo_context/native_snapshot_tests.rs"]
mod native_snapshot_tests;

#[cfg(test)]
#[path = "cargo_context/tests.rs"]
mod tests;

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

fn path_failure(config: &SandboxConfig, path: &Path, reason: impl std::fmt::Display) -> KvistError {
    failure(
        config,
        format!("Cargo context `{}`: {reason}", path.display()),
    )
}

fn manifest(config: &SandboxConfig, root: &Path) -> Result<toml::Value> {
    let path = root.join("Cargo.toml");
    let metadata = inspect(config, &path)?;
    if !metadata.is_file() {
        return Err(path_failure(
            config,
            &path,
            format!(
                "manifest is a {}; replace it with a regular non-link file",
                resource_kind(&metadata)
            ),
        ));
    }
    let bytes = read_metadata(config, &path, fingerprint(&metadata))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|e| path_failure(config, &path, format!("manifest must be UTF-8: {e}")))?;
    toml::from_str(text).map_err(|e| {
        failure(
            config,
            format!("invalid Cargo build manifest `{}`: {e}", path.display()),
        )
    })
}

fn read_metadata(config: &SandboxConfig, path: &Path, expected: Fingerprint) -> Result<Vec<u8>> {
    read_metadata_bounded(config, path, expected, 1 << 20)
}

fn read_metadata_bounded(
    config: &SandboxConfig,
    path: &Path,
    expected: Fingerprint,
    limit: u64,
) -> Result<Vec<u8>> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| sandbox_path_error(config, "open Cargo metadata", path, e))?;
    let before = file
        .metadata()
        .map_err(|e| sandbox_path_error(config, "inspect Cargo metadata", path, e))?;
    if !before.is_file() {
        return Err(path_failure(
            config,
            path,
            format!(
                "opened metadata is a {}; use a regular file",
                resource_kind(&before)
            ),
        ));
    }
    if before.nlink() != 1 {
        return Err(path_failure(
            config,
            path,
            format!(
                "metadata has {} hard links; replace it with a single-link regular file",
                before.nlink()
            ),
        ));
    }
    if before.len() > limit {
        return Err(path_failure(
            config,
            path,
            format!(
                "metadata is {} bytes (limit {limit} bytes); reduce its size",
                before.len()
            ),
        ));
    }
    if fingerprint(&before) != expected {
        return Err(path_failure(
            config,
            path,
            "metadata changed while opening; retry after concurrent edits have stopped",
        ));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| sandbox_path_error(config, "read bounded Cargo metadata", path, e))?;
    let after = file
        .metadata()
        .map_err(|e| sandbox_path_error(config, "reinspect Cargo metadata", path, e))?;
    if bytes.len() as u64 > limit {
        return Err(path_failure(
            config,
            path,
            format!(
                "metadata read exceeded {limit} bytes (read {} bytes); reduce its size",
                bytes.len()
            ),
        ));
    }
    if fingerprint(&after) != expected {
        return Err(path_failure(
            config,
            path,
            "metadata changed while reading; retry after concurrent edits have stopped",
        ));
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

fn acquisition_lock_sources(config: &SandboxConfig, path: &Path, bytes: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(bytes).map_err(|e| {
        path_failure(
            config,
            path,
            format!("acquisition Cargo.lock must be UTF-8: {e}"),
        )
    })?;
    let lock: toml::Value = toml::from_str(text).map_err(|e| {
        path_failure(
            config,
            path,
            format!("cannot parse acquisition Cargo.lock: {e}"),
        )
    })?;
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
        .map_err(|e| sandbox_path_error(config, "resolve declared Cargo dependency", &path, e))?;
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
        return Err(path_failure(
            config,
            &path,
            format!(
                "dependency root resolves to `{}`; it must be a non-link directory inside approved project `{}`",
                root.display(),
                project.display()
            ),
        ));
    }
    if root
        .strip_prefix(project)
        .map_err(|_| failure(config, "Cargo dependency escapes the project"))?
        .components()
        .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
    {
        return Err(path_failure(
            config,
            &root,
            "Cargo dependencies cannot expose hidden operational roots; select a non-hidden in-project dependency directory",
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
        return Err(path_failure(
            config,
            &base.join("Cargo.toml"),
            format!(
                "workspace pattern `{pattern}` is unsupported; use literal paths or whole-directory * segments only"
            ),
        ));
    }
    let mut roots = vec![base.to_owned()];
    let mut entries = 0_usize;
    for segment in segments {
        check_budget(config, started)?;
        let mut next = Vec::new();
        for root in roots {
            if segment == "*" {
                for entry in fs::read_dir(&root).map_err(|e| {
                    sandbox_path_error(config, "expand Cargo workspace member", &root, e)
                })? {
                    let entry = entry.map_err(|e| {
                        sandbox_path_error(config, "read Cargo workspace member entry", &root, e)
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
                    let kind = entry.file_type().map_err(|e| {
                        sandbox_path_error(config, "inspect Cargo member entry", &entry.path(), e)
                    })?;
                    if !name.starts_with('.') && kind.is_symlink() {
                        return Err(path_failure(
                            config,
                            &entry.path(),
                            "workspace member is a symbolic link; replace it with a regular directory",
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
        return Err(path_failure(
            config,
            path,
            format!(
                "source context exceeds its bounds: entries={} (limit 100000), path bytes={} (limit 33554432), depth={} (limit 128); narrow selected source material",
                tracked.len(),
                bounds.path_bytes,
                depth
            ),
        ));
    }
    let metadata = inspect(config, path)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(path_failure(
            config,
            path,
            format!(
                "source entry is a {}; only regular non-link files/directories are allowed; stop the owning process and move runtime data outside the selected source tree before retrying",
                resource_kind(&metadata)
            ),
        ));
    }
    tracked.push((path.into(), fingerprint(&metadata)));
    if metadata.is_dir() {
        let mut complete = true;
        let mut selected = Vec::new();
        for entry in fs::read_dir(path)
            .map_err(|e| sandbox_path_error(config, "inspect Cargo build source", path, e))?
        {
            let entry = entry.map_err(|e| {
                sandbox_path_error(config, "read Cargo build source entry", path, e)
            })?;
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
                return Err(path_failure(
                    config,
                    path,
                    format!(
                        "filtered Cargo context requires {} mounts (limit 256); narrow selected source material",
                        selected.len()
                    ),
                ));
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
    prepare_with_policy(
        config,
        project,
        component,
        started,
        SnapshotPurpose::Authoring,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SnapshotPurpose {
    Authoring,
    Acquisition,
    Admission,
}

fn prepare_with_policy(
    config: &SandboxConfig,
    project: &Path,
    component: &Path,
    started: Instant,
    purpose: SnapshotPurpose,
) -> Result<CargoContext> {
    let project = project
        .canonicalize()
        .map_err(|e| sandbox_path_error(config, "resolve Cargo build project", project, e))?;
    let component = component
        .canonicalize()
        .map_err(|e| sandbox_path_error(config, "resolve Cargo build component", component, e))?;
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
        Err(e) => {
            return Err(sandbox_path_error(
                config,
                "inspect workspace Cargo manifest",
                &project.join("Cargo.toml"),
                e,
            ));
        }
    }
    let mut roots = BTreeMap::new();
    let mut manifest_inputs = BTreeMap::new();
    while let Some(root) = pending.pop_first() {
        check_budget(config, started)?;
        if roots.contains_key(&root) {
            continue;
        }
        if roots.len() >= 128 {
            return Err(failure(config, "Cargo build context exceeds 128 crates"));
        }
        let manifest_path = root.join("Cargo.toml");
        let expected = fingerprint(&inspect(config, &manifest_path)?);
        let value = manifest(config, &root)?;
        manifest_inputs.insert(manifest_path, expected);
        if purpose == SnapshotPurpose::Authoring
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
                dependency_paths(
                    config,
                    table,
                    &mut paths,
                    purpose == SnapshotPurpose::Acquisition,
                )?;
            }
        }
        if let Some(targets) = value.get("target").and_then(toml::Value::as_table) {
            for target in targets.values() {
                for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
                    if let Some(table) = target.get(key) {
                        dependency_paths(
                            config,
                            table,
                            &mut paths,
                            purpose == SnapshotPurpose::Acquisition,
                        )?;
                    }
                }
            }
        }
        if let Some(patches) = value.get("patch").and_then(toml::Value::as_table) {
            for table in patches.values() {
                dependency_paths(
                    config,
                    table,
                    &mut paths,
                    purpose == SnapshotPurpose::Acquisition,
                )?;
            }
        }
        if let Some(workspace) = value.get("workspace") {
            if let Some(table) = workspace.get("dependencies") {
                dependency_paths(
                    config,
                    table,
                    &mut paths,
                    purpose == SnapshotPurpose::Acquisition,
                )?;
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
    if purpose != SnapshotPurpose::Authoring {
        let context = select_native_metadata(config, &project, &roots, started)?;
        for (path, expected) in &context.tracked {
            if manifest_inputs
                .get(path)
                .is_some_and(|original| original != expected)
            {
                return Err(path_failure(
                    config,
                    path,
                    "manifest changed during Cargo graph discovery; retry after concurrent edits have stopped",
                ));
            }
        }
        return Ok(context);
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
            .map_err(|e| sandbox_path_error(config, "select Cargo build resources", root, e))?
        {
            let entry = entry.map_err(|e| {
                sandbox_path_error(config, "read Cargo build resource entry", root, e)
            })?;
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
            return Err(path_failure(
                config,
                &path,
                format!(
                    "filtered Cargo context requires {} mounts (limit 256); narrow selected source material",
                    selected.len()
                ),
            ));
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

struct NativeSelection<'a> {
    config: &'a SandboxConfig,
    project: &'a Path,
    tracked: BTreeMap<PathBuf, Fingerprint>,
    bounds: SourceBounds,
    entries: usize,
}

impl NativeSelection<'_> {
    fn observe(&mut self, path: &Path, required: bool, directory: bool) -> Result<bool> {
        check_budget(self.config, self.bounds.started)?;
        match fs::symlink_metadata(path) {
            Err(e) if !required && e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => {
                return Err(sandbox_path_error(
                    self.config,
                    "inspect Cargo snapshot input",
                    path,
                    e,
                ));
            }
            Ok(_) => {}
        }
        let metadata = inspect(self.config, path)?;
        if if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        } {
            return Err(path_failure(
                self.config,
                path,
                format!(
                    "selected Cargo {} is a {}; use a regular non-link {}",
                    if directory {
                        "target directory"
                    } else {
                        "metadata/target"
                    },
                    resource_kind(&metadata),
                    if directory { "directory" } else { "file" },
                ),
            ));
        }
        if !directory && metadata.nlink() != 1 {
            return Err(path_failure(
                self.config,
                path,
                format!(
                    "selected Cargo metadata/target has {} hard links; use a single-link regular file",
                    metadata.nlink(),
                ),
            ));
        }
        if let Some(expected) = self.tracked.get(path) {
            if fingerprint(&metadata) != *expected {
                return Err(path_failure(
                    self.config,
                    path,
                    "input changed during Cargo target discovery; retry after concurrent edits have stopped",
                ));
            }
            return Ok(true);
        }
        let relative = path.strip_prefix(self.project).map_err(|_| {
            path_failure(
                self.config,
                path,
                "Cargo snapshot input escapes the approved project",
            )
        })?;
        self.bounds.path_bytes += path.as_os_str().len();
        let depth = relative.components().count();
        if depth > 128 || self.tracked.len() >= 100_000 || self.bounds.path_bytes > 32 << 20 {
            return Err(path_failure(
                self.config,
                path,
                format!(
                    "Cargo snapshot exceeds bounds: entries={} (limit 100000), path bytes={} (limit 33554432), depth={depth} (limit 128)",
                    self.tracked.len(),
                    self.bounds.path_bytes,
                ),
            ));
        }
        self.tracked.insert(path.into(), fingerprint(&metadata));
        Ok(true)
    }

    fn optional_target(&mut self, root: &Path, relative: &str) -> Result<()> {
        let path = root.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| path_failure(self.config, &path, "Cargo target has no parent"))?;
        if self.observe(parent, false, true)? {
            self.observe(&path, false, false)?;
        }
        Ok(())
    }

    fn explicit_target(&mut self, root: &Path, relative: &str) -> Result<()> {
        if relative.is_empty() || relative.contains('\0') || Path::new(relative).is_absolute() {
            return Err(path_failure(
                self.config,
                &root.join("Cargo.toml"),
                format!(
                    "Cargo target path `{relative}` must be a nonempty relative path inside the approved project",
                ),
            ));
        }
        let mut path = root.to_owned();
        for part in Path::new(relative).components() {
            match part {
                std::path::Component::Normal(name) => {
                    path.push(name);
                    if name == ".cargo" {
                        return Err(path_failure(
                            self.config,
                            &root.join(relative),
                            "Cargo target enters .cargo configuration; native snapshots exclude Cargo configuration",
                        ));
                    }
                }
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    self.observe(&path, true, true)?;
                    path.pop();
                }
                _ => {
                    return Err(path_failure(
                        self.config,
                        &root.join(relative),
                        "invalid Cargo target path",
                    ));
                }
            }
            if !path.starts_with(self.project) {
                return Err(path_failure(
                    self.config,
                    &root.join(relative),
                    "Cargo target escapes the approved project",
                ));
            }
        }
        // Preserve safe in-project ../ targets, while rejecting links in every selected ancestor.
        let mut parent = path.parent();
        while let Some(directory) = parent.filter(|directory| directory.starts_with(self.project)) {
            self.observe(directory, true, true)?;
            parent = directory.parent();
        }
        self.observe(&path, true, false)?;
        Ok(())
    }

    fn auto_targets(&mut self, root: &Path, relative: &str) -> Result<()> {
        let directory = root.join(relative);
        if !self.observe(&directory, false, true)? {
            return Ok(());
        }
        for entry in fs::read_dir(&directory).map_err(|e| {
            sandbox_path_error(self.config, "discover Cargo auto targets", &directory, e)
        })? {
            check_budget(self.config, self.bounds.started)?;
            let entry = entry.map_err(|e| {
                sandbox_path_error(self.config, "read Cargo auto target entry", &directory, e)
            })?;
            self.entries += 1;
            if self.entries > 100_000 {
                return Err(path_failure(
                    self.config,
                    &directory,
                    "Cargo auto target discovery exceeds 100000 entries",
                ));
            }
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "rs") {
                self.observe(&path, true, false)?;
            } else {
                let kind = entry.file_type().map_err(|e| {
                    sandbox_path_error(self.config, "inspect Cargo auto target entry", &path, e)
                })?;
                if kind.is_dir() || kind.is_symlink() {
                    self.observe(&path, true, true)?;
                    self.observe(&path.join("main.rs"), false, false)?;
                }
            }
        }
        Ok(())
    }

    fn package_targets(&mut self, root: &Path, value: &toml::Value) -> Result<()> {
        let Some(package) = value.get("package") else {
            return Ok(());
        };
        if let Some(library) = value.get("lib") {
            let path = library
                .get("path")
                .map(|path| {
                    path.as_str().ok_or_else(|| {
                        path_failure(
                            self.config,
                            &root.join("Cargo.toml"),
                            "Cargo lib.path must be a string",
                        )
                    })
                })
                .transpose()?
                .unwrap_or("src/lib.rs");
            self.explicit_target(root, path)?;
        } else if package.get("autolib").and_then(toml::Value::as_bool) != Some(false) {
            self.optional_target(root, "src/lib.rs")?;
        }
        for (key, automatic, directory) in [
            ("bin", "autobins", "src/bin"),
            ("test", "autotests", "tests"),
            ("bench", "autobenches", "benches"),
            ("example", "autoexamples", "examples"),
        ] {
            if let Some(targets) = value.get(key).and_then(toml::Value::as_array) {
                for target in targets {
                    if let Some(path) = target.get("path") {
                        let path = path.as_str().ok_or_else(|| {
                            path_failure(
                                self.config,
                                &root.join("Cargo.toml"),
                                format!("Cargo {key}.path must be a string"),
                            )
                        })?;
                        self.explicit_target(root, path)?;
                    } else if let Some(name) = target.get("name").and_then(toml::Value::as_str) {
                        if name.is_empty()
                            || name.contains(['/', '\\', '\0'])
                            || matches!(name, "." | "..")
                        {
                            return Err(path_failure(
                                self.config,
                                &root.join("Cargo.toml"),
                                format!(
                                    "Cargo {key} target name `{name}` must be a single path segment"
                                ),
                            ));
                        }
                        let parent = root.join(directory);
                        if self.observe(&parent, false, true)? {
                            self.observe(&parent.join(format!("{name}.rs")), false, false)?;
                            let nested = parent.join(name);
                            if self.observe(&nested, false, true)? {
                                self.observe(&nested.join("main.rs"), false, false)?;
                            }
                        }
                        if key == "bin"
                            && package.get("name").and_then(toml::Value::as_str) == Some(name)
                        {
                            self.optional_target(root, "src/main.rs")?;
                        }
                    }
                }
            }
            if package.get(automatic).and_then(toml::Value::as_bool) != Some(false) {
                if key == "bin" {
                    self.optional_target(root, "src/main.rs")?;
                }
                self.auto_targets(root, directory)?;
            }
        }
        match package.get("build") {
            Some(toml::Value::Boolean(false)) => {}
            Some(toml::Value::String(path)) => self.explicit_target(root, path)?,
            None | Some(toml::Value::Boolean(true)) => {
                self.optional_target(root, "build.rs")?;
            }
            Some(_) => {
                return Err(path_failure(
                    self.config,
                    &root.join("Cargo.toml"),
                    "Cargo package.build must be a path string or boolean",
                ));
            }
        }
        Ok(())
    }
}

fn select_native_metadata(
    config: &SandboxConfig,
    project: &Path,
    roots: &BTreeMap<PathBuf, toml::Value>,
    started: Instant,
) -> Result<CargoContext> {
    let mut selection = NativeSelection {
        config,
        project,
        tracked: BTreeMap::new(),
        bounds: SourceBounds {
            path_bytes: 0,
            started,
        },
        entries: 0,
    };
    let mut metadata = BTreeSet::new();
    for (root, value) in roots {
        selection.observe(root, true, true)?;
        let manifest = root.join("Cargo.toml");
        selection.observe(&manifest, true, false)?;
        metadata.insert(manifest);
        let lock = root.join("Cargo.lock");
        if selection.observe(&lock, false, false)? {
            metadata.insert(lock);
        }
        selection.package_targets(root, value)?;
    }
    Ok(CargoContext {
        grants: Vec::new(),
        tracked: selection.tracked.into_iter().collect(),
        directory: "/rust/project".into(),
        metadata,
    })
}

/// Stage bounded manifests/lock bytes and empty target files, never source contents.
pub(crate) fn stage_acquisition_workspace(
    config: &SandboxConfig,
    project: &Path,
    destination: &Path,
) -> Result<String> {
    crate::acquisition::validate_acquisition_config(&config.acquisition)
        .map_err(|e| failure(config, format!("invalid acquisition source policy: {e}")))?;
    stage_native_workspace(config, project, destination, SnapshotPurpose::Acquisition)
}

/// Stage read-only offline resolver inputs without granting acquisition authority.
pub(crate) fn stage_native_admission_workspace(
    config: &SandboxConfig,
    project: &Path,
    destination: &Path,
) -> Result<String> {
    stage_native_workspace(config, project, destination, SnapshotPurpose::Admission)
}

fn stage_native_workspace(
    config: &SandboxConfig,
    project: &Path,
    destination: &Path,
    purpose: SnapshotPurpose,
) -> Result<String> {
    let started = Instant::now();
    let project = project
        .canonicalize()
        .map_err(|e| sandbox_path_error(config, "resolve acquisition project", project, e))?;
    let limits = kvist_dependencies::VendorLimits::load(&project).map_err(|e| {
        failure(
            config,
            format!("cannot load native Cargo snapshot limits: {e}"),
        )
    })?;
    let context = prepare_with_policy(config, &project, &project, started, purpose)?;
    let mut lock_identity = None;
    for (source, expected) in &context.tracked {
        check_budget(config, started)?;
        let metadata = inspect(config, source)?;
        if fingerprint(&metadata) != *expected {
            return Err(path_failure(
                config,
                source,
                "inputs changed before acquisition staging; retry after concurrent edits have stopped",
            ));
        }
        let relative = source
            .strip_prefix(&project)
            .map_err(|_| failure(config, "Cargo acquisition inputs escape the project"))?;
        let target = destination.join(relative);
        if metadata.is_dir() {
            fs::create_dir_all(&target).map_err(|e| {
                sandbox_path_error(config, "stage Cargo target directory", &target, e)
            })?;
            continue;
        }
        fs::create_dir_all(
            target
                .parent()
                .ok_or_else(|| failure(config, "Cargo target has no parent"))?,
        )
        .map_err(|e| sandbox_path_error(config, "stage Cargo metadata parent", &target, e))?;
        let bytes = if context.metadata.contains(source) {
            let limit = if relative == Path::new("Cargo.lock") {
                limits.max_lockfile_bytes
            } else {
                1 << 20
            };
            read_metadata_bounded(config, source, *expected, limit)?
        } else {
            Vec::new()
        };
        if relative == Path::new("Cargo.lock") {
            if purpose == SnapshotPurpose::Acquisition {
                acquisition_lock_sources(config, source, &bytes)?;
            }
            lock_identity = Some(crate::vendoring::lockfile_digest(&bytes));
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| {
                sandbox_path_error(config, "create private Cargo metadata/target", &target, e)
            })?;
        file.write_all(&bytes).map_err(|e| {
            sandbox_path_error(config, "write private Cargo metadata/target", &target, e)
        })?;
    }
    for (path, expected) in &context.tracked {
        check_budget(config, started)?;
        if fingerprint(&inspect(config, path)?) != *expected {
            return Err(path_failure(
                config,
                path,
                "inputs changed during acquisition staging; retry after concurrent edits have stopped",
            ));
        }
    }
    let identity = lock_identity
        .ok_or_else(|| path_failure(config, &project.join("Cargo.lock"), "native Cargo snapshot requires a regular Cargo.lock; generate the project lockfile before offline admission or acquisition"))?;
    Ok(identity)
}
