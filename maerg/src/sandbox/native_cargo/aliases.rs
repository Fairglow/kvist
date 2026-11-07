use super::*;
use std::collections::BTreeSet;
use std::os::unix::fs::OpenOptionsExt as _;

pub(super) fn selected(
    config: &SandboxConfig,
    workspace: &Path,
) -> Result<Option<Vec<SandboxNativeRegistryAlias>>> {
    let mut aliases: Vec<_> = config
        .acquisition
        .additional_sources
        .iter()
        .filter_map(|source| match source {
            crate::config::PackageSource::CargoRegistry {
                name, index_origin, ..
            } => Some((name, index_origin)),
            crate::config::PackageSource::CargoGit { .. } => None,
        })
        .collect();
    if aliases.is_empty() {
        return Ok(None);
    }
    aliases.sort_by(|left, right| left.0.cmp(right.0));
    if aliases.len() > 64 {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!(
                "native registry aliases total {} (limit 64); reduce the selected registry set",
                aliases.len()
            ),
        });
    }
    let lock = workspace.join("Cargo.lock");
    let before = rust_environment::inspect(config, &lock)?;
    if !before.is_file() || before.len() > 16 * 1024 * 1024 {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!(
                "native alias lock `{}` must be a regular file within 16 MiB (observed {}, {} bytes)",
                lock.display(),
                rust_environment::resource_kind(&before),
                before.len()
            ),
        });
    }
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
        .open(&lock)
        .map_err(|error| sandbox_path_error(config, "open native alias lock", &lock, error))?;
    if rust_environment::fingerprint(
        &file.metadata().map_err(|error| {
            sandbox_path_error(config, "inspect opened alias lock", &lock, error)
        })?,
    ) != rust_environment::fingerprint(&before)
    {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!(
                "native alias lock `{}` changed while opening; stop concurrent edits before retrying",
                lock.display()
            ),
        });
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| sandbox_path_error(config, "read native alias lock", &lock, error))?;
    let after = file
        .metadata()
        .map_err(|error| sandbox_path_error(config, "reinspect native alias lock", &lock, error))?;
    if bytes.len() > 16 * 1024 * 1024
        || rust_environment::fingerprint(&after) != rust_environment::fingerprint(&before)
    {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!(
                "native alias lock `{}` changed or exceeded 16 MiB while reading; stop concurrent edits before retrying",
                lock.display()
            ),
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(|error| KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: format!(
            "native alias lock `{}` is not UTF-8: {error}",
            lock.display()
        ),
    })?;
    let document: toml::Value =
        toml::from_str(text).map_err(|error| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!(
                "cannot parse native alias lock `{}`: {error}",
                lock.display()
            ),
        })?;
    let mut sources = BTreeSet::new();
    if let Some(packages) = document.get("package").and_then(toml::Value::as_array) {
        for package in packages {
            if let Some(value) = package.get("source") {
                sources.insert(value.as_str().ok_or_else(|| KvistError::SandboxUnavailable {
                    runner: config.runner.clone(),
                    reason: format!("native alias lock `{}` has a non-string package source; repair Cargo.lock", lock.display()),
                })?);
            }
        }
    }
    let mut selected = Vec::new();
    for (name, origin) in aliases {
        let git = sources.contains(format!("registry+{origin}").as_str());
        let sparse = sources.contains(format!("sparse+{origin}").as_str())
            || sources.contains(format!("registry+sparse+{origin}").as_str());
        if git && sparse {
            return Err(KvistError::SandboxUnavailable {
                runner: config.runner.clone(),
                reason: format!(
                    "native registry alias `{name}` matches both Git and sparse locked sources; make the locked registry protocol unambiguous"
                ),
            });
        }
        selected.push(SandboxNativeRegistryAlias {
            name: name.clone(),
            index_origin: origin.clone(),
            protocol: if git || (!sparse && !origin.ends_with('/')) {
                SandboxNativeRegistryProtocol::Git
            } else {
                SandboxNativeRegistryProtocol::Sparse
            },
        });
    }
    Ok(Some(selected))
}
