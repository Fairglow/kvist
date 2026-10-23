use super::*;
use crate::config::{SandboxConfig, VcsSelection};
use crate::sandbox::RunnerIdentity;
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

struct Fixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    sandbox: SandboxConfig,
    runner: RunnerIdentity,
    policy: String,
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
    let manifest_path = VendorManifest::path_in(&fixture.project);
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
    let manifest_path = VendorManifest::path_in(&fixture.project);
    let manifest = fs::read(&manifest_path).expect("manifest");
    let cancellation = sav::CancellationToken::new();
    let report = check_protected_vendor_readiness(&fixture.project, fixture.options(&cancellation))
        .expect("ready Rust reuse");
    assert!(report.ready());
    assert_eq!(fs::read(&manifest_path).expect("manifest after"), manifest);
    assert!(!fixture.project.join(".kvist/acquisition").exists());
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
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "vendor_command::protected_tests::protected_vendor_refusal_child",
            "--nocapture",
        ])
        .env("KVIST_PROTECTED_VENDOR_PROJECT", &fixture.project)
        .env("KVIST_PROTECTED_VENDOR_RUNNER", &fixture.sandbox.runner)
        .env("KVIST_HOST_TOOL_MARKER", &marker)
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
        let error = check_protected_vendor_readiness(&project, options())
            .expect_err("unmediated readiness must be refused");
        assert!(
            error.to_string().contains("protected") && error.to_string().contains("host"),
            "{lockfile}: {error}"
        );
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
    fs::create_dir(&stale).expect("stale package");
    fs::write(
        stale.join("Cargo.toml"),
        "[package]\nname = \"stale\"\nversion = \"1.0.0\"\n",
    )
    .expect("stale manifest");
    let error = check_protected_vendor_readiness(&project, options()).expect_err("stale registry");
    assert!(error.to_string().contains("vendor repack"), "{error}");
    assert!(
        stale.exists(),
        "protected refusal must not remove stale material"
    );
    fs::remove_file(stale.join("Cargo.toml")).expect("remove stale manifest");
    fs::remove_dir(&stale).expect("remove stale package");

    fs::write(
        project.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"missing\"\nversion = \"1.0.0\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    )
    .expect("changed lock");
    let error = check_protected_vendor_readiness(&project, options()).expect_err("lock drift");
    assert!(matches!(error, KvistError::VendoringStale { .. }));

    let mut manifest = VendorManifest::load(&project)
        .expect("load manifest")
        .expect("manifest");
    manifest.lockfile_digest = VendorManifest::current_digest(&project).expect("current digest");
    manifest.save(&project).expect("record matching lock");
    let error =
        check_protected_vendor_readiness(&project, options()).expect_err("missing material");
    assert!(matches!(error, KvistError::VendoringIncomplete { .. }));
    assert!(!project.join(".kvist/acquisition").exists());
}
