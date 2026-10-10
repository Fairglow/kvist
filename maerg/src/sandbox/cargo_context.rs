//! Explicitly approved, read-only Cargo workspace/path-dependency build context.

use super::rust_environment::{Fingerprint, check_budget, fingerprint, inspect};
use super::*;
use std::collections::BTreeSet;

pub(super) struct CargoContext {
    pub grants: Vec<SandboxGrant>,
    pub tracked: Vec<(PathBuf, Fingerprint)>,
    pub directory: String,
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
    let mut bytes = Vec::new();
    File::open(&path)
        .and_then(|file| file.take((1 << 20) + 1).read_to_end(&mut bytes))
        .map_err(|e| sandbox_error(config, "read bounded Cargo build manifest", e))?;
    if bytes.len() > 1 << 20 {
        return Err(failure(config, "Cargo manifest grew beyond 1 MiB"));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| failure(config, "Cargo build manifests must be UTF-8"))?;
    toml::from_str(text).map_err(|e| {
        failure(
            config,
            format!("invalid Cargo build manifest `{}`: {e}", path.display()),
        )
    })
}

fn dependency_paths(
    config: &SandboxConfig,
    value: &toml::Value,
    paths: &mut Vec<String>,
) -> Result<()> {
    if let Some(table) = value.as_table() {
        for dependency in table.values().filter_map(toml::Value::as_table) {
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
        if root != component
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
                dependency_paths(config, table, &mut paths)?;
            }
        }
        if let Some(targets) = value.get("target").and_then(toml::Value::as_table) {
            for target in targets.values() {
                for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
                    if let Some(table) = target.get(key) {
                        dependency_paths(config, table, &mut paths)?;
                    }
                }
            }
        }
        if let Some(patches) = value.get("patch").and_then(toml::Value::as_table) {
            for table in patches.values() {
                dependency_paths(config, table, &mut paths)?;
            }
        }
        if let Some(workspace) = value.get("workspace") {
            if let Some(table) = workspace.get("dependencies") {
                dependency_paths(config, table, &mut paths)?;
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
    for (root, value) in &roots {
        candidates.insert(root.join("Cargo.toml"));
        if root.join("Cargo.lock").exists() {
            candidates.insert(root.join("Cargo.lock"));
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
    })
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
