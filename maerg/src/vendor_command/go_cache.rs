//! Native Go module-cache selection and explicit locked preparation.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::error::{KvistError, Result};
use crate::language_vendoring::{VendoringReport, lockfile_digest};

pub(crate) const MOUNT: &str = "/workspace/go-modules";
const INPUT_LIMIT: usize = 1024 * 1024;

fn error(project: &Path, reason: impl Into<String>) -> KvistError {
    KvistError::VendoringUnavailable {
        path: project.to_string_lossy().into_owned(),
        reason: reason.into(),
    }
}

fn run_go(project: &Path, args: &[&str], cache: Option<&Path>, offline: bool) -> Result<Vec<u8>> {
    let mut argv = vec![
        "GOTOOLCHAIN=local".to_owned(),
        "GOFLAGS=-mod=readonly".to_owned(),
        "GOWORK=off".to_owned(),
    ];
    if let Some(cache) = cache {
        let cache = cache
            .to_str()
            .ok_or_else(|| error(project, "GOMODCACHE is not valid UTF-8"))?;
        argv.push(format!("GOMODCACHE={cache}"));
    }
    if offline {
        argv.extend(["GOPROXY=off".into(), "GOSUMDB=off".into()]);
    }
    argv.push("go".into());
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let timeout = Duration::from_secs(if offline { 30 } else { 900 });
    let policy = sav::SupervisionPolicy {
        idle_timeout: timeout,
        attempt_timeout: Some(timeout),
        detect_loops: false,
        max_retries: 0,
        max_output_bytes: INPUT_LIMIT,
    };
    let command = sav::CommandSpec::new("/usr/bin/env", argv).in_directory(project);
    sav::run_supervised_capture(&policy, |_| Ok(command.clone()))
        .map(|report| report.stdout)
        .map_err(|source| {
            let redactions: Vec<_> = ["GOPROXY", "GONOPROXY", "GOSUMDB"]
                .into_iter()
                .filter_map(|name| std::env::var(name).ok())
                .collect();
            let mut detail = crate::presentation::sanitize(&crate::presentation::redact(
                &source.to_string(),
                &redactions,
            ));
            if detail.len() > 4096 {
                let mut end = 4096;
                while !detail.is_char_boundary(end) {
                    end -= 1;
                }
                detail.truncate(end);
                detail.push_str(" [truncated]");
            }
            error(project, format!("native `go {}` failed: {detail}; inspect locked inputs and cache before retrying; retained downloads were not removed", args.join(" ")))
        })
}

pub(crate) fn cache_root(project: &Path) -> Result<PathBuf> {
    let mut path = match std::env::var_os("GOMODCACHE") {
        Some(path) => PathBuf::from(path),
        None => {
            let output = run_go(project, &["env", "GOMODCACHE"], None, true)?;
            let output = String::from_utf8(output).map_err(|source| {
                error(project, format!("go env GOMODCACHE is not UTF-8: {source}"))
            })?;
            PathBuf::from(output.trim())
        }
    };
    if !path.is_absolute() {
        return Err(error(
            project,
            format!(
                "GOMODCACHE `{}` must be an absolute native cache path",
                path.display()
            ),
        ));
    }
    if path
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(error(
            project,
            "GOMODCACHE must not contain parent traversal components",
        ));
    }
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_dir() => {
            path = path.canonicalize().map_err(|source| {
                error(
                    project,
                    format!(
                        "cannot resolve native GOMODCACHE `{}`: {source}",
                        path.display()
                    ),
                )
            })?;
        }
        Ok(metadata) => {
            return Err(error(
                project,
                format!(
                    "GOMODCACHE `{}` is {}, not a regular directory",
                    path.display(),
                    if metadata.file_type().is_symlink() {
                        "a symbolic link"
                    } else {
                        "a non-directory entry"
                    }
                ),
            ));
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            path = resolve_missing_cache(project, &path)?;
        }
        Err(source) => {
            return Err(error(
                project,
                format!("cannot inspect GOMODCACHE `{}`: {source}", path.display()),
            ));
        }
    }
    let project_root = project.canonicalize().map_err(|source| {
        error(
            project,
            format!(
                "cannot resolve Go project `{}`: {source}",
                project.display()
            ),
        )
    })?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|home| home.canonicalize().ok());
    if path.parent().is_none()
        || project_root.starts_with(&path)
        || home.is_some_and(|home| home.starts_with(&path))
    {
        return Err(error(
            project,
            format!(
                "GOMODCACHE `{}` is a broad filesystem/home/project ancestor, not a dedicated native module cache",
                path.display()
            ),
        ));
    }
    Ok(path)
}

fn resolve_missing_cache(project: &Path, path: &Path) -> Result<PathBuf> {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    loop {
        match fs::metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() => break,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let name = ancestor.file_name().ok_or_else(|| {
                    error(project, "GOMODCACHE has no existing directory ancestor")
                })?;
                suffix.push(name);
                ancestor = ancestor.parent().ok_or_else(|| {
                    error(project, "GOMODCACHE has no existing directory ancestor")
                })?;
            }
            result => {
                return Err(error(
                    project,
                    format!(
                        "cannot resolve GOMODCACHE ancestor `{}`: {}",
                        ancestor.display(),
                        match result {
                            Ok(_) => "not a directory".to_owned(),
                            Err(source) => source.to_string(),
                        }
                    ),
                ));
            }
        }
    }
    let mut resolved = ancestor.canonicalize().map_err(|source| {
        error(
            project,
            format!(
                "cannot resolve GOMODCACHE ancestor `{}`: {source}",
                ancestor.display()
            ),
        )
    })?;
    resolved.extend(suffix.into_iter().rev());
    Ok(resolved)
}

fn lock_inputs(project: &Path) -> Result<(String, String)> {
    let read = |name| {
        let path = project.join(name);
        crate::file_io::read_text_bounded(&path, INPUT_LIMIT).map_err(|source| {
            error(
                project,
                format!("cannot read locked Go input `{}`: {source}", path.display()),
            )
        })
    };
    Ok((read("go.mod")?, read("go.sum")?))
}

fn digest(modfile: &str, sums: &str) -> String {
    let mut bytes = Vec::with_capacity(16 + modfile.len() + sums.len());
    bytes.extend_from_slice(&(modfile.len() as u64).to_be_bytes());
    bytes.extend_from_slice(modfile.as_bytes());
    bytes.extend_from_slice(&(sums.len() as u64).to_be_bytes());
    bytes.extend_from_slice(sums.as_bytes());
    lockfile_digest(&bytes)
}

pub(crate) fn identity(project: &Path) -> Result<String> {
    let (modfile, sums) = lock_inputs(project)?;
    Ok(digest(&modfile, &sums))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Requirement {
    path: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Replacement {
    old: Requirement,
    new: Requirement,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Module {
    #[serde(default)]
    require: Option<Vec<Requirement>>,
    #[serde(default)]
    replace: Option<Vec<Replacement>>,
}

fn escape(project: &Path, value: &str) -> Result<String> {
    if value.is_empty()
        || value
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/._-+~".contains(&byte))
    {
        return Err(error(
            project,
            format!("unsupported native Go module/version path `{value}`"),
        ));
    }
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_uppercase() {
            escaped.push('!');
            escaped.push(character.to_ascii_lowercase());
        } else {
            escaped.push(character);
        }
    }
    Ok(escaped)
}

fn present(project: &Path, path: &Path, directory: bool) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if (directory && metadata.is_dir()) || (!directory && metadata.is_file()) => {
            Ok(true)
        }
        Ok(metadata) => Err(error(
            project,
            format!(
                "native Go metadata `{}` is {}, expected {}",
                path.display(),
                if metadata.file_type().is_symlink() {
                    "a symbolic link"
                } else if metadata.is_dir() {
                    "a directory"
                } else if metadata.is_file() {
                    "a regular file"
                } else {
                    "a special entry"
                },
                if directory {
                    "a directory"
                } else {
                    "a regular file"
                }
            ),
        )),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(error(
            project,
            format!(
                "cannot inspect native Go metadata `{}`: {source}",
                path.display()
            ),
        )),
    }
}

pub(crate) fn report(project: &Path) -> Result<VendoringReport> {
    let cache = cache_root(project)?;
    let (modfile, sums) = lock_inputs(project)?;
    let expected = digest(&modfile, &sums);
    let module: Module = serde_json::from_slice(&run_go(
        project,
        &["mod", "edit", "-json"],
        Some(&cache),
        true,
    )?)
    .map_err(|source| {
        error(
            project,
            format!("cannot parse native Go manifest metadata: {source}"),
        )
    })?;
    let mut present_dependencies = 0;
    let mut missing = Vec::new();
    for requirement in module.require.as_deref().unwrap_or_default() {
        let requirement = module
            .replace
            .as_deref()
            .unwrap_or_default()
            .iter()
            .find(|replacement| {
                replacement.old.path == requirement.path
                    && (replacement.old.version.is_empty()
                        || replacement.old.version == requirement.version)
            })
            .map(|replacement| &replacement.new)
            .unwrap_or(requirement);
        if requirement.version.is_empty() {
            return Err(error(
                project,
                "local Go replace sources are unsupported by the native-cache topology; declare a separately approved source resource instead",
            ));
        }
        let module_path = escape(project, &requirement.path)?;
        let version = escape(project, &requirement.version)?;
        if version.contains('/') {
            return Err(error(
                project,
                "native Go module version must not contain a path separator",
            ));
        }
        let source = cache.join(format!("{module_path}@{version}"));
        let download = cache.join("cache/download").join(&module_path).join("@v");
        let expected_hash = sums.lines().find_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            (fields.len() == 3 && fields[0] == requirement.path && fields[1] == requirement.version)
                .then(|| fields[2])
        });
        let native_hash =
            crate::file_io::read_text_bounded(&download.join(format!("{version}.ziphash")), 256);
        let source_ready = present(project, &source, true)?
            && present(project, &download.join(format!("{version}.mod")), false)?
            && present(project, &download.join(format!("{version}.info")), false)?
            && !fs::symlink_metadata(download.join(format!("{version}.partial")))
                .map(|_| true)
                .or_else(|source| {
                    if source.kind() == std::io::ErrorKind::NotFound {
                        Ok(false)
                    } else {
                        Err(source)
                    }
                })
                .map_err(|source| {
                    error(
                        project,
                        format!(
                            "cannot inspect native Go extraction state at `{}`: {source}",
                            download.display()
                        ),
                    )
                })?;
        match native_hash {
            Ok(hash) if source_ready && expected_hash.is_some_and(|expected| expected == hash.trim()) => present_dependencies += 1,
            Ok(_) => missing.push(format!("{}@{}: native module metadata/archive checksum missing or incomplete; run `kvist vendor`", requirement.path, requirement.version)),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => missing.push(format!("{}@{}: native module cache is missing; run `kvist vendor`", requirement.path, requirement.version)),
            Err(source) => return Err(error(project, format!("cannot inspect native Go checksum `{}`: {source}", download.join(format!("{version}.ziphash")).display()))),
        }
    }
    if identity(project)? != expected {
        return Err(error(
            project,
            "go.mod/go.sum changed during native cache inspection; retry only after host edits stop",
        ));
    }
    Ok(VendoringReport {
        language: "go".into(),
        project_root: project.to_string_lossy().into_owned(),
        lockfile_path: project.join("go.sum").to_string_lossy().into_owned(),
        lockfile_digest: expected,
        vendored_dir: cache.to_string_lossy().into_owned(),
        present_dependencies,
        missing_dependencies: missing,
    })
}

pub(crate) fn populate(project: &Path) -> Result<()> {
    let cache = cache_root(project)?;
    let before = identity(project)?;
    fs::create_dir_all(&cache).map_err(|source| {
        error(
            project,
            format!(
                "cannot create native Go cache `{}`: {source}",
                cache.display()
            ),
        )
    })?;
    run_go(project, &["mod", "download"], Some(&cache), false)?;
    let after = identity(project)?;
    if after != before {
        return Err(error(
            project,
            "go.mod/go.sum changed during native acquisition; cached downloads retained; inspect the lock change before a new explicit attempt",
        ));
    }
    Ok(())
}
