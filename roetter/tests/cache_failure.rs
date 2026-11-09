use roetter::{Error, ProcessFailure, rust::cache};
use sav::CancellationToken;
use std::{fs, path::Path};

const REV: &str = "0123456789012345678901234567890123456789";
const URL: &str = "https://example.invalid/repo";

fn project(git: bool) -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix(".native-failure-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    let source = if git {
        format!("git+{URL}#{REV}")
    } else {
        "registry+sparse+https://other.example/index/".into()
    };
    let checksum = if git {
        String::new()
    } else {
        format!("checksum='{}'\n", "a".repeat(64))
    };
    fs::write(
        dir.path().join("Cargo.lock"),
        format!(
            "version=4\n[[package]]\nname='demo'\nversion='1.0.0'\nsource='{source}'\n{checksum}"
        ),
    )
    .unwrap();
    fs::create_dir(dir.path().join("cargo-home")).unwrap();
    dir
}

fn classify(project: &Path, failure: ProcessFailure, code: Option<i32>, stderr: &[u8]) -> Error {
    cache::classify_metadata_failure(project, failure, code, stderr, &CancellationToken::new())
}

#[test]
fn only_observed_cargo_exit_and_locked_offline_missing_registry_chains_allow_acquisition() {
    let dir = project(false);
    for stderr in [
        "error: no matching package named `demo` found\nlocation searched: `fixture` index\nAs a reminder, you're using offline mode (--offline)\n",
        "error: failed to download `demo v1.0.0`\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\n",
    ] {
        assert!(matches!(
            classify(
                dir.path(),
                ProcessFailure::Exit,
                Some(101),
                stderr.as_bytes()
            ),
            Error::VendoringIncomplete { .. }
        ));
        for (failure, code) in [
            (ProcessFailure::Timeout, None),
            (ProcessFailure::Io, None),
            (ProcessFailure::OutputLimit, None),
            (ProcessFailure::Exit, Some(2)),
            (ProcessFailure::Exit, None),
        ] {
            assert!(matches!(
                classify(dir.path(), failure, code, stderr.as_bytes()),
                Error::NativeCargo { .. }
            ));
        }
    }
}

#[test]
fn warnings_unknown_dependencies_lock_errors_and_malformed_diagnostics_are_not_missing() {
    let dir = project(false);
    for stderr in [
        "warning: no matching package named `demo` found\nerror: malformed Cargo.toml\n",
        "error: no matching package named `unlocked` found\nAs a reminder, you're using offline mode (--offline)\n",
        "error: failed to download `demo v9.0.0`\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\n",
        "error: the lock file needs to be updated but --locked was passed\n",
        "error: no matching package named `demo` found\nerror: credential provider failed\n",
        "error: failed to parse manifest\nCaused by:\n  no matching package named `demo` found\n",
    ] {
        let error = classify(
            dir.path(),
            ProcessFailure::Exit,
            Some(101),
            stderr.as_bytes(),
        );
        assert!(matches!(
            error,
            Error::NativeCargo {
                exit_code: Some(101),
                ..
            }
        ));
    }
    assert!(matches!(
        classify(dir.path(), ProcessFailure::Exit, Some(101), &[0xff]),
        Error::NativeCargo { .. }
    ));
    let mut oversized = vec![b' '; 65536];
    oversized.extend_from_slice(b"error: no matching package named `demo` found\n");
    assert!(matches!(
        classify(dir.path(), ProcessFailure::Exit, Some(101), &oversized),
        Error::NativeCargo { .. }
    ));
}

#[test]
fn git_missing_checkout_or_revision_must_match_locked_source_and_offline_chain() {
    let dir = project(true);
    let checkout = format!(
        "error: failed to get `demo` as a dependency of package `fixture`\nCaused by:\n  Unable to update {URL}#{REV}\nCaused by:\n  can't checkout from '{URL}': you are in the offline mode (--offline)\n"
    );
    let revision = format!(
        "error: failed to get `demo` as a dependency of package `fixture`\nCaused by:\n  Unable to update {URL}#{REV}\nCaused by:\n  failed to lookup reference in preexisting repository, and can't check for updates in offline mode (--offline)\nCaused by:\n  revspec '{REV}' not found; class=Reference (4); code=NotFound (-3)\n"
    );
    for stderr in [checkout, revision] {
        assert!(matches!(
            classify(
                dir.path(),
                ProcessFailure::Exit,
                Some(101),
                stderr.as_bytes()
            ),
            Error::VendoringIncomplete { .. }
        ));
        let unrelated = stderr.replace(URL, "https://unrelated.invalid/repo");
        assert!(matches!(
            classify(
                dir.path(),
                ProcessFailure::Exit,
                Some(101),
                unrelated.as_bytes()
            ),
            Error::NativeCargo { .. }
        ));
    }
    let unknown = format!(
        "error: failed to get `demo` as a dependency of package `fixture`\nCaused by:\n  Unable to update {URL}#{REV}\nCaused by:\n  failed to lookup reference in preexisting repository\n"
    );
    assert!(matches!(
        classify(
            dir.path(),
            ProcessFailure::Exit,
            Some(101),
            unknown.as_bytes()
        ),
        Error::NativeCargo { .. }
    ));
}

#[test]
fn cold_noncanonical_native_directories_acquire_before_metadata_without_speculation() {
    for git in [false, true] {
        let dir = project(git);
        let home = dir.path().join("cargo-home");
        let calls = std::cell::Cell::new(0);
        let error = cache::provision_with_validation(dir.path(), &home, &CancellationToken::new(), false,
            |selected| { calls.set(calls.get() + 1); assert!(selected.join("registry").is_dir()); assert!(selected.join("git").is_dir()); Err(Error::Cancelled) },
            |_| panic!("absent required native data is a proof of missing, not an unknown metadata failure")).unwrap_err();
        assert!(matches!(error, Error::Cancelled));
        assert_eq!(calls.get(), 1);
    }
}

#[test]
fn only_request_bound_private_home_metadata_aliases_are_normalized() {
    let dir = project(false);
    let private = Path::new("/workspace/scratch/cargo-home");
    let bytes = serde_json::to_vec(&serde_json::json!({"packages":[{"name":"demo","version":"1.0.0",
        "source":"registry+sparse+https://other.example/index/",
        "manifest_path":"/workspace/scratch/cargo-home/registry/src/native-opaque/demo-1.0.0/Cargo.toml"}]})).unwrap();
    let normalized =
        cache::normalize_metadata_paths(dir.path(), private, &bytes, &CancellationToken::new())
            .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&normalized).unwrap();
    assert_eq!(
        value["packages"][0]["manifest_path"],
        "/workspace/cargo-registry/src/native-opaque/demo-1.0.0/Cargo.toml"
    );
    assert!(
        cache::normalize_metadata_paths(
            dir.path(),
            Path::new("/workspace/other/cargo-home"),
            &bytes,
            &CancellationToken::new()
        )
        .is_err()
    );
    let traversal = String::from_utf8(bytes)
        .unwrap()
        .replace("native-opaque", "../unapproved");
    assert!(
        cache::normalize_metadata_paths(
            dir.path(),
            private,
            traversal.as_bytes(),
            &CancellationToken::new()
        )
        .is_err()
    );
}
