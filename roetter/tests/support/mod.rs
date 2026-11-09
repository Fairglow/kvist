use roetter::{Error, ProcessFailure, rust::cache};
use sav::{
    CancellationToken, CommandSpec, OutputStream, SupervisionPolicy, run_supervised_observed,
};
use std::{env, fs, path::Path, time::Duration};

/// The absolute path of the cargo binary the test process should drive.
///
/// Cargo sets `CARGO` to the active toolchain's cargo while running tests, so
/// this is the portable handle that works on a local dev box (Debian,
/// `/usr/bin/cargo`), a GitHub runner (Ubuntu, `~/.cargo/bin/cargo`), and the
/// Galla sandbox (which inherits the host toolchain). When `CARGO` is a bare
/// program name it is resolved against `PATH`; a missing or unresolvable value
/// falls back to the conventional `/usr/bin/cargo`.
pub fn cargo_path() -> String {
    let raw = env::var_os("CARGO").map(|value| value.to_string_lossy().into_owned());
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return "/usr/bin/cargo".to_owned();
    };
    if Path::new(&raw).is_absolute() {
        return raw;
    }
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            if dir.is_absolute() {
                let candidate = dir.join(&raw);
                if candidate.is_file() {
                    return candidate.to_string_lossy().into_owned();
                }
            }
        }
    }
    "/usr/bin/cargo".to_owned()
}

pub fn assert_actual_missing_metadata(project: &Path, home: &Path) {
    for root in [
        "registry/index",
        "registry/cache",
        "registry/src",
        "git/db",
        "git/checkouts",
    ] {
        fs::create_dir_all(home.join(root)).unwrap();
    }
    let token = CancellationToken::new();
    let command = CommandSpec::new(
        "/usr/bin/env",
        vec![
            format!("CARGO_HOME={}", home.display()),
            cargo_path(),
            "metadata".into(),
            "--offline".into(),
            "--locked".into(),
            "--format-version".into(),
            "1".into(),
            "--all-features".into(),
        ],
    )
    .in_directory(project);
    let policy = SupervisionPolicy {
        idle_timeout: Duration::from_secs(20),
        attempt_timeout: Some(Duration::from_secs(20)),
        detect_loops: false,
        max_retries: 0,
        max_output_bytes: 64 * 1024,
    };
    let mut stderr = Vec::new();
    let result = run_supervised_observed(&policy, &command, &token, &mut |stream, bytes| {
        if stream == OutputStream::Stderr {
            stderr.extend_from_slice(bytes);
        }
        Ok(())
    });
    let status = match result {
        Err(sav::Error::ProcessFailed { status }) => status,
        other => panic!("expected actual offline missing-source exit: {other:?}"),
    };
    let classified = cache::classify_metadata_failure(
        project,
        ProcessFailure::Exit,
        status.code(),
        &stderr,
        &token,
    );
    assert!(
        matches!(classified, Error::VendoringIncomplete { .. }),
        "actual bounded fixture diagnostic was not classified: {classified}; {}",
        String::from_utf8_lossy(&stderr)
    );
}
