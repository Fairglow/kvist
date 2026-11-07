use super::*;

pub(super) fn config() -> SandboxConfig {
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
    assert!(!destination.path().join(".cargo").exists());
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
            std::os::unix::fs::symlink("/etc/passwd", project.path().join("src/lib.rs")).unwrap();
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
