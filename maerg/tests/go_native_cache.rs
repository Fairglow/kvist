//! Go native-cache tests isolate environment selection in child processes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use kvist::language_vendoring::{detect_language_strategy, enforce_vendoring};
use kvist::vendor_command::{VendorOptions, vendor_project};

fn project() -> tempfile::TempDir {
    let project = tempfile::Builder::new()
        .prefix(".go-native-test-")
        .tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("owned Go fixture");
    fs::write(
        project.path().join("go.mod"),
        "module example.test/nativecache\n\ngo 1.23\n",
    )
    .expect("go.mod");
    fs::write(project.path().join("go.sum"), "").expect("go.sum");
    fs::create_dir(project.path().join("module-cache")).expect("native module cache");
    project
}

fn child(project: &Path, mode: &str, cache: Option<&Path>) {
    if !Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("skip: native Go unavailable");
        return;
    }
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", "go_native_child", "--nocapture"])
        .env("KVIST_GO_NATIVE_TEST", mode)
        .env("KVIST_GO_NATIVE_PROJECT", project)
        .env("GOPATH", project.join("gopath"))
        .env("GOCACHE", project.join("compiler-cache"))
        .env("GOENV", "off")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .env("GOTOOLCHAIN", "local");
    match cache {
        Some(cache) => {
            command.env("GOMODCACHE", cache);
        }
        None => {
            command.env_remove("GOMODCACHE");
        }
    }
    if mode == "drift" {
        command.env(
            "PATH",
            format!(
                "{}:{}",
                project.join("tool-bin").display(),
                std::env::var("PATH").expect("PATH")
            ),
        );
    }
    let output = command.output().expect("isolated Go child");
    assert!(
        output.status.success(),
        "Go child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn go_native_operator_reuses_explicit_cache_without_vendor_sources() {
    let project = project();
    child(
        project.path(),
        "prepare",
        Some(&project.path().join("module-cache")),
    );
}

#[test]
fn go_native_cache_selection_falls_back_only_when_not_explicit() {
    let project = project();
    child(project.path(), "query", None);
    child(project.path(), "invalid", Some(Path::new("relative-cache")));
    child(project.path(), "invalid", Some(Path::new("/")));
    child(project.path(), "invalid", Some(project.path()));
}

#[test]
fn go_native_missing_cache_selection_rejects_traversal_and_resolves_aliases() {
    use std::os::unix::fs::symlink;

    let project = project();
    let traversal = project.path().join("absent/../..");
    child(project.path(), "invalid", Some(&traversal));
    assert!(!project.path().join("absent").exists());

    let alias = project.path().join("cache-parent-alias");
    symlink(project.path().join("module-cache"), &alias).expect("owned ancestor alias");
    child(project.path(), "alias", Some(&alias.join("missing-cache")));
}

#[test]
fn go_native_identity_includes_manifest_and_reports_missing_material() {
    let project = project();
    child(
        project.path(),
        "identity",
        Some(&project.path().join("module-cache")),
    );
    fs::write(
        project.path().join("go.mod"),
        "module example.test/nativecache\n\ngo 1.23\n\nrequire example.test/missing v1.0.0\n",
    )
    .expect("missing declared module");
    child(
        project.path(),
        "missing",
        Some(&project.path().join("module-cache")),
    );
}

#[test]
fn go_native_acquisition_rejects_lock_drift_without_cleanup() {
    use std::os::unix::fs::PermissionsExt;

    let project = project();
    let tools = project.path().join("tool-bin");
    fs::create_dir(&tools).expect("owned tool fixture");
    let go = tools.join("go");
    fs::write(
        &go,
        "#!/bin/sh\nprintf 'changed locked input\\n' >> go.sum\n",
    )
    .expect("drift fixture");
    fs::set_permissions(&go, fs::Permissions::from_mode(0o700)).expect("executable fixture");
    fs::write(
        project.path().join("module-cache/retained-download"),
        "useful cache",
    )
    .expect("cache sentinel");
    child(
        project.path(),
        "drift",
        Some(&project.path().join("module-cache")),
    );
}

#[test]
fn go_native_real_prepared_cache_when_explicit_fixture_provided() {
    let Ok(root) = std::env::var("KVIST_GO_NATIVE_FIXTURE") else {
        eprintln!("skip: no explicit real native Go fixture supplied");
        return;
    };
    let cache = std::env::var("KVIST_GO_NATIVE_CACHE").expect("explicit fixture cache");
    child(Path::new(&root), "real", Some(Path::new(&cache)));
}

#[test]
fn go_native_child() {
    let Ok(mode) = std::env::var("KVIST_GO_NATIVE_TEST") else {
        return;
    };
    let project = PathBuf::from(std::env::var_os("KVIST_GO_NATIVE_PROJECT").expect("root"));
    let strategy = detect_language_strategy(&project).expect("Go strategy");
    match mode.as_str() {
        "prepare" | "query" => {
            let expected = std::env::var_os("GOMODCACHE")
                .map(PathBuf::from)
                .unwrap_or_else(|| project.join("gopath/pkg/mod"));
            let before = fs::read(project.join("go.mod")).expect("original manifest");
            vendor_project(&project, VendorOptions::default()).expect("native preparation");
            vendor_project(&project, VendorOptions::default()).expect("repeat native preparation");
            let enforcement = enforce_vendoring(&project, false, None).expect("native enforcement");
            assert_eq!(enforcement.language, "go");
            assert_eq!(enforcement.vendored_dir, expected);
            assert_eq!(enforcement.mounts.len(), 1);
            assert_eq!(enforcement.mounts[0].source, expected);
            assert_eq!(enforcement.mounts[0].destination, "/workspace/go-modules");
            assert_eq!(enforcement.mounts[0].identity, enforcement.lockfile_digest);
            assert_eq!(fs::read(project.join("go.mod")).expect("manifest"), before);
            assert_eq!(fs::read(project.join("go.sum")).expect("sum"), b"");
            assert!(!project.join("vendor").exists());
            assert!(!project.join(".kvist").exists());
        }
        "invalid" => {
            let error = strategy
                .enforce(&project)
                .expect_err("explicit relative cache is invalid");
            assert!(error.to_string().contains("GOMODCACHE"), "{error}");
        }
        "alias" => {
            let report = strategy.enforce(&project).expect("missing cache catalogue");
            assert_eq!(
                PathBuf::from(report.vendored_dir),
                project.join("module-cache/missing-cache")
            );
            assert!(!project.join("module-cache/missing-cache").exists());
        }
        "identity" => {
            let before = strategy.enforce(&project).expect("initial catalogue");
            fs::write(
                project.join("go.mod"),
                "module example.test/nativecache\n\ngo 1.23\n\n// legitimate host edit\n",
            )
            .expect("manifest edit");
            let after = strategy.enforce(&project).expect("updated catalogue");
            assert_ne!(before.lockfile_digest, after.lockfile_digest);
            assert!(strategy.mounts(&project, &before.lockfile_digest).is_err());
        }
        "missing" => {
            let report = strategy
                .enforce(&project)
                .expect("declared native catalogue");
            assert!(!report.ready());
            assert!(
                report
                    .missing_human_readable()
                    .contains("example.test/missing"),
                "{report:?}"
            );
            assert_eq!(report.present_dependencies, 0);
        }
        "drift" => {
            let error = vendor_project(&project, VendorOptions::default())
                .expect_err("acquisition must not change locked inputs");
            assert!(
                error.to_string().contains("go.mod/go.sum changed"),
                "{error}"
            );
            assert_eq!(
                fs::read_to_string(project.join("module-cache/retained-download"))
                    .expect("retained cache"),
                "useful cache"
            );
            assert!(
                fs::read_to_string(project.join("go.sum"))
                    .expect("changed input")
                    .contains("changed locked input")
            );
        }
        "real" => {
            let before = (
                fs::read(project.join("go.mod")).expect("manifest"),
                fs::read(project.join("go.sum")).expect("sums"),
            );
            vendor_project(&project, VendorOptions::default()).expect("native warm download");
            let report = strategy
                .enforce(&project)
                .expect("actual native module catalogue");
            assert!(report.ready(), "{report:?}");
            assert_eq!(report.present_dependencies, 1);
            assert_eq!(
                (
                    fs::read(project.join("go.mod")).expect("manifest"),
                    fs::read(project.join("go.sum")).expect("sums")
                ),
                before
            );
            assert!(!project.join("vendor").exists());
        }
        _ => panic!("unknown mode"),
    }
}
