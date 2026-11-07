//! Operator configuration authority stays distinct from shared mechanics.

use std::fs;
use std::process::{Command, Stdio};

#[test]
fn both_operator_frontends_honor_the_same_vendor_policy() {
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("Cargo.lock"), "version=4\n").unwrap();
    fs::write(project.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    fs::create_dir(project.path().join(".kvist")).unwrap();
    fs::write(
        project.path().join(".kvist/vendor.toml"),
        "[limits]\nmax_entries=0\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .arg("vendor")
        .arg(project.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "Kvist ignored the shared invalid policy: {output:?}"
    );
    assert!(!project.path().join(".kvist/vendored").exists());
}

#[test]
fn changed_component_queues_conform_to_the_native_schema() {
    for (component, queue) in [
        (
            "dependencies",
            include_str!("../../dependencies/TODOS.yaml"),
        ),
        ("maerg", include_str!("../TODOS.yaml")),
        ("sav", include_str!("../../sav/TODOS.yaml")),
        ("skott", include_str!("../../skott/TODOS.yaml")),
    ] {
        kvist::task_queue::parse(queue).unwrap_or_else(|error| panic!("{component}: {error}"));
    }
}

#[test]
fn invalid_project_configuration_never_silently_selects_host_acquisition() {
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("Cargo.lock"), "version=4\n").unwrap();
    fs::write(project.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    fs::write(project.path().join("kvist.toml"), "invalid = [").unwrap();
    for args in [vec!["vendor"], vec!["--json", "vendor"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
            .args(args)
            .arg(project.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
        assert!(!project.path().join(".kvist").exists());
    }
}

#[test]
fn operator_custom_vendor_destination_preserves_relative_and_absolute_selection() {
    for relative in [false, true] {
        let project = tempfile::tempdir().unwrap();
        fs::write(project.path().join("Cargo.lock"), "version=4\n").unwrap();
        fs::write(project.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        let target = project.path().join("offline-registry");
        let argument = if relative {
            std::path::Path::new("offline-registry")
        } else {
            &target
        };
        let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
            .args(["vendor", "--vendored-dir"])
            .arg(argument)
            .arg(project.path())
            .current_dir(project.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(target.is_dir(), "{output:?}");
        assert!(!project.path().join(".kvist/vendored").exists());
    }
}

#[test]
fn rust_enforcement_rejects_interrupted_same_lock_publication() {
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("Cargo.lock"), "version=4\n").unwrap();
    fs::write(project.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    kvist::vendor_command::vendor_project(project.path(), Default::default()).unwrap();
    fs::create_dir(project.path().join(".kvist/vendor-publication")).unwrap();
    assert!(
        kvist::language_vendoring::enforce_vendoring(project.path(), false, None).is_err(),
        "interrupted publication was reported ready for Rust execution"
    );
}
