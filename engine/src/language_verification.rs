//! Offline, network-denied verification topologies for the non-Rust supported
//! languages (ADR-0011, ADR-0013).
//!
//! Rust builds use the closed Cargo topology (`run_offline_cargo_verification`).
//! Every other supported language uses the generic system-toolchain
//! verification path extended with the language's vendored read-only mounts and
//! one writable scratch: the vendored material (Go's committed `vendor/`, the
//! npm/yarn cache, the provisioned Python venv, the Conan home) is mounted
//! read-only, a disjoint scratch absorbs caches and build output, the network
//! is denied, and the language's canonical offline test command runs against
//! the host system toolchain, which the shared runner already binds read-only
//! (`/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`).
//!
//! The same fail-closed, lock-file-digest-identified model the Rust path uses
//! applies: vendoring is enforced before any sandbox work, and a sandbox
//! infrastructure failure never falls back to a host build.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::{SandboxConfig, VcsSelection};
use crate::error::{KvistError, Result};
use crate::sandbox::{
    ExecutionOptions, ExecutionRequest, ExecutionResult, ReadOnlyMount, SandboxProbe,
    execute_with_timeout, runner_identity,
};

/// Fixed sandbox destination of the provisioned Python virtualenv.
const PYTHON_VENV_DEST: &str = "/workspace/venv";
/// The deterministic sandbox `PATH`: the host system toolchains the runner
/// already binds read-only. No host search path is leaked into the sandbox.
const SANDBOX_PATH: &str = "/usr/bin:/bin";
/// The host system prefixes the sandbox runner binds read-only (the runner's
/// system layout: `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`). A host binary is
/// only usable inside the sandbox — and for Go, so is its standard library
/// under `GOROOT` — when it lives under one of them.
const BOUND_SYSTEM_PREFIXES: &[&str] = &["/usr", "/lib", "/lib64", "/bin", "/sbin"];
/// Sandbox destination of the writable scratch, shared with the Cargo topology.
const SCRATCH_DEST: &str = crate::sandbox::CARGO_SCRATCH_DEST;

/// Run offline, network-denied verification for a vendored non-Rust project.
///
/// Enforces vendoring readiness (failing closed when the project is not
/// vendored, the lockfile drifted, or material is missing), then runs the
/// language's canonical offline test command — or the approved `test_command`
/// when one is supplied (the C/C++ build-system path) — with the vendored
/// mounts read-only and a disjoint writable scratch.
#[allow(clippy::too_many_arguments)]
pub fn run_offline_language_verification(
    config: &SandboxConfig,
    project_root: &Path,
    vcs_selection: VcsSelection,
    probe: &SandboxProbe,
    policy_identity: &str,
    component_dir: &Path,
    test_command: Option<Vec<String>>,
    options: ExecutionOptions,
) -> Result<ExecutionResult> {
    let enforcement = crate::language_vendoring::enforce_vendoring(project_root)?;
    if enforcement.language == "rust" {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "verification routed to language verification but the project is Rust; \
                     the closed Cargo topology owns Rust builds"
                .to_owned(),
        });
    }

    let profile = offline_profile(&enforcement.language, project_root, test_command.as_deref())?;

    let mounts: Vec<ReadOnlyMount> = enforcement
        .mounts
        .iter()
        .map(|mount| ReadOnlyMount {
            source: mount.source.clone(),
            destination: mount.destination.clone(),
            identity: Some(mount.identity.clone()),
        })
        .collect();

    let scratch = tempfile::tempdir().map_err(|source| KvistError::Io {
        operation: "create writable scratch for offline language verification",
        path: PathBuf::from("."),
        source,
    })?;
    // Some toolchains (Go in particular) require their cache and tmp
    // directories to exist before use; the standard scratch layout is
    // pre-created so every language profile finds its directories ready.
    for child in ["home", "tmp", "gocache", "gopath", "npm-cache"] {
        std::fs::create_dir_all(scratch.path().join(child)).map_err(|source| KvistError::Io {
            operation: "create writable scratch subdirectory for offline language verification",
            path: scratch.path().join(child),
            source,
        })?;
    }

    tracing::info!(
        language = %enforcement.language,
        component = %component_dir.display(),
        vendored = %enforcement.vendored_dir.display(),
        "executing offline language verification"
    );

    let expected_runner = runner_identity(config, project_root, vcs_selection)
        .map_err(|source| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!(
                "cannot re-identify the trusted sandbox runner for offline language verification: {source}"
            ),
        })?;

    let request = ExecutionRequest {
        project_root,
        vcs_selection,
        component_dir,
        phase: crate::sandbox::ExecutionPhase::Verification,
        program: &profile.program,
        arguments: &profile.arguments,
        environment: profile.environment,
        read_only_mounts: &mounts,
        scratch_host_dir: Some(scratch.path()),
        backend: &probe.backend,
        policy_identity,
    };
    execute_with_timeout(config, request, options, &expected_runner)
}

/// The sandbox-side program, arguments, and environment for one language.
struct LanguageProfile {
    program: String,
    arguments: Vec<String>,
    environment: BTreeMap<String, String>,
}

/// Build the canonical offline test command and environment for a language.
///
/// C/C++ accepts an approved project test command (a build system such as
/// `make` drives the compile-and-test cycle); Go, JavaScript, and Python use
/// their canonical offline commands, so verification is the same for every
/// project in the language.
fn offline_profile(
    language: &str,
    project_root: &Path,
    test_command: Option<&[String]>,
) -> Result<LanguageProfile> {
    let mut environment: BTreeMap<String, String> = BTreeMap::new();
    environment.insert("HOME".to_owned(), format!("{SCRATCH_DEST}/home"));
    environment.insert("PATH".to_owned(), SANDBOX_PATH.to_owned());

    match language {
        "go" | "javascript" | "python" if test_command.is_some() => {
            Err(KvistError::SandboxUnavailable {
                runner: "<language-verification>".to_owned(),
                reason: format!(
                    "the {language} offline profile verifies with its canonical \
                     test command; an approved test command drives only the C/C++ \
                     build system"
                ),
            })
        }
        "go" => {
            // `go test -mod=vendor` builds entirely from the committed vendor/
            // directory; `GOPROXY=off` plus `GOTOOLCHAIN=local` make any
            // accidental fetch or toolchain download fail immediately instead
            // of reaching for the network.
            //
            // The profile runs against the host toolchain, so the binary and
            // its standard library must both be reachable inside the sandbox,
            // where only the bound system prefixes are visible. `GOROOT` is
            // always the grandparent of `bin/go`; toolchain managers frequently
            // put an install ahead of the system one on `PATH` (e.g.
            // `/opt/hostedtoolcache/go/...` on CI runners), and a GOROOT
            // outside the bound prefixes leaves the sandbox with a go binary
            // it cannot compile with (`package bytes is not in std`).
            // `locate_host_binary` prefers a candidate under a bound prefix;
            // the guard below fails closed with an actionable message when no
            // such install exists.
            let program = locate_host_binary("go")?;
            let goroot = Path::new(&program)
                .parent()
                .and_then(|parent| parent.parent())
                .ok_or_else(|| KvistError::SandboxUnavailable {
                    runner: "<language-verification>".to_owned(),
                    reason: format!("cannot derive GOROOT from the go binary path `{}`", program),
                })?;
            if !under_bound_system_prefix(goroot) {
                return Err(KvistError::SandboxUnavailable {
                    runner: "<language-verification>".to_owned(),
                    reason: format!(
                        "the go toolchain at `{program}` keeps its standard library at \
                         GOROOT `{}`, which the sandbox cannot reach (it binds only {}); \
                         install a go toolchain under one of those prefixes",
                        goroot.display(),
                        BOUND_SYSTEM_PREFIXES.join(", ")
                    ),
                });
            }
            environment.insert("GOROOT".to_owned(), goroot.to_string_lossy().into_owned());
            environment.insert("GOPROXY".to_owned(), "off".to_owned());
            environment.insert("GOTOOLCHAIN".to_owned(), "local".to_owned());
            environment.insert("GOFLAGS".to_owned(), "-mod=vendor".to_owned());
            environment.insert("GOCACHE".to_owned(), format!("{SCRATCH_DEST}/gocache"));
            environment.insert("GOTMPDIR".to_owned(), format!("{SCRATCH_DEST}/tmp"));
            environment.insert("GOPATH".to_owned(), format!("{SCRATCH_DEST}/gopath"));
            environment.insert(
                "GOMODCACHE".to_owned(),
                format!("{SCRATCH_DEST}/gopath/pkg/mod"),
            );
            Ok(LanguageProfile {
                program,
                arguments: vec![
                    "test".to_owned(),
                    "-mod=vendor".to_owned(),
                    "./...".to_owned(),
                ],
                environment,
            })
        }
        "javascript" => {
            // `node --test` runs the project's node:test suites from the
            // committed node_modules (provisioned on the host from the vendored
            // package cache); node never reaches the network for this.
            Ok(LanguageProfile {
                program: locate_host_binary("node")?,
                arguments: vec!["--test".to_owned()],
                environment,
            })
        }
        "python" => {
            // The provisioned virtualenv (mounted at `PYTHON_VENV_DEST`) is the
            // offline runtime: CPython discovers a virtualenv from the
            // `pyvenv.cfg` beside its own executable, not from `VIRTUAL_ENV`,
            // so the host interpreter is pointed at the venv's site-packages
            // explicitly through `PYTHONPATH` (with `VIRTUAL_ENV` set for the
            // tools that do read it). Bytecode writes and the user site are
            // disabled so the read-only venv stays clean.
            environment.insert("VIRTUAL_ENV".to_owned(), PYTHON_VENV_DEST.to_owned());
            environment.insert("PYTHONPATH".to_owned(), venv_site_packages(project_root)?);
            environment.insert("PYTHONDONTWRITEBYTECODE".to_owned(), "1".to_owned());
            environment.insert("PYTHONNOUSERSITE".to_owned(), "1".to_owned());
            Ok(LanguageProfile {
                program: locate_host_binary("python3")?,
                arguments: vec!["-m".to_owned(), "unittest".to_owned(), "-v".to_owned()],
                environment,
            })
        }
        // The C/C++ build system is project-defined; the approved test command
        // drives compile and test against the package manager's vendored root
        // mounted at its canonical host path (the generated toolchain file's
        // absolute cache paths then resolve inside the sandbox even under
        // symlinked prefixes). Conan and vcpkg share this shape.
        "c" => c_package_manager_profile(
            &mut environment,
            project_root,
            test_command,
            "CONAN_HOME",
            crate::language_vendoring::CONAN_HOME_DIRNAME,
        ),
        "c-vcpkg" => c_package_manager_profile(
            &mut environment,
            project_root,
            test_command,
            "VCPKG_ROOT",
            crate::language_vendoring::VCPKG_ROOT_DIRNAME,
        ),
        other => Err(KvistError::SandboxUnavailable {
            runner: "<language-verification>".to_owned(),
            reason: format!("no offline verification profile for language `{other}`"),
        }),
    }
}

/// Build an approved-test-command profile for a C/C++ package manager whose
/// vendored root is mounted read-only at its canonical host path: resolve the
/// first program of the approved command, and set the package-manager root
/// environment variable (`CONAN_HOME`, `VCPKG_ROOT`) to the canonicalized
/// vendored root so the generated toolchain file's absolute cache paths resolve
/// inside the sandbox even under symlinked prefixes. Fails closed when no
/// approved test command is supplied (the build system is project-defined).
fn c_package_manager_profile(
    environment: &mut BTreeMap<String, String>,
    project_root: &Path,
    test_command: Option<&[String]>,
    root_env_var: &str,
    vendored_dirname: &str,
) -> Result<LanguageProfile> {
    let (program, arguments) = match test_command {
        Some(command) if !command.is_empty() => {
            let Some((program, arguments)) = command.split_first() else {
                unreachable!("split_first of a non-empty slice");
            };
            (program.to_owned(), arguments.to_vec())
        }
        _ => {
            return Err(KvistError::SandboxUnavailable {
                runner: "<language-verification>".to_owned(),
                reason: "C/C++ verification requires an approved test command from the \
                         project [test_policy] (for example `make test`)"
                    .to_owned(),
            });
        }
    };
    let vendored = project_root.join(".kvist").join(vendored_dirname);
    let canonical = vendored
        .canonicalize()
        .map_err(|source| KvistError::SandboxUnavailable {
            runner: "<language-verification>".to_owned(),
            reason: format!(
                "cannot canonicalize the vendored {vendored_dirname} at `{}`: {source}",
                vendored.display()
            ),
        })?;
    environment.insert(
        root_env_var.to_owned(),
        canonical.to_string_lossy().into_owned(),
    );
    Ok(LanguageProfile {
        program,
        arguments,
        environment: std::mem::take(environment),
    })
}

/// The sandbox path of the provisioned venv's site-packages directory.
///
/// The venv layout is deterministic (`lib/python<major.minor>/site-packages`) and
/// `kvist vendor` creates the venv from the host interpreter, so the version
/// directory is discovered from the on-disk venv and mapped to the fixed
/// sandbox destination the venv is mounted at.
fn venv_site_packages(project_root: &Path) -> Result<String> {
    let lib = project_root
        .join(".kvist")
        .join(crate::language_vendoring::PYTHON_VENV_DIRNAME)
        .join("lib");
    let entries = std::fs::read_dir(&lib).map_err(|source| KvistError::SandboxUnavailable {
        runner: "<language-verification>".to_owned(),
        reason: format!(
            "cannot read the provisioned venv lib directory `{}`: {source}",
            lib.display()
        ),
    })?;
    let mut version_dirs: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| KvistError::SandboxUnavailable {
            runner: "<language-verification>".to_owned(),
            reason: format!(
                "cannot read the provisioned venv lib directory `{}`: {source}",
                lib.display()
            ),
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("python") && entry.path().is_dir() {
            version_dirs.push(name);
        }
    }
    if version_dirs.len() != 1 {
        return Err(KvistError::SandboxUnavailable {
            runner: "<language-verification>".to_owned(),
            reason: format!(
                "the provisioned venv lib directory `{}` must contain exactly one \
                 python version directory, found {version_dirs:?}",
                lib.display()
            ),
        });
    }
    Ok(format!(
        "{PYTHON_VENV_DEST}/lib/{}/site-packages",
        version_dirs
            .pop()
            .expect("version_dirs has exactly one entry when the count is checked above")
    ))
}

/// True when `path` is one of the bound system prefixes or inside one of
/// them (component-wise, so `/usr2` is not inside `/usr`).
fn under_bound_system_prefix(path: &Path) -> bool {
    BOUND_SYSTEM_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// Choose the toolchain candidate to run in the sandbox: the first PATH
/// candidate under a bound system prefix (reachable inside the sandbox),
/// falling back to the first candidate overall when none is reachable. The
/// fallback keeps the previous behavior on hosts whose only toolchain install
/// sits outside the bound prefixes; language profiles that need more of the
/// toolchain than the granted executable (Go's `GOROOT`) fail closed on their
/// own.
fn select_toolchain_candidate(candidates: &[PathBuf]) -> Option<&Path> {
    let first = candidates.first()?;
    Some(
        candidates
            .iter()
            .find(|candidate| under_bound_system_prefix(candidate))
            .unwrap_or(first)
            .as_path(),
    )
}

/// Locate a host binary by name through the host `PATH`, returning its
/// canonical (symlink-resolved) path so the sandbox toolchain grant binds a
/// regular, non-symlink executable. Prefers a candidate under a bound system
/// prefix, because only those remain reachable inside the sandbox; PATH
/// frequently lists toolchain-manager installs (e.g.
/// `/opt/hostedtoolcache/...`) ahead of the system toolchain. Fails closed
/// with an actionable message.
fn locate_host_binary(name: &str) -> Result<String> {
    let path_value = std::env::var("PATH").unwrap_or_else(|_| SANDBOX_PATH.to_owned());
    let mut candidates: Vec<PathBuf> = Vec::new();
    for directory in path_value.split(':') {
        if directory.is_empty() {
            continue;
        }
        let candidate = Path::new(directory).join(name);
        let metadata = std::fs::symlink_metadata(&candidate);
        let Ok(metadata) = metadata else {
            continue;
        };
        // PATH entries are frequently symlinks (`/usr/bin/go`); accept files
        // and links-to-files, then canonicalize to the regular executable.
        if metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            let canonical =
                candidate
                    .canonicalize()
                    .map_err(|source| KvistError::SandboxUnavailable {
                        runner: "<language-verification>".to_owned(),
                        reason: format!(
                            "cannot canonicalize host binary `{name}` at `{}`: {source}",
                            candidate.display()
                        ),
                    })?;
            let canonical_metadata = std::fs::symlink_metadata(&canonical).map_err(|source| {
                KvistError::SandboxUnavailable {
                    runner: "<language-verification>".to_owned(),
                    reason: format!("cannot inspect host binary `{name}`: {source}"),
                }
            })?;
            if canonical_metadata.file_type().is_file() {
                candidates.push(canonical);
            }
        }
    }
    match select_toolchain_candidate(&candidates) {
        Some(canonical) => {
            canonical
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| KvistError::SandboxUnavailable {
                    runner: "<language-verification>".to_owned(),
                    reason: format!(
                        "host binary `{name}` resolved to non-UTF-8 path `{}`",
                        canonical.display()
                    ),
                })
        }
        None => Err(KvistError::SandboxUnavailable {
            runner: "<language-verification>".to_owned(),
            reason: format!(
                "host binary `{name}` was not found on the host PATH; install the \
                 {name} toolchain before verifying this project"
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(root: &std::path::Path, name: &str) -> std::path::PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).expect("create project dir");
        dir
    }

    #[test]
    fn go_profile_uses_vendor_mod_and_offline_proxy() {
        let root = tempfile::tempdir().expect("temp root");
        let project = project(root.path(), "go");
        let profile = offline_profile("go", &project, None).expect("go profile");
        assert_eq!(
            profile.program,
            locate_host_binary("go").unwrap_or_default()
        );
        assert_eq!(
            profile.arguments,
            vec![
                "test".to_owned(),
                "-mod=vendor".to_owned(),
                "./...".to_owned()
            ]
        );
        assert_eq!(
            profile.environment.get("GOPROXY").map(String::as_str),
            Some("off")
        );
        assert_eq!(
            profile.environment.get("GOTOOLCHAIN").map(String::as_str),
            Some("local")
        );
        assert!(profile.environment.contains_key("GOCACHE"));
        assert!(profile.environment.contains_key("GOTMPDIR"));
    }

    #[test]
    fn toolchain_candidate_prefers_a_bound_system_prefix() {
        let outside = PathBuf::from("/opt/hostedtoolcache/go/1.24.13/x64/bin/go");
        let inside = PathBuf::from("/usr/local/go/bin/go");
        // A sandbox-reachable candidate beats an earlier unreachable one.
        assert_eq!(
            select_toolchain_candidate(&[outside.clone(), inside.clone()]).map(PathBuf::from),
            Some(inside.clone())
        );
        // A single reachable candidate is kept.
        assert_eq!(
            select_toolchain_candidate(std::slice::from_ref(&inside)).map(PathBuf::from),
            Some(inside.clone())
        );
        // A single unreachable candidate is kept as the documented fallback.
        assert_eq!(
            select_toolchain_candidate(std::slice::from_ref(&outside)).map(PathBuf::from),
            Some(outside)
        );
        // No candidate at all is not a selection.
        assert!(select_toolchain_candidate(&[]).is_none());
    }

    #[test]
    fn under_bound_prefix_is_component_wise() {
        assert!(under_bound_system_prefix(Path::new(
            "/usr/lib/go-1.22/bin/go"
        )));
        assert!(under_bound_system_prefix(Path::new("/usr")));
        assert!(!under_bound_system_prefix(Path::new("/usr2/go/bin/go")));
        assert!(!under_bound_system_prefix(Path::new("/usrx")));
        assert!(!under_bound_system_prefix(Path::new(
            "/opt/hostedtoolcache/go/bin/go"
        )));
    }

    #[test]
    fn javascript_profile_runs_node_test() {
        let root = tempfile::tempdir().expect("temp root");
        let project = project(root.path(), "js");
        let profile = offline_profile("javascript", &project, None).expect("javascript profile");
        assert!(profile.program.ends_with("node"));
        assert_eq!(profile.arguments, vec!["--test".to_owned()]);
    }

    #[test]
    fn python_profile_points_at_the_provisioned_venv() {
        let root = tempfile::tempdir().expect("temp root");
        let project = project(root.path(), "py");
        // The profile discovers the venv's version directory on disk; the
        // sandbox path is the fixed mount destination plus the layout.
        let site = project
            .join(".kvist")
            .join("venv")
            .join("lib")
            .join("python3.14")
            .join("site-packages");
        std::fs::create_dir_all(&site).expect("venv site-packages");
        let profile = offline_profile("python", &project, None).expect("python profile");
        // The host `python3` is normally a symlink to `python3.<version>`; the
        // resolved interpreter version differs between hosts (3.14 locally,
        // 3.12 on the CI runner), so match the basename prefix rather than a
        // specific patch version.
        let program = profile
            .program
            .rsplit('/')
            .next()
            .expect("program basename");
        assert!(
            program.starts_with("python3"),
            "python profile must resolve to a python3 interpreter, got `{}`",
            profile.program
        );
        assert_eq!(
            profile.arguments,
            vec!["-m".to_owned(), "unittest".to_owned(), "-v".to_owned()]
        );
        assert_eq!(
            profile.environment.get("VIRTUAL_ENV").map(String::as_str),
            Some(PYTHON_VENV_DEST)
        );
        assert_eq!(
            profile.environment.get("PYTHONPATH").map(String::as_str),
            Some("/workspace/venv/lib/python3.14/site-packages")
        );
    }

    #[test]
    fn c_profile_requires_an_approved_test_command() {
        let root = tempfile::tempdir().expect("temp root");
        let project = project(root.path(), "c");
        let missing = offline_profile("c", &project, None);
        assert!(missing.is_err(), "C without a test command fails closed");
        // A provisioned Conan home is present when the profile is built in
        // production (enforcement runs first); the profile canonicalizes it
        // so CONAN_HOME matches the mount destination.
        let conan_home = project.join(".kvist").join("vendored-conan");
        std::fs::create_dir_all(&conan_home).expect("conan home dir");
        let with_command =
            offline_profile("c", &project, Some(&["make".to_owned(), "test".to_owned()]))
                .expect("c profile with command");
        assert_eq!(with_command.program, "make");
        assert_eq!(with_command.arguments, vec!["test".to_owned()]);
        let expected = conan_home.canonicalize().expect("canonical conan home");
        assert_eq!(
            with_command
                .environment
                .get("CONAN_HOME")
                .map(String::as_str),
            Some(expected.to_str().unwrap())
        );
    }

    #[test]
    fn c_vcpkg_profile_requires_an_approved_test_command() {
        let root = tempfile::tempdir().expect("temp root");
        let project = project(root.path(), "c-vcpkg");
        let missing = offline_profile("c-vcpkg", &project, None);
        assert!(
            missing.is_err(),
            "vcpkg without a test command fails closed"
        );
        // A provisioned vcpkg root is present when the profile is built in
        // production (enforcement runs first); the profile canonicalizes it so
        // VCPKG_ROOT matches the mount destination.
        let vcpkg_root = project.join(".kvist").join("vendored-vcpkg");
        std::fs::create_dir_all(&vcpkg_root).expect("vcpkg root dir");
        let with_command = offline_profile(
            "c-vcpkg",
            &project,
            Some(&["make".to_owned(), "test".to_owned()]),
        )
        .expect("vcpkg profile with command");
        assert_eq!(with_command.program, "make");
        assert_eq!(with_command.arguments, vec!["test".to_owned()]);
        let expected = vcpkg_root.canonicalize().expect("canonical vcpkg root");
        assert_eq!(
            with_command
                .environment
                .get("VCPKG_ROOT")
                .map(String::as_str),
            Some(expected.to_str().unwrap())
        );
    }

    #[test]
    fn canonical_profiles_reject_an_approved_test_command() {
        let root = tempfile::tempdir().expect("temp root");
        let project = project(root.path(), "canonical");
        for language in ["go", "javascript", "python"] {
            assert!(
                offline_profile(
                    language,
                    &project,
                    Some(&["make".to_owned(), "test".to_owned()])
                )
                .is_err(),
                "{language} must fail closed with an approved test command"
            );
        }
    }

    #[test]
    fn unknown_language_fails_closed() {
        let root = tempfile::tempdir().expect("temp root");
        let project = project(root.path(), "other");
        assert!(offline_profile("jvm", &project, None).is_err());
    }
}
