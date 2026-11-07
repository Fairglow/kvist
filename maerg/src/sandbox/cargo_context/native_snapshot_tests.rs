use super::tests::config;
use super::*;

fn fixture() -> tempfile::TempDir {
    let project = tempfile::Builder::new()
        .prefix(".native-snapshot-fixture-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    fs::write(
        project.path().join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(project.path().join("Cargo.lock"), "version=4\n").unwrap();
    project
}

fn destination() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(".native-snapshot-output-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap()
}

#[test]
fn native_snapshot_ignores_unrelated_socket_and_deep_module_tree() {
    let project = fixture();
    let output = destination();
    fs::create_dir(project.path().join("src")).unwrap();
    fs::write(project.path().join("src/lib.rs"), "PRIVATE_SOURCE").unwrap();
    let socket = project.path().join("control.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let mut deep = project.path().join("src/modules");
    for _ in 0..130 {
        deep.push("nested");
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("implementation.rs"), "PRIVATE_MODULE").unwrap();
    stage_acquisition_workspace(&config(), project.path(), output.path()).unwrap();
    assert_eq!(fs::read(output.path().join("src/lib.rs")).unwrap(), b"");
    assert!(!output.path().join("src/modules").exists());
    assert!(!output.path().join("control.sock").exists());
    assert!(fs::symlink_metadata(&socket).is_ok());
    let authoring = prepare(&config(), project.path(), project.path(), Instant::now())
        .err()
        .unwrap()
        .to_string();
    assert!(authoring.contains(socket.to_str().unwrap()), "{authoring}");
    assert!(authoring.contains("Unix socket"), "{authoring}");
}

#[test]
fn native_snapshot_preserves_declared_and_auto_targets_and_local_graph() {
    let project = fixture();
    let output = destination();
    let manifest = "[workspace]\nmembers=['member']\n[workspace.package]\nversion='0.1.0'\n\
        [package]\nname='fixture'\nversion='0.1.0'\nbuild='tools/build.rs'\n\
        [lib]\npath='custom/library.rs'\n[[bin]]\nname='declared'\npath='custom/program.rs'\n\
        [[test]]\nname='declared'\npath='custom/test.rs'\n[[bench]]\nname='declared'\npath='custom/bench.rs'\n\
        [[example]]\nname='declared'\npath='custom/example.rs'\n[dependencies]\nprovider={path='provider'}\n";
    fs::write(project.path().join("Cargo.toml"), manifest).unwrap();
    for (root, contents) in [
        (
            "member",
            "[package]\nname='member'\nversion.workspace=true\n[lib]\npath='../shared/member.rs'\n",
        ),
        ("provider", "[package]\nname='provider'\nversion='0.1.0'\n"),
    ] {
        fs::create_dir(project.path().join(root)).unwrap();
        fs::write(project.path().join(root).join("Cargo.toml"), contents).unwrap();
    }
    let targets = [
        "custom/library.rs",
        "custom/program.rs",
        "custom/test.rs",
        "custom/bench.rs",
        "custom/example.rs",
        "tools/build.rs",
        "src/main.rs",
        "src/bin/auto.rs",
        "src/bin/nested/main.rs",
        "tests/auto.rs",
        "tests/nested/main.rs",
        "examples/auto.rs",
        "examples/nested/main.rs",
        "benches/auto.rs",
        "benches/nested/main.rs",
        "shared/member.rs",
        "provider/src/lib.rs",
        "provider/build.rs",
    ];
    for target in targets {
        let path = project.path().join(target);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "PRIVATE_IMPLEMENTATION").unwrap();
    }
    fs::write(
        project.path().join("src/bin/nested/module.rs"),
        "PRIVATE_MODULE",
    )
    .unwrap();
    fs::write(project.path().join("unrelated.txt"), "PRIVATE_DATA").unwrap();
    fs::create_dir(project.path().join(".cargo")).unwrap();
    fs::write(
        project.path().join(".cargo/config.toml"),
        "[net]\noffline=false\n",
    )
    .unwrap();
    let mut settings = config();
    settings
        .acquisition
        .additional_sources
        .push(crate::config::PackageSource::CargoRegistry {
            name: "approved".into(),
            index_origin: "https://registry.example.invalid/index/".into(),
            download_origin: "https://registry.example.invalid/crates/".into(),
        });
    stage_acquisition_workspace(&settings, project.path(), output.path()).unwrap();
    assert_eq!(
        fs::read_to_string(output.path().join("Cargo.toml")).unwrap(),
        manifest
    );
    for target in targets {
        assert_eq!(
            fs::read(output.path().join(target)).unwrap(),
            b"",
            "{target}"
        );
    }
    for root in ["member", "provider"] {
        assert_eq!(
            fs::read(output.path().join(root).join("Cargo.toml")).unwrap(),
            fs::read(project.path().join(root).join("Cargo.toml")).unwrap()
        );
    }
    for absent in [".cargo", "unrelated.txt", "src/bin/nested/module.rs"] {
        assert!(!output.path().join(absent).exists(), "{absent}");
    }
}

#[test]
fn native_snapshot_selected_targets_retain_type_and_link_causes() {
    for (target, declaration, kind) in [
        ("src/lib.rs", "", "symbolic link"),
        (
            "custom/target.rs",
            "[lib]\npath='custom/target.rs'\n",
            "Unix socket",
        ),
        ("src/main.rs", "", "directory"),
        (
            "custom/target.rs",
            "[lib]\npath='custom/target.rs'\n",
            "2 hard links",
        ),
    ] {
        let project = fixture();
        let output = destination();
        let path = project.path().join(target);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        if !declaration.is_empty() {
            let manifest = fs::read_to_string(project.path().join("Cargo.toml")).unwrap();
            fs::write(
                project.path().join("Cargo.toml"),
                format!("{manifest}{declaration}"),
            )
            .unwrap();
        }
        let _listener = match kind {
            "symbolic link" => {
                std::os::unix::fs::symlink("missing", &path).unwrap();
                None
            }
            "Unix socket" => Some(std::os::unix::net::UnixListener::bind(&path).unwrap()),
            "directory" => {
                fs::create_dir(&path).unwrap();
                None
            }
            _ => {
                fs::write(&path, "PRIVATE").unwrap();
                fs::hard_link(&path, project.path().join("alias")).unwrap();
                None
            }
        };
        let error = stage_acquisition_workspace(&config(), project.path(), output.path())
            .unwrap_err()
            .to_string();
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert!(error.contains(kind), "{error}");
    }
}

#[test]
fn native_snapshot_root_lock_uses_configured_native_budget() {
    let project = fixture();
    let output = destination();
    let lock = format!("version=4\n#{}\n", "x".repeat((1 << 20) + 1));
    fs::write(project.path().join("Cargo.lock"), &lock).unwrap();
    fs::create_dir(project.path().join(".kvist")).unwrap();
    let policy = project.path().join(".kvist/vendor.toml");
    fs::write(&policy, "[limits]\nmax_lockfile_bytes=2097152\n").unwrap();
    stage_acquisition_workspace(&config(), project.path(), output.path()).unwrap();
    assert_eq!(
        fs::read_to_string(output.path().join("Cargo.lock")).unwrap(),
        lock
    );
    let admitted = destination();
    stage_native_admission_workspace(&config(), project.path(), admitted.path()).unwrap();
    assert_eq!(
        fs::read_to_string(admitted.path().join("Cargo.lock")).unwrap(),
        lock
    );
    fs::write(policy, "[limits]\nmax_lockfile_bytes=1048576\n").unwrap();
    let error = stage_acquisition_workspace(&config(), project.path(), destination().path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("Cargo.lock"), "{error}");
    assert!(error.contains("1048576"), "{error}");
}

#[test]
fn native_snapshot_disabled_auto_targets_and_explicit_path_guards() {
    let project = fixture();
    let manifest = "[package]\nname='fixture'\nversion='0.1.0'\n\
        autolib=false\nautobins=false\nautotests=false\nautoexamples=false\nautobenches=false\nbuild=false\n\
        [[bin]]\nname='chosen'\n";
    fs::write(project.path().join("Cargo.toml"), manifest).unwrap();
    fs::create_dir_all(project.path().join("src/bin")).unwrap();
    fs::write(project.path().join("src/bin/chosen.rs"), "PRIVATE_TARGET").unwrap();
    fs::write(project.path().join("src/lib.rs"), "PRIVATE_UNSELECTED").unwrap();
    fs::write(
        project.path().join("src/bin/unselected.rs"),
        "PRIVATE_UNSELECTED",
    )
    .unwrap();
    std::os::unix::fs::symlink("missing", project.path().join("build.rs")).unwrap();
    std::os::unix::fs::symlink("missing", project.path().join("examples")).unwrap();
    let output = destination();
    stage_native_admission_workspace(&config(), project.path(), output.path()).unwrap();
    assert_eq!(
        fs::read(output.path().join("src/bin/chosen.rs")).unwrap(),
        b""
    );
    for absent in [
        "src/lib.rs",
        "src/bin/unselected.rs",
        "build.rs",
        "examples",
    ] {
        assert!(!output.path().join(absent).exists(), "{absent}");
    }
    for relative in [
        "../outside.rs",
        "/etc/passwd",
        "linked/target.rs",
        ".cargo/config.toml",
    ] {
        fs::write(
            project.path().join("Cargo.toml"),
            format!("[package]\nname='fixture'\nversion='0.1.0'\n[lib]\npath='{relative}'\n"),
        )
        .unwrap();
        if relative.starts_with("linked") {
            std::os::unix::fs::symlink(project.path().join("src"), project.path().join("linked"))
                .unwrap();
            fs::write(project.path().join("src/target.rs"), "PRIVATE_TARGET").unwrap();
        }
        let error =
            stage_native_admission_workspace(&config(), project.path(), destination().path())
                .unwrap_err()
                .to_string();
        assert!(
            error.contains(if relative.starts_with("linked") {
                "symbolic link"
            } else if relative.starts_with(".cargo") {
                "exclude Cargo configuration"
            } else if relative.starts_with('/') {
                "relative path"
            } else {
                "escapes"
            }),
            "{error}"
        );
    }
}

#[test]
fn native_admission_does_not_authorize_cached_git_sources_for_acquisition() {
    for (repository, selector) in [
        ("file:///private/cached/repository", "branch='cached'"),
        (
            "https://unapproved.example.invalid/private",
            "branch='cached'",
        ),
    ] {
        let project = fixture();
        let manifest = format!(
            "[package]\nname='fixture'\nversion='0.1.0'\n[dependencies]\n\
             external={{git='{repository}',{selector}}}\n"
        );
        let revision = "a".repeat(40);
        let lock = format!(
            "version=4\n[[package]]\nname='external'\nversion='0.1.0'\n\
             source='git+{repository}?branch=cached#{revision}'\n"
        );
        fs::write(project.path().join("Cargo.toml"), &manifest).unwrap();
        fs::write(project.path().join("Cargo.lock"), &lock).unwrap();
        let output = destination();
        let identity =
            stage_native_admission_workspace(&config(), project.path(), output.path()).unwrap();
        assert_eq!(identity, crate::vendoring::lockfile_digest(lock.as_bytes()));
        assert_eq!(
            fs::read_to_string(output.path().join("Cargo.toml")).unwrap(),
            manifest
        );
        let error = stage_acquisition_workspace(&config(), project.path(), destination().path())
            .unwrap_err()
            .to_string();
        assert!(error.contains("full immutable revision"), "{error}");
    }
}
