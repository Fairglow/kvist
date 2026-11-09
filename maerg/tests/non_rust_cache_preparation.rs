//! Focused preparation tests; fixtures are owned directories in the checkout.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use maerg::language_vendoring::{detect_language_strategy, lockfile_digest};

fn project(lock: &str) -> tempfile::TempDir {
    let project = tempfile::Builder::new()
        .prefix(".non-rust-cache-test-")
        .tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("owned fixture directory");
    fs::write(project.path().join(lock), "{}\n").expect("lock");
    if lock == "conanfile.lock" {
        fs::create_dir_all(project.path().join(".kvist/vendored-conan/p")).expect("Conan home");
    }
    project
}

fn config_path(project: &Path, lock: &str) -> PathBuf {
    project.join(match lock {
        "requirements.lock.txt" => ".kvist/sandbox-pip/pip.conf",
        "package-lock.json" => ".kvist/sandbox-npm/.npmrc",
        "conanfile.lock" => ".kvist/sandbox-conan/OFFLINE.md",
        _ => panic!("unsupported test lock"),
    })
}

#[test]
fn offline_verification_missing_store_never_runs_host_installer() {
    use std::os::unix::fs::PermissionsExt;
    let project = project("package-lock.json");
    fs::write(
        project.path().join("package.json"),
        "{\"name\":\"fixture\",\"version\":\"1.0.0\"}",
    )
    .unwrap();
    let tools = project.path().join("tools");
    fs::create_dir(&tools).unwrap();
    let npm = tools.join("npm");
    fs::write(
        &npm,
        "#!/bin/sh\nprintf launched > \"$KVIST_INSTALL_MARKER\"\nexit 42\n",
    )
    .unwrap();
    fs::set_permissions(npm, fs::Permissions::from_mode(0o755)).unwrap();
    let marker = project.path().join("host-install");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "offline_verification_missing_store_child",
            "--nocapture",
        ])
        .env("KVIST_MISSING_STORE_PROJECT", project.path())
        .env("KVIST_INSTALL_MARKER", &marker)
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .output()
        .unwrap();
    assert!(
        !marker.exists(),
        "offline verification invoked a host installer"
    );
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn offline_verification_missing_store_child() {
    let Some(project) = std::env::var_os("KVIST_MISSING_STORE_PROJECT") else {
        return;
    };
    use maerg::config::{AcquisitionConfig, SandboxConfig, VcsSelection};
    use maerg::sandbox::{BackendIdentity, ExecutionOptions, SandboxProbe};
    let config = SandboxConfig {
        runner: "/missing/native-runner".into(),
        backend: "/usr/bin/bwrap".into(),
        environment_allowlist: Vec::new(),
        acquisition: AcquisitionConfig::default(),
    };
    let probe = SandboxProbe {
        runner_path: config.runner.clone(),
        runner_digest: lockfile_digest(b"runner"),
        backend: BackendIdentity {
            kind: "bubblewrap".into(),
            path: config.backend.clone(),
            digest: lockfile_digest(b"backend"),
        },
    };
    let project = PathBuf::from(project);
    let error = maerg::language_verification::run_offline_language_verification(
        &config,
        &project,
        VcsSelection::Git,
        &probe,
        &lockfile_digest(b"policy"),
        &project,
        None,
        ExecutionOptions::default(),
    )
    .err()
    .expect("unprepared offline verification must fail");
    assert!(
        matches!(error, maerg::KvistError::VendoringIncomplete { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("kvist vendor"), "{error}");
}

#[test]
fn non_rust_cache_identity_uses_the_provisioning_managers_lock() {
    for (locks, selected) in [
        (vec!["package-lock.json", "yarn.lock"], "yarn.lock"),
        (
            vec!["package-lock.json", "yarn.lock", "pnpm-lock.yaml"],
            "pnpm-lock.yaml",
        ),
    ] {
        let project = project(locks[0]);
        for lock in &locks {
            fs::write(project.path().join(lock), format!("distinct {lock}\n")).expect("lock");
        }
        fs::create_dir_all(project.path().join(".kvist/vendored-js")).expect("native cache");
        fs::write(
            project.path().join(".kvist/vendored-js/.seed"),
            "native cache",
        )
        .expect("seed");

        let strategy = detect_language_strategy(project.path()).expect("JavaScript strategy");
        let report = strategy.enforce(project.path()).expect("readiness report");
        assert_eq!(
            Path::new(&report.lockfile_path).file_name().unwrap(),
            selected
        );
        assert_eq!(
            report.lockfile_digest,
            lockfile_digest(&fs::read(project.path().join(selected)).expect("selected lock"))
        );
        let mounts = strategy
            .mounts(project.path(), &report.lockfile_digest)
            .expect("mounts");
        assert!(
            mounts
                .iter()
                .all(|mount| mount.identity == report.lockfile_digest)
        );
    }
}

#[test]
fn non_rust_cache_preparation_reuses_identical_configuration() {
    for lock in [
        "requirements.lock.txt",
        "package-lock.json",
        "conanfile.lock",
    ] {
        let project = project(lock);
        let strategy = detect_language_strategy(project.path()).expect("strategy");
        strategy
            .mounts(project.path(), "sha256:test")
            .expect("first preparation");
        let config = config_path(project.path(), lock);
        let fixed_time = UNIX_EPOCH + Duration::from_secs(60);
        fs::File::open(&config)
            .expect("generated config")
            .set_times(fs::FileTimes::new().set_modified(fixed_time))
            .expect("fixed modification time");

        strategy
            .mounts(project.path(), "sha256:test")
            .expect("repeat preparation");
        assert_eq!(
            fs::metadata(&config)
                .expect("config metadata")
                .modified()
                .expect("mtime"),
            fixed_time,
            "identical {lock} configuration must not be rewritten"
        );

        let expected = fs::read(&config).expect("expected configuration");
        fs::write(&config, "stale configuration").expect("stale configuration");
        strategy
            .mounts(project.path(), "sha256:test")
            .expect("repair preparation");
        assert_eq!(fs::read(config).expect("repaired configuration"), expected);
    }
}

#[test]
fn non_rust_cache_preparation_rejects_special_and_hardlinked_configuration() {
    for lock in [
        "requirements.lock.txt",
        "package-lock.json",
        "conanfile.lock",
    ] {
        for kind in ["directory", "FIFO", "hard links"] {
            let project = project(lock);
            let config = config_path(project.path(), lock);
            fs::create_dir_all(config.parent().expect("config parent")).expect("config directory");
            let target = project.path().join("untouched");
            match kind {
                "directory" => fs::create_dir(&config).expect("selected directory"),
                "FIFO" => nix::unistd::mkfifo(
                    &config,
                    nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
                )
                .expect("selected FIFO"),
                _ => {
                    fs::write(&target, "retained bytes").expect("target");
                    fs::hard_link(&target, &config).expect("hard-linked config");
                }
            }
            let strategy = detect_language_strategy(project.path()).expect("strategy");
            let error = strategy
                .mounts(project.path(), "sha256:test")
                .expect_err("invalid selected configuration");
            assert!(error.to_string().contains(kind), "{error}");
            if target.exists() {
                assert_eq!(
                    fs::read_to_string(target).expect("retained target"),
                    "retained bytes"
                );
            }
        }
    }
}

#[test]
fn non_rust_cache_preparation_rejects_configuration_links_without_target_writes() {
    use std::os::unix::fs::symlink;

    for lock in [
        "requirements.lock.txt",
        "package-lock.json",
        "conanfile.lock",
    ] {
        for link_directory in [false, true] {
            let project = project(lock);
            let config = config_path(project.path(), lock);
            let config_dir = config.parent().expect("config parent");
            let target = project.path().join("operator-owned");
            fs::create_dir_all(config_dir.parent().expect(".kvist")).expect(".kvist");
            if link_directory {
                fs::create_dir(&target).expect("target directory");
                fs::write(target.join(config.file_name().unwrap()), "do not overwrite")
                    .expect("target");
                symlink(&target, config_dir).expect("directory link");
            } else {
                fs::create_dir(config_dir).expect("config directory");
                fs::write(&target, "do not overwrite").expect("target file");
                symlink(&target, &config).expect("file link");
            }

            let strategy = detect_language_strategy(project.path()).expect("strategy");
            let error = strategy
                .mounts(project.path(), "sha256:test")
                .expect_err("selected links must be refused");
            assert!(error.to_string().contains("symbolic link"), "{error}");
            let target_file = if link_directory {
                target.join(config.file_name().unwrap())
            } else {
                target
            };
            assert_eq!(
                fs::read_to_string(target_file).expect("target"),
                "do not overwrite"
            );
        }
    }
}
