use super::tests::config;
use super::*;

fn diagnostic_project() -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    fs::write(
        project.path().join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(project.path().join("Cargo.lock"), "version=4\n").unwrap();
    project
}

#[test]
fn diagnostic_special_sources_name_path_type_and_safe_action() {
    for kind in ["Unix socket", "FIFO"] {
        let project = diagnostic_project();
        let path = project.path().join("sav-control-session/control.sock");
        fs::create_dir(path.parent().unwrap()).unwrap();
        let _socket = if kind == "Unix socket" {
            Some(std::os::unix::net::UnixListener::bind(&path).unwrap())
        } else {
            nix::unistd::mkfifo(&path, nix::sys::stat::Mode::S_IRUSR).unwrap();
            None
        };
        let error = prepare(&config(), project.path(), project.path(), Instant::now())
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert!(error.contains(kind), "{error}");
        assert!(
            error.contains("outside the selected source tree"),
            "{error}"
        );
        assert!(!error.contains("is unavailable"), "{error}");
        assert!(fs::symlink_metadata(&path).is_ok());
    }
}

#[test]
fn diagnostic_metadata_identifies_type_size_links_and_encoding() {
    let project = diagnostic_project();
    let path = project.path().join("Cargo.toml");
    for (case, expected) in [
        ("directory", "directory"),
        ("oversized", "1048577 bytes"),
        ("hardlink", "2 hard links"),
        ("encoding", "invalid utf-8"),
    ] {
        fs::remove_file(&path).unwrap();
        if case == "directory" {
            fs::create_dir(&path).unwrap();
        } else {
            let bytes = match case {
                "oversized" => vec![b' '; (1 << 20) + 1],
                "encoding" => vec![0xff],
                _ => b"[package]\n".to_vec(),
            };
            fs::write(&path, bytes).unwrap();
            if case == "hardlink" {
                fs::hard_link(&path, project.path().join("manifest-alias")).unwrap();
            }
        }
        let error = manifest(&config(), project.path())
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert!(error.contains(expected), "{error}");
        if case == "directory" {
            fs::remove_dir(&path).unwrap();
            fs::write(&path, "").unwrap();
        }
    }
}

#[test]
fn diagnostic_lock_parse_preserves_native_cause_and_path() {
    let project = diagnostic_project();
    let path = project.path().join("Cargo.lock");
    let invalid = "version = [";
    fs::write(&path, invalid).unwrap();
    let cause = toml::from_str::<toml::Value>(invalid)
        .unwrap_err()
        .to_string();
    let destination = tempfile::tempdir().unwrap();
    let error = stage_acquisition_workspace(&config(), project.path(), destination.path())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains(path.to_str().unwrap()), "{error}");
    assert!(error.contains(&cause), "{error}");
}

#[test]
fn diagnostic_metadata_missing_and_changed_identify_path() {
    let project = diagnostic_project();
    let path = project.path().join("Cargo.toml");
    let expected = fingerprint(&fs::metadata(&path).unwrap());
    fs::write(&path, "changed manifest").unwrap();
    let error = read_metadata(&config(), &path, expected)
        .unwrap_err()
        .to_string();
    assert!(error.contains(path.to_str().unwrap()), "{error}");
    assert!(error.contains("changed"), "{error}");
    fs::remove_file(&path).unwrap();
    let cause = fs::symlink_metadata(&path).unwrap_err().to_string();
    let error = manifest(&config(), project.path()).unwrap_err().to_string();
    assert!(error.contains(path.to_str().unwrap()), "{error}");
    assert!(error.contains(&cause), "{error}");
}
