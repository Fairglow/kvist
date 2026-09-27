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
                program: locate_host_binary("go")?,
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
            // The provisioned virtualenv (mounted at `PYTHON_VENV_DEST`) makes
            // the locked wheels importable; the host interpreter is the real
            // executable, and `VIRTUAL_ENV` is the documented mechanism that
            // redirects it to the venv's site-packages. Bytecode writes and
            // the user site are disabled so the read-only venv stays clean.
            environment.insert("VIRTUAL_ENV".to_owned(), PYTHON_VENV_DEST.to_owned());
            environment.insert("PYTHONDONTWRITEBYTECODE".to_owned(), "1".to_owned());
            environment.insert("PYTHONNOUSERSITE".to_owned(), "1".to_owned());
            Ok(LanguageProfile {
                program: locate_host_binary("python3")?,
                arguments: vec!["-m".to_owned(), "unittest".to_owned(), "-v".to_owned()],
                environment,
            })
        }
        "c" => {
            // The C/C++ build system is project-defined; the approved test
            // command drives compile and test against the Conan home mounted
            // at its host path (the generated toolchain file references it).
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
            // The canonical path matches the Conan home mount destination, so
            // the absolute cache paths embedded in the generated toolchain
            // file resolve inside the sandbox even under symlinked prefixes.
            let conan_home = project_root
                .join(".kvist")
                .join(crate::language_vendoring::CONAN_HOME_DIRNAME);
            let canonical =
                conan_home
                    .canonicalize()
                    .map_err(|source| KvistError::SandboxUnavailable {
                        runner: "<language-verification>".to_owned(),
                        reason: format!(
                            "cannot canonicalize the Conan home at `{}`: {source}",
                            conan_home.display()
                        ),
                    })?;
            environment.insert(
                "CONAN_HOME".to_owned(),
                canonical.to_string_lossy().into_owned(),
            );
            Ok(LanguageProfile {
                program,
                arguments,
                environment,
            })
        }
        other => Err(KvistError::SandboxUnavailable {
            runner: "<language-verification>".to_owned(),
            reason: format!("no offline verification profile for language `{other}`"),
        }),
    }
}

/// Locate a host binary by name through the host `PATH`, returning its
/// canonical (symlink-resolved) path so the sandbox toolchain grant binds a
/// regular, non-symlink executable. Fails closed with an actionable message.
fn locate_host_binary(name: &str) -> Result<String> {
    let path_value = std::env::var("PATH").unwrap_or_else(|_| SANDBOX_PATH.to_owned());
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
                return canonical.to_str().map(str::to_owned).ok_or_else(|| {
                    KvistError::SandboxUnavailable {
                        runner: "<language-verification>".to_owned(),
                        reason: format!(
                            "host binary `{name}` resolved to non-UTF-8 path `{}`",
                            canonical.display()
                        ),
                    }
                });
            }
        }
    }
    Err(KvistError::SandboxUnavailable {
        runner: "<language-verification>".to_owned(),
        reason: format!(
            "host binary `{name}` was not found on the host PATH; install the \
             {name} toolchain before verifying this project"
        ),
    })
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
        let profile = offline_profile("python", &project, None).expect("python profile");
        assert!(profile.program.ends_with("python3.14") || profile.program.ends_with("python3"));
        assert_eq!(
            profile.arguments,
            vec!["-m".to_owned(), "unittest".to_owned(), "-v".to_owned()]
        );
        assert_eq!(
            profile.environment.get("VIRTUAL_ENV").map(String::as_str),
            Some(PYTHON_VENV_DEST)
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
