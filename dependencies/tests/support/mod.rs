use kvist_dependencies::{Error, ProcessFailure, rust::cache};
use sav::{
    CancellationToken, CommandSpec, OutputStream, SupervisionPolicy, run_supervised_observed,
};
use std::{fs, path::Path, time::Duration};

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
            "/usr/bin/cargo".into(),
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
