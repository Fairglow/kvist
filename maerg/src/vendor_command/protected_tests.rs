use super::*;
use crate::config::{SandboxConfig, VcsSelection};
use crate::sandbox::RunnerIdentity;
use std::{fs, os::unix::fs::PermissionsExt, process::Command, time::Duration};

struct Fixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    sandbox: SandboxConfig,
    runner: RunnerIdentity,
    policy: String,
}

#[test]
fn native_pnpm_reuses_project_store_without_copying_unrelated_global_sources() {
    let fixture = tempfile::tempdir().unwrap();
    let project = fixture.path().join("project");
    let tools = fixture.path().join("tools");
    let global = fixture.path().join("global-store");
    for path in [&project, &tools, &global] {
        fs::create_dir(path).unwrap();
    }
    let store = project.join(".kvist/vendored-js");
    fs::create_dir_all(&store).unwrap();
    fs::write(store.join("retained-cache"), b"existing locked material").unwrap();
    fs::write(global.join("unrelated-package"), b"must not copy").unwrap();
    let log = fixture.path().join("argv.jsonl");
    let executable = tools.join("pnpm");
    fs::write(
        &executable,
        r#"#!/usr/bin/python3
import json,os,pathlib,sys
with open(os.environ["KVIST_PNPM_ARGV"],"a") as output:
    output.write(json.dumps(sys.argv[1:])+"\n")
if sys.argv[1:]==["store","path"]:
    print(os.environ["KVIST_PNPM_GLOBAL"])
elif sys.argv[1]=="install":
    pathlib.Path("node_modules").mkdir(exist_ok=True)
else:
    sys.exit(9)
"#,
    )
    .unwrap();
    fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "vendor_command::protected_tests::native_pnpm_child",
            "--nocapture",
        ])
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .env("KVIST_PNPM_PROJECT", &project)
        .env("KVIST_PNPM_ARGV", &log)
        .env("KVIST_PNPM_GLOBAL", &global)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let calls: Vec<Vec<String>> = fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(calls.len(), 1, "no global-store lookup or second operation");
    assert_eq!(
        calls[0],
        [
            "install".into(),
            "--frozen-lockfile".into(),
            "--store-dir".into(),
            store.to_string_lossy().into_owned()
        ],
    );
    assert!(!store.join("unrelated-package").exists());
    assert_eq!(
        fs::read(store.join("retained-cache")).unwrap(),
        b"existing locked material"
    );
}

#[test]
fn native_pnpm_child() {
    if let Some(project) = std::env::var_os("KVIST_PNPM_PROJECT") {
        populate_pnpm(Path::new(&project)).unwrap();
    }
}

#[test]
fn native_uv_export_preserves_permission_denied_cause() {
    let fixture = tempfile::tempdir().unwrap();
    let tools = fixture.path().join("tools");
    fs::create_dir(&tools).unwrap();
    let uv = tools.join("uv");
    fs::write(&uv, b"not executable").unwrap();
    fs::set_permissions(uv, fs::Permissions::from_mode(0o644)).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "vendor_command::protected_tests::native_uv_export_child",
            "--nocapture",
        ])
        .env("PATH", tools)
        .env("KVIST_UV_PROJECT", fixture.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn native_uv_export_child() {
    if let Some(project) = std::env::var_os("KVIST_UV_PROJECT") {
        let project = Path::new(&project);
        let error = export_uv_lock(project, &project.join("requirements.txt")).unwrap_err();
        assert!(
            error
                .to_string()
                .to_lowercase()
                .contains("permission denied"),
            "{error}"
        );
    }
}

#[test]
fn native_python_distinguishes_optional_uv_absence_from_failed_installation() {
    for available in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let tools = fixture.path().join("tools");
        let interpreter = fixture.path().join(".kvist/venv/bin/python");
        fs::create_dir(&tools).unwrap();
        fs::create_dir_all(interpreter.parent().unwrap()).unwrap();
        fs::write(
            fixture.path().join("requirements.lock.txt"),
            b"fixture==1.0\n",
        )
        .unwrap();
        let fallback = fixture.path().join("standard-installer-ran");
        for (path, script) in [
            (tools.join("pip"), "#!/usr/bin/python3\n"),
            (
                interpreter,
                "#!/usr/bin/python3\nimport os,pathlib\npathlib.Path(os.environ['KVIST_PYTHON_FALLBACK']).write_text('installed')\n",
            ),
        ] {
            fs::write(&path, script).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        if available {
            let uv = tools.join("uv");
            fs::write(&uv, "#!/usr/bin/python3\nimport sys\nif sys.argv[1:]==['--version']: sys.exit(0)\nprint('uv-native-install-failure',file=sys.stderr)\nsys.exit(47)\n").unwrap();
            fs::set_permissions(uv, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "vendor_command::protected_tests::native_python_child",
                "--nocapture",
            ])
            .env("PATH", tools)
            .env("KVIST_PYTHON_PROJECT", fixture.path())
            .env("KVIST_PYTHON_AVAILABLE", available.to_string())
            .env("KVIST_PYTHON_FALLBACK", &fallback)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            fallback.exists(),
            !available,
            "failed uv install must not silently switch installers"
        );
    }
}

#[test]
fn native_python_child() {
    if let Some(project) = std::env::var_os("KVIST_PYTHON_PROJECT") {
        let result = populate_python(Path::new(&project));
        if std::env::var("KVIST_PYTHON_AVAILABLE").unwrap() == "true" {
            let error = result.unwrap_err().to_string();
            assert!(
                error.contains("47") && error.contains("uv-native-install-failure"),
                "{error}"
            );
        } else {
            result.unwrap();
        }
    }
}

#[test]
fn host_acquisition_errors_retain_native_cause_and_redact_before_bounding() {
    let project = tempfile::tempdir().unwrap();
    let error = run_host_tool(
        "/usr/bin/python3",
        &["-c", "import os,sys; print('missing locked archive '+os.environ['KVIST_PROBE_SECRET'], file=sys.stderr); sys.exit(7)"],
        project.path(),
        "prepare locked artifacts",
        &[("KVIST_PROBE_SECRET", "private-acquisition-value")],
    ).unwrap_err().to_string();
    assert!(error.contains("missing locked archive"), "{error}");
    assert!(error.contains("exit status: 7"), "{error}");
    assert!(error.contains("[REDACTED]"), "{error}");
    assert!(!error.contains("private-acquisition-value"), "{error}");
}

#[test]
fn host_acquisition_timeout_preserves_last_diagnostic_without_retry() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join(".kvist")).unwrap();
    fs::write(
        project.path().join(".kvist/vendor.toml"),
        "[limits]\ntimeout_secs=1\n",
    )
    .unwrap();
    let start = std::time::Instant::now();
    let error = run_host_tool(
        "/usr/bin/python3",
        &["-c", "import sys,time; print('waiting for package source', file=sys.stderr, flush=True); time.sleep(10)"],
        project.path(),
        "acquire missing locked artifacts",
        &[],
    ).unwrap_err().to_string();
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(error.contains("waiting for package source"), "{error}");
    assert!(
        error.contains("timeout") || error.contains("timed"),
        "{error}"
    );
    assert!(error.contains("retained"), "{error}");
}

#[test]
fn acquisition_failure_diagnostics_retain_status_and_captured_cause() {
    use std::os::unix::process::ExitStatusExt;
    let project = tempfile::tempdir().unwrap();
    let result = crate::sandbox::ExecutionResult {
        output: std::process::Output {
            status: std::process::ExitStatus::from_raw(101 << 8),
            stdout: b"unrelated stdout".to_vec(),
            stderr: b"error: failed to resolve locked dependency".to_vec(),
        },
        timed_out: false,
        output_limit_exceeded: false,
        cancelled: false,
    };
    let error = check_acquisition_result(
        project.path(),
        &result,
        &kvist_dependencies::VendorLimits::default(),
        &[],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("101"), "{error}");
    assert!(
        error.contains("failed to resolve locked dependency"),
        "{error}"
    );
    assert!(!error.contains("unrelated stdout"), "{error}");
    assert!(error.contains("no host fallback"), "{error}");
}

#[test]
fn acquisition_failure_diagnostics_name_each_limit_and_bound_redacted_output() {
    use std::os::unix::process::ExitStatusExt;
    let project = tempfile::tempdir().unwrap();
    let mut result = crate::sandbox::ExecutionResult {
        output: std::process::Output {
            status: std::process::ExitStatus::from_raw(9),
            stdout: "private-value ".repeat(1000).into_bytes(),
            stderr: Vec::new(),
        },
        timed_out: true,
        output_limit_exceeded: true,
        cancelled: true,
    };
    let limits = kvist_dependencies::VendorLimits::default();
    let error =
        check_acquisition_result(project.path(), &result, &limits, &["private-value".into()])
            .unwrap_err()
            .to_string();
    for expected in [
        "cancelled".into(),
        format!("{} seconds", limits.timeout_secs),
        format!("{} bytes", limits.max_output_bytes),
        "[REDACTED]".into(),
        "[truncated]".into(),
    ] {
        assert!(error.contains(&expected), "{error}");
    }
    assert!(!error.contains("private-value"), "{error}");
    assert!(
        error.len() < MAX_VENDOR_ERROR_BYTES + 1024,
        "{}",
        error.len()
    );
    result.timed_out = false;
    result.output_limit_exceeded = false;
    result.cancelled = false;
    result.output.stdout.clear();
    let error = check_acquisition_result(project.path(), &result, &limits, &[])
        .unwrap_err()
        .to_string();
    assert!(error.contains("no stdout/stderr"), "{error}");
    assert!(error.contains("signal"), "{error}");
    result.output.stdout = format!("{}\x1b[31m", "\u{03bb}".repeat(3000)).into_bytes();
    let error = check_acquisition_result(project.path(), &result, &limits, &[])
        .unwrap_err()
        .to_string();
    assert!(!error.contains('\x1b'), "{error}");
    assert!(error.contains("[truncated]"), "{error}");
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir_in(".").expect("project-local fixture");
        let project = root.path().join("project");
        fs::create_dir(&project).expect("project");
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .arg(&project)
                .status()
                .expect("git init")
                .success()
        );
        let runner_path = root.path().join("runner");
        fs::write(&runner_path, "#!/bin/sh\nexit 98\n").expect("runner");
        fs::set_permissions(&runner_path, fs::Permissions::from_mode(0o755)).expect("runner mode");
        let sandbox = SandboxConfig {
            runner: runner_path
                .canonicalize()
                .expect("runner path")
                .to_string_lossy()
                .into(),
            backend: "/usr/bin/true".into(),
            environment_allowlist: Vec::new(),
            acquisition: crate::config::AcquisitionConfig::default(),
        };
        let runner = crate::sandbox::runner_identity(&sandbox, &project, VcsSelection::Git)
            .expect("runner identity");
        Self {
            _root: root,
            project,
            sandbox,
            runner,
            policy: crate::vendoring::lockfile_digest(b"approved policy"),
        }
    }

    fn options<'a>(
        &'a self,
        cancellation: &'a sav::CancellationToken,
    ) -> ProtectedVendorOptions<'a> {
        ProtectedVendorOptions {
            sandbox: &self.sandbox,
            vcs_selection: VcsSelection::Git,
            expected_runner: &self.runner,
            policy_identity: &self.policy,
            cancellation,
            remaining_budget: Duration::from_secs(3),
        }
    }

    fn provision_empty_rust(&self) {
        fs::write(self.project.join("Cargo.lock"), "version = 4\n").expect("lock");
        vendor_project(&self.project, VendorOptions::default()).expect("explicit local provision");
    }
}

#[test]
fn protected_vendor_refuses_precancelled_and_exhausted_work_before_inspection() {
    let fixture = Fixture::new();
    let cancellation = sav::CancellationToken::new();
    cancellation.cancel();
    let error = vendor_project_protected(&fixture.project, fixture.options(&cancellation))
        .expect_err("cancelled");
    assert!(matches!(error, KvistError::Sav(sav::Error::Cancelled)));
    let error = check_protected_vendor_readiness(&fixture.project, fixture.options(&cancellation))
        .expect_err("cancelled readiness inspection");
    assert!(matches!(error, KvistError::Sav(sav::Error::Cancelled)));

    let cancellation = sav::CancellationToken::new();
    let mut options = fixture.options(&cancellation);
    options.remaining_budget = Duration::ZERO;
    let error = vendor_project_protected(&fixture.project, options).expect_err("exhausted");
    assert!(error.to_string().contains("budget"), "{error}");
    let mut options = fixture.options(&cancellation);
    options.remaining_budget = Duration::ZERO;
    let error = check_protected_vendor_readiness(&fixture.project, options)
        .expect_err("exhausted readiness");
    assert!(error.to_string().contains("budget"), "{error}");
    assert!(!fixture.project.join(".kvist").exists());
}

#[test]
fn protected_vendor_does_not_fulfill_unbound_requests_from_unrelated_ready_rust() {
    let fixture = Fixture::new();
    fixture.provision_empty_rust();
    let manifest_path = fixture.project.join(".kvist/cargo-cache.json");
    let before = fs::read(&manifest_path).expect("manifest");
    let cancellation = sav::CancellationToken::new();
    let error = vendor_project_protected(&fixture.project, fixture.options(&cancellation))
        .expect_err("readiness alone cannot fulfill a model dependency request");
    assert!(error.to_string().contains("not bound"), "{error}");
    assert_eq!(fs::read(&manifest_path).expect("manifest after"), before);
    assert!(!fixture.project.join(".kvist/acquisition").exists());
}

#[test]
fn protected_vendor_refuses_unbound_requests_without_filesystem_inspection() {
    let mut fixture = Fixture::new();
    fixture.sandbox.runner = "/nonexistent/protected-vendor-runner".into();
    let nonexistent_project = fixture.project.join("not-created");
    let cancellation = sav::CancellationToken::new();
    let error = vendor_project_protected(&nonexistent_project, fixture.options(&cancellation))
        .expect_err("an unbound request must be refused without resolving any paths");
    assert!(error.to_string().contains("not bound"), "{error}");
    assert!(!nonexistent_project.exists());
}

#[test]
fn protected_readiness_reuses_ready_rust_without_rewriting_state() {
    let fixture = Fixture::new();
    fixture.provision_empty_rust();
    let manifest_path = fixture.project.join(".kvist/cargo-cache.json");
    let manifest = fs::read(&manifest_path).expect("manifest");
    let cancellation = sav::CancellationToken::new();
    let report = check_protected_vendor_readiness(&fixture.project, fixture.options(&cancellation))
        .expect("ready Rust reuse");
    assert!(report.ready());
    assert_eq!(fs::read(&manifest_path).expect("manifest after"), manifest);
    assert!(!fixture.project.join(".kvist/acquisition").exists());
}

#[test]
fn protected_native_readiness_leaves_legacy_export_fences_untouched() {
    for name in ["vendor-operation.lock", "vendor-publication"] {
        let fixture = Fixture::new();
        fixture.provision_empty_rust();
        let path = fixture.project.join(".kvist").join(name);
        if name == "vendor-publication" {
            fs::create_dir(&path).unwrap();
        } else {
            fs::write(&path, "").unwrap();
        }
        let cancellation = sav::CancellationToken::new();
        assert!(
            check_protected_vendor_readiness(&fixture.project, fixture.options(&cancellation))
                .is_ok(),
            "legacy {name} must not block independently selected native sources"
        );
        assert!(path.exists(), "readiness must not reconcile the fence");
    }
}

#[test]
fn native_acquisition_fetches_a_locked_workspace_without_building_or_downloading() {
    let mut fixture = Fixture::new();
    let runner = std::env::var_os("KVIST_GALLA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("galla-runner")
        });
    assert!(runner.is_file(), "build the native Galla runner first");
    fixture.sandbox.runner = runner
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    fixture.sandbox.backend = "/usr/bin/bwrap".into();
    fs::create_dir_all(fixture.project.join("app/src")).unwrap();
    fs::create_dir_all(fixture.project.join("src/dependency/src")).unwrap();
    fs::write(
        fixture.project.join("Cargo.toml"),
        "[workspace]\nmembers=['app']\nresolver='3'\n",
    )
    .unwrap();
    fs::write(fixture.project.join("app/Cargo.toml"), "[package]\nname='acquisition-app'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nacquisition-dependency={path='../src/dependency'}\n").unwrap();
    fs::write(fixture.project.join("app/src/lib.rs"), "pub fn app() {}\n").unwrap();
    fs::write(
        fixture.project.join("app/build.rs"),
        "fn main(){panic!(\"acquisition must not build\");}\n",
    )
    .unwrap();
    fs::write(
        fixture.project.join("src/dependency/Cargo.toml"),
        "[package]\nname='acquisition-dependency'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        fixture.project.join("src/dependency/src/lib.rs"),
        "pub fn dependency() {}\n",
    )
    .unwrap();
    let cargo =
        crate::toolchain::resolve_pinned_toolchain(&fixture.project, "native acquisition fixture")
            .unwrap()
            .cargo;
    let output = Command::new(cargo)
        .args(["generate-lockfile", "--offline"])
        .current_dir(&fixture.project)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let lock = fs::read(fixture.project.join("Cargo.lock")).unwrap();
    let stage = fixture._root.path().join("staging");
    fs::create_dir(&stage).unwrap();
    let vendor = stage.join("vendor");
    fs::create_dir(&vendor).unwrap();
    provision_rust_via_acquisition_sandbox(&fixture.project, &fixture.sandbox, &vendor).unwrap();
    assert_eq!(fs::read(fixture.project.join("Cargo.lock")).unwrap(), lock);
    assert!(!fixture.project.join("target").exists());
    assert_eq!(fs::read_dir(&vendor).unwrap().count(), 0);
    assert_eq!(
        fs::read_dir(&stage).unwrap().count(),
        1,
        "acquisition scratch leaked"
    );
}

#[test]
fn protected_vendor_does_not_replace_the_approved_runner_identity() {
    let mut fixture = Fixture::new();
    fixture.provision_empty_rust();
    fixture.runner.digest = crate::vendoring::lockfile_digest(b"other approved runner");
    let cancellation = sav::CancellationToken::new();
    let error = check_protected_vendor_readiness(&fixture.project, fixture.options(&cancellation))
        .expect_err("identity substitution");
    assert!(error.to_string().contains("after approval"), "{error}");
}

#[test]
fn native_configured_metadata_admits_cached_git_without_building_or_host_fallback() {
    let mut fixture = Fixture::new();
    let runner = std::env::var_os("KVIST_GALLA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("galla-runner")
        });
    assert!(runner.is_file(), "build the native Galla runner first");
    fixture.sandbox.runner = runner
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    fixture.sandbox.backend = "/usr/bin/bwrap".into();
    let repo = fixture._root.path().join("git-dependency");
    let home = fixture._root.path().join("cargo-home");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::create_dir(&home).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname='cached-git-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n",
    )
    .unwrap();
    fs::write(repo.join("src/lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
    fs::write(
        repo.join("build.rs"),
        "fn main(){panic!(\"metadata must not build dependencies\");}\n",
    )
    .unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "Cargo.toml", "src/lib.rs", "build.rs"],
        vec![
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    let rev = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(rev.status.success(), "{rev:?}");
    let rev = String::from_utf8(rev.stdout).unwrap();
    let url = format!("file://{}", repo.canonicalize().unwrap().display());
    fs::create_dir(fixture.project.join("src")).unwrap();
    fs::write(fixture.project.join("src/lib.rs"), "pub fn app() {}\n").unwrap();
    fs::write(
        fixture.project.join("build.rs"),
        "fn main(){panic!(\"metadata must not build workspace\");}\n",
    )
    .unwrap();
    fs::write(fixture.project.join("Cargo.toml"), format!("[package]\nname='native-metadata-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n[dependencies]\ncached-git-fixture={{git='{url}',rev='{}'}}\n", rev.trim())).unwrap();
    let cargo = crate::toolchain::resolve_pinned_toolchain(&fixture.project, "cached Git fixture")
        .unwrap()
        .cargo;
    let output = Command::new(cargo)
        .arg("generate-lockfile")
        .env("CARGO_HOME", &home)
        .current_dir(&fixture.project)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let lock = fs::read(fixture.project.join("Cargo.lock")).unwrap();
    let cancellation = sav::CancellationToken::new();
    let view = native_cargo_admission(
        &fixture.project,
        &home,
        Some(&fixture.sandbox),
        &cancellation,
    )
    .unwrap();
    assert!(view.report.ready());
    assert_eq!(view.report.git_dependencies, 1);
    assert_eq!(fs::read(fixture.project.join("Cargo.lock")).unwrap(), lock);
    assert!(!fixture.project.join("target").exists());
    assert!(!fixture.project.join(".kvist/cargo-cache.json").exists());
    fixture.sandbox.runner = "/missing/native-metadata-runner".into();
    let error = native_cargo_admission(
        &fixture.project,
        &home,
        Some(&fixture.sandbox),
        &cancellation,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("no host fallback"), "{error}");
}

#[test]
#[ignore = "requires public crates.io access, installed Rust and Bubblewrap"]
fn native_configured_fetch_downloads_missing_crate_over_https() {
    let mut fixture = Fixture::new();
    let runner = std::env::var_os("KVIST_GALLA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("galla-runner")
        });
    fixture.sandbox.runner = runner
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    fixture.sandbox.backend = "/usr/bin/bwrap".into();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(workspace.join("Cargo.lock")).unwrap()).unwrap();
    let package = lock["package"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"].as_str() == Some("cfg-if"))
        .unwrap();
    let mut package = package.as_table().unwrap().clone();
    package.remove("dependencies");
    let version = package["version"].as_str().unwrap();
    fs::create_dir(fixture.project.join("src")).unwrap();
    fs::write(fixture.project.join("src/lib.rs"), "").unwrap();
    fs::write(fixture.project.join("Cargo.toml"), format!("[package]\nname='native-https-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n[dependencies]\ncfg-if='={version}'\n")).unwrap();
    fs::write(fixture.project.join("Cargo.lock"), format!("version=4\n[[package]]\nname='native-https-fixture'\nversion='0.1.0'\ndependencies=['cfg-if']\n[[package]]\n{}", toml::to_string(&package).unwrap())).unwrap();
    fs::create_dir(fixture.project.join(".kvist")).unwrap();
    fs::write(
        fixture.project.join(".kvist/vendor.toml"),
        "[limits]\ntimeout_secs=60\n",
    )
    .unwrap();
    let home = fixture._root.path().join("cold-cargo-home");
    fs::create_dir_all(home.join("registry")).unwrap();
    let cancellation = sav::CancellationToken::new();
    assert!(
        !kvist_dependencies::rust::cache::view(&fixture.project, &home, &cancellation)
            .unwrap()
            .report
            .ready()
    );
    provision_rust_via_acquisition_sandbox(&fixture.project, &fixture.sandbox, &home).unwrap();
    assert!(
        kvist_dependencies::rust::cache::view(&fixture.project, &home, &cancellation)
            .unwrap()
            .report
            .ready()
    );
    assert!(!fixture.project.join(".kvist/vendored").exists());
}

#[test]
fn protected_vendor_rejects_malformed_policy_identity() {
    let mut fixture = Fixture::new();
    fixture.provision_empty_rust();
    fixture.policy = "unapproved".into();
    let cancellation = sav::CancellationToken::new();
    let error = check_protected_vendor_readiness(&fixture.project, fixture.options(&cancellation))
        .expect_err("invalid policy");
    assert!(error.to_string().contains("policy"), "{error}");
}

#[test]
fn protected_vendor_never_launches_host_package_managers() {
    let fixture = Fixture::new();
    let tools = fixture._root.path().join("tools");
    fs::create_dir(&tools).expect("sentinel tools");
    for tool in [
        "cargo", "rustup", "go", "npm", "yarn", "pnpm", "uv", "python", "python3", "pip", "conan",
        "vcpkg",
    ] {
        let path = tools.join(tool);
        fs::write(
            &path,
            "#!/bin/sh\nprintf launched >> \"$KVIST_HOST_TOOL_MARKER\"\nexit 97\n",
        )
        .expect("sentinel");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("sentinel mode");
    }
    let marker = fixture._root.path().join("host-tool-launched");
    let home = fixture._root.path().join("cargo-home");
    fs::create_dir(&home).expect("isolated native home");
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "vendor_command::protected_tests::protected_vendor_refusal_child",
            "--nocapture",
        ])
        .env("KVIST_PROTECTED_VENDOR_PROJECT", &fixture.project)
        .env("KVIST_PROTECTED_VENDOR_RUNNER", &fixture.sandbox.runner)
        .env("KVIST_HOST_TOOL_MARKER", &marker)
        .env("CARGO_HOME", &home)
        .env(
            "PATH",
            format!(
                "{}:{}",
                tools.canonicalize().expect("tools path").display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .expect("isolated sentinel test");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists(), "protected refusal launched a host tool");
}

#[test]
fn protected_vendor_refusal_child() {
    let Some(project) = std::env::var_os("KVIST_PROTECTED_VENDOR_PROJECT") else {
        return;
    };
    let project = PathBuf::from(project);
    let sandbox = SandboxConfig {
        runner: std::env::var("KVIST_PROTECTED_VENDOR_RUNNER").expect("runner"),
        backend: "/usr/bin/true".into(),
        environment_allowlist: Vec::new(),
        acquisition: crate::config::AcquisitionConfig::default(),
    };
    let runner = crate::sandbox::runner_identity(&sandbox, &project, VcsSelection::Git)
        .expect("approved runner");
    let cancellation = sav::CancellationToken::new();
    let policy = crate::vendoring::lockfile_digest(b"approved policy");
    let options = || ProtectedVendorOptions {
        sandbox: &sandbox,
        vcs_selection: VcsSelection::Git,
        expected_runner: &runner,
        policy_identity: &policy,
        cancellation: &cancellation,
        remaining_budget: Duration::from_secs(3),
    };
    for (lockfile, contents) in [
        ("go.sum", ""),
        ("package-lock.json", "{}"),
        ("yarn.lock", ""),
        ("pnpm-lock.yaml", ""),
        ("uv.lock", ""),
        ("requirements.lock.txt", ""),
        ("conanfile.txt", ""),
        ("vcpkg.json", "{}"),
        ("Cargo.lock", "version = 4\n"),
    ] {
        fs::write(project.join(lockfile), contents).expect("language marker");
        let error = vendor_project_protected(&project, options())
            .expect_err("unmediated provisioning must be refused");
        assert!(
            error.to_string().contains("protected") && error.to_string().contains("host"),
            "{lockfile}: {error}"
        );
        if lockfile == "Cargo.lock" {
            assert!(
                check_protected_vendor_readiness(&project, options())
                    .expect("empty native catalogue is inspectable without acquisition")
                    .ready()
            );
        } else {
            let error = check_protected_vendor_readiness(&project, options())
                .expect_err("unmediated non-Rust readiness must be refused");
            assert!(
                error.to_string().contains("protected") && error.to_string().contains("host"),
                "{lockfile}: {error}"
            );
        }
        assert!(
            !project.join(".kvist").exists(),
            "refusal must not stage work"
        );
        fs::remove_file(project.join(lockfile)).expect("remove language marker");
    }

    fs::write(project.join("Cargo.lock"), "version = 4\n").expect("empty lock");
    vendor_project(&project, VendorOptions::default()).expect("provision without any downloads");
    let error = vendor_project_protected(&project, options())
        .expect_err("ready vendoring cannot fulfill an unbound model request");
    assert!(error.to_string().contains("not bound"), "{error}");
    assert!(
        check_protected_vendor_readiness(&project, options())
            .expect("readiness only")
            .ready()
    );
    let vendored = project.join(".kvist/vendored");
    let stale = vendored.join("stale");
    fs::create_dir_all(&stale).expect("stale export package");
    fs::write(
        stale.join("Cargo.toml"),
        "[package]\nname = \"stale\"\nversion = \"1.0.0\"\n",
    )
    .expect("stale manifest");
    assert!(
        check_protected_vendor_readiness(&project, options())
            .expect("native readiness ignores unrelated export contents")
            .ready()
    );
    assert!(
        stale.exists(),
        "native readiness must not remove unrelated export material"
    );
    fs::remove_file(stale.join("Cargo.toml")).expect("remove stale manifest");
    fs::remove_dir(&stale).expect("remove stale package");

    fs::write(
        project.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"missing\"\nversion = \"1.0.0\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
         checksum = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
    )
    .expect("changed lock");
    let error =
        check_protected_vendor_readiness(&project, options()).expect_err("missing native material");
    assert!(matches!(error, KvistError::VendoringIncomplete { .. }));
    assert!(!project.join(".kvist/acquisition").exists());
}
