//! Tests that verify:
//! 1. The probe actually executes bwrap to test namespace creation
//! 2. Backend location is secure against symlink substitution

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn probe_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_galla"));
    cmd.arg("--kvist-sandbox-probe-v1").env_clear();
    cmd
}

fn inside_sandbox() -> bool {
    Path::new("/.galla-sandbox").exists()
}

fn run_probe() -> Result<serde_json::Value, String> {
    let output = probe_cmd()
        .output()
        .map_err(|e| format!("failed to run probe: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("failed to parse probe output: {e}"))
}

/// Verify that the probe actually executes bwrap to test namespace creation.
///
/// This test creates a fake bwrap that exits with failure, and verifies
/// that the probe reports unavailability. If the probe just checked
/// /proc/self/ns existence without executing bwrap, it would succeed.
#[test]
fn probe_executes_bwrap_to_test_namespaces() {
    if inside_sandbox() {
        eprintln!("skipping: already inside a Galla sandbox");
        return;
    }

    // Create a temp directory with a fake bwrap that fails
    let temp = tempfile::tempdir().expect("create temp dir");
    let fake_bwrap = temp.path().join("bwrap");

    // Write a fake bwrap script that exits with failure
    fs::write(&fake_bwrap, "#!/bin/sh\nexit 1\n").expect("write fake bwrap");
    fs::set_permissions(&fake_bwrap, std::fs::Permissions::from_mode(0o755))
        .expect("make fake bwrap executable");

    // Point the probe at the stub through the test-only backend override.
    // The secure locator rejects a /tmp backend, so PATH alone would never
    // select it; the override exists so the test can prove the probe
    // executes the backend rather than inferring capabilities from
    // /proc/self/ns.
    let output = probe_cmd()
        .env("GALLA_TEST_BACKEND", &fake_bwrap)
        .output()
        .expect("run probe with fake bwrap");

    // The probe should fail because the fake bwrap exits with failure
    // This proves the probe actually executes bwrap rather than just
    // checking /proc/self/ns existence
    assert!(
        !output.status.success(),
        "probe should fail when bwrap fails to create namespaces; \
         this proves the probe actually executes bwrap rather than just \
         checking /proc/self/ns existence. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Verify that the probe succeeds when bwrap works correctly.
#[test]
fn probe_succeeds_with_working_bwrap() {
    if inside_sandbox() {
        eprintln!("skipping: already inside a Galla sandbox");
        return;
    }

    let probe = run_probe().expect("probe should succeed with working bwrap");

    // Verify the probe reports the backend
    let backend_path = probe
        .get("backend")
        .and_then(|b| b.get("path"))
        .and_then(|p| p.as_str())
        .expect("probe should report backend path");

    assert!(
        !backend_path.is_empty(),
        "probe should report a non-empty backend path"
    );

    // Verify namespace capabilities are reported
    let namespaces = probe
        .get("capabilities")
        .and_then(|c| c.get("namespaces"))
        .expect("probe should report namespace capabilities");

    // At least one namespace should be confirmed
    let mount = namespaces
        .get("mount")
        .and_then(|m| m.as_bool())
        .unwrap_or(false);
    let network = namespaces
        .get("network")
        .and_then(|n| n.as_bool())
        .unwrap_or(false);
    let pid = namespaces
        .get("pid")
        .and_then(|p| p.as_bool())
        .unwrap_or(false);

    assert!(
        mount || network || pid,
        "probe should confirm at least one namespace capability"
    );
}

/// Verify that the backend location is secure against symlink substitution.
/// The probe should report the canonical path of the backend, not a symlink target.
#[test]
fn probe_backend_path_is_canonical_not_symlink() {
    if inside_sandbox() {
        eprintln!("skipping: already inside a Galla sandbox");
        return;
    }

    let probe = run_probe().expect("probe should succeed");

    let backend_path = probe
        .get("backend")
        .and_then(|b| b.get("path"))
        .and_then(|p| p.as_str())
        .expect("probe should report backend path");

    // The reported path should not be a symlink
    let reported = Path::new(backend_path);
    let metadata = reported
        .symlink_metadata()
        .expect("should be able to read backend metadata");
    assert!(
        !metadata.file_type().is_symlink(),
        "probe should report canonical backend path, not a symlink: {backend_path}"
    );

    // Verify by canonicalizing and comparing
    let canonical = reported
        .canonicalize()
        .expect("should be able to canonicalize backend path");
    assert_eq!(
        backend_path,
        canonical.to_string_lossy(),
        "reported backend path should match canonical path"
    );
}

/// Verify that the probe validates the backend digest matches the actual binary.
#[test]
fn probe_backend_digest_matches_binary() {
    if inside_sandbox() {
        eprintln!("skipping: already inside a Galla sandbox");
        return;
    }

    let probe = run_probe().expect("probe should succeed");

    let backend_path = probe
        .get("backend")
        .and_then(|b| b.get("path"))
        .and_then(|p| p.as_str())
        .expect("probe should report backend path");

    let backend_digest = probe
        .get("backend")
        .and_then(|b| b.get("digest"))
        .and_then(|d| d.as_str())
        .expect("probe should report backend digest");

    // Compute the actual digest of the binary
    use sha2::Digest;
    let mut file = fs::File::open(backend_path).expect("open backend binary");
    let mut hasher = sha2::Sha256::new();
    use std::io::Read;
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).expect("read backend binary");
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual_digest = format!("sha256:{}", hex::encode(hasher.finalize()));

    assert_eq!(
        backend_digest, actual_digest,
        "probe backend digest should match actual binary digest"
    );
}
