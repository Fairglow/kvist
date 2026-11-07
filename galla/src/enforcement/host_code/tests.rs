use super::*;
use crate::enforcement::source;
use crate::protocol::Purpose;
use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
static LIVE_IMAGE: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::current_dir().unwrap().join(format!(
            ".galla-host-code-fixture-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn writable(path: &Path) -> (Vec<Grant>, Vec<Option<OwnedFd>>) {
    (
        vec![Grant {
            source: path.to_str().unwrap().to_owned(),
            destination: "/workspace/file".into(),
            access: Access::ReadWrite,
            purpose: Purpose::Authoring,
            identity: "fixture".into(),
        }],
        vec![Some(source::pin(path).unwrap())],
    )
}

fn dependency_alias_is_rejected(executable: &Path, library: &str) {
    let mut guard = Guard::default();
    guard.installed(executable).unwrap();
    guard.dependencies(executable).unwrap();
    let library = Path::new("/usr/lib").join(library).canonicalize().unwrap();
    let (grants, sources) = writable(&library);
    let error = guard
        .check(&grants, &sources)
        .expect_err("host dependency was writable");
    assert!(error.contains("narrower writable scope"), "{error}");
    let (mut grants, sources) = writable(&library);
    grants[0].access = Access::ReadOnly;
    guard.check(&grants, &sources).unwrap();
}

#[test]
fn native_backend_libcap_dependency_is_rejected() {
    dependency_alias_is_rejected(Path::new("/usr/bin/bwrap"), "libcap.so.2");
}

#[test]
fn native_prlimit_libsmartcols_dependency_is_rejected() {
    dependency_alias_is_rejected(Path::new("/usr/bin/prlimit"), "libsmartcols.so.1");
}

#[test]
fn direct_file_hardlink_is_rejected_using_the_pinned_origin() {
    let fixture = Fixture::new();
    let enforcer = fixture.0.join("enforcer");
    let alias = fixture.0.join("alias");
    fs::write(&enforcer, b"owned host code").unwrap();
    fs::hard_link(&enforcer, &alias).unwrap();
    let mut guard = Guard::default();
    guard.installed(&enforcer).unwrap();
    let (grants, sources) = writable(&alias);
    fs::remove_file(&alias).unwrap();
    fs::write(&alias, b"legitimate later replacement").unwrap();
    assert!(
        guard.check(&grants, &sources).is_err(),
        "pinned hardlink alias escaped"
    );
    guard.check(&grants, &writable(&alias).1).unwrap();
}

fn live_image_remains_protected(replacement: bool) {
    let _serial_image_launch = LIVE_IMAGE.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    let image = fixture.0.join("image");
    fs::copy(std::env::current_exe().unwrap(), &image).unwrap();
    fs::set_permissions(&image, fs::Permissions::from_mode(0o700)).unwrap();
    fs::hard_link(&image, fixture.0.join("running-alias")).unwrap();
    let mut command = Command::new(&image);
    command
        .args([
            "--exact",
            "enforcement::host_code::tests::fixture_running_image_child",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("GALLA_IMAGE_FIXTURE", &fixture.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut child = loop {
        match command.spawn() {
            Ok(child) => break child,
            // Another parallel trace fork may briefly retain the just-closed copy FD.
            Err(error)
                if error.raw_os_error() == Some(libc::ETXTBSY) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("cannot launch owned fixture image: {error}"),
        }
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.0.join("ready").exists() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("fixture image did not become ready: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    fs::remove_file(&image).unwrap();
    if replacement {
        fs::write(&image, b"new installed revision").unwrap();
    }
    fs::write(fixture.0.join("continue"), b"").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "replacement={replacement}: {output:?}"
    );
}

#[test]
fn running_image_remains_readable_and_protected_after_live_replacement() {
    live_image_remains_protected(true);
}

#[test]
fn running_image_remains_readable_and_protected_after_live_unlink() {
    live_image_remains_protected(false);
}

#[test]
#[ignore = "launched only as an owned fixture image by the replacement test"]
fn fixture_running_image_child() {
    let root = PathBuf::from(std::env::var_os("GALLA_IMAGE_FIXTURE").unwrap());
    assert!(fs::read("/proc/self/exe").unwrap().starts_with(b"\x7fELF"));
    fs::write(root.join("ready"), b"").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.join("continue").exists() {
        assert!(Instant::now() < deadline, "fixture parent did not continue");
        std::thread::sleep(Duration::from_millis(5));
    }
    let image_bytes = fs::read("/proc/self/exe").unwrap();
    assert!(image_bytes.starts_with(b"\x7fELF"));
    let mut guard = Guard::default();
    guard.running_image().unwrap();
    let (grants, sources) = writable(&root.join("running-alias"));
    assert!(
        guard.check(&grants, &sources).is_err(),
        "live image inode was not protected"
    );
    let installed = root.join("image");
    if installed.exists() {
        let (grants, sources) = writable(&installed);
        assert!(
            guard.check(&grants, &sources).is_err(),
            "installed replacement was not protected"
        );
    }
}

#[test]
fn complete_guard_accepts_independent_writes_and_closes_code_fds_at_exec() {
    let fixture = Fixture::new();
    let ordinary = fixture.0.join("ordinary");
    fs::write(&ordinary, b"payload-owned").unwrap();
    let (grants, sources) = writable(&ordinary);
    let guard = protect(Path::new("/usr/bin/bwrap"), &grants, &sources).unwrap();
    let identities = guard
        .origins
        .iter()
        .filter_map(|origin| origin.identity)
        .map(|identity| format!("{}:{}", identity.device, identity.inode))
        .collect::<Vec<_>>()
        .join(",");
    for origin in &guard.origins {
        if let Some(fd) = &origin._fd {
            assert!(
                nix::fcntl::FdFlag::from_bits_truncate(fcntl(fd, FcntlArg::F_GETFD).unwrap())
                    .contains(nix::fcntl::FdFlag::FD_CLOEXEC)
            );
        }
    }
    let mut command = trace_fixture_command("fds", &fixture);
    command.env("GALLA_GUARD_IDENTITIES", identities);
    assert!(
        capture(&mut command, TRACE_DEADLINE)
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn loader_trace_requires_resolved_complete_entries_including_paths_with_spaces() {
    let fixture = Fixture::new();
    let library = fixture.0.join("lib with spaces.so");
    fs::write(&library, b"library fixture").unwrap();
    let trace = format!(
        "linux-vdso.so.1 (0x123)\nlibexample.so => {} (0xabc)\n{} (0x456)\n",
        library.display(),
        library.display()
    );
    assert_eq!(
        trace_paths(trace.as_bytes()).unwrap(),
        vec![library.canonicalize().unwrap()]
    );
    for malformed in [
        "libmissing.so => not found\n",
        "libunknown.so (0x123)\n",
        "native launcher version 1\n",
        "",
        "/unavailable/loader (0x123)\n",
    ] {
        assert!(trace_paths(malformed.as_bytes()).is_err(), "{malformed}");
    }
    assert!(trace_paths(&vec![b' '; MAX_METADATA + 1]).is_err());
    let lookup = fixture.0.join("lookup");
    fs::create_dir(&lookup).unwrap();
    let alias = lookup.join("libexample.so");
    std::os::unix::fs::symlink(&library, &alias).unwrap();
    let paths =
        trace_paths(format!("libexample.so => {} (0x123)\n", alias.display()).as_bytes()).unwrap();
    let mut guard = Guard::default();
    for path in paths {
        guard.installed(&path).unwrap();
    }
    let (grants, sources) = writable(&lookup);
    assert!(
        guard.check(&grants, &sources).is_err(),
        "loader lookup symlink directory remained writable"
    );
}

#[test]
fn outside_lookup_ancestor_alias_cannot_hide_a_writable_selector_parent() {
    for hidden_target_ancestor in [false, true] {
        let fixture = Fixture::new();
        let writable_root = fixture.0.join("writable");
        let trusted = fixture.0.join("trusted");
        let lookup = fixture.0.join("outside-lookup");
        fs::create_dir(&writable_root).unwrap();
        fs::create_dir(&trusted).unwrap();
        fs::create_dir(writable_root.join("independent")).unwrap();
        let library = trusted.join("libexample.so");
        fs::write(&library, b"trusted fixture dependency").unwrap();
        std::os::unix::fs::symlink(&trusted, writable_root.join("slot")).unwrap();
        if hidden_target_ancestor {
            let hidden = fixture.0.join("outside-hidden");
            std::os::unix::fs::symlink(&writable_root, &hidden).unwrap();
            std::os::unix::fs::symlink(hidden.join("slot"), &lookup).unwrap();
        } else {
            std::os::unix::fs::symlink(&writable_root, &lookup).unwrap();
        }
        let known = if hidden_target_ancestor {
            lookup.join("libexample.so")
        } else {
            lookup.join("slot/libexample.so")
        };
        assert!(!known.starts_with(&writable_root));
        assert_eq!(known.canonicalize().unwrap(), library);
        assert!(!library.starts_with(&writable_root));
        let paths =
            trace_paths(format!("libexample.so => {} (0x123)\n", known.display()).as_bytes())
                .unwrap();
        let mut guard = Guard::default();
        for path in paths {
            guard.installed(&path).unwrap();
        }
        let (grants, sources) = writable(&writable_root);
        assert!(
            guard.check(&grants, &sources).is_err(),
            "outside lookup/target concealed a writable ancestor selector (hidden target ancestor: {hidden_target_ancestor})"
        );
        let (mut grants, sources) = writable(&writable_root);
        grants[0].access = Access::ReadOnly;
        guard.check(&grants, &sources).unwrap();
        let (grants, sources) = writable(&writable_root.join("independent"));
        guard.check(&grants, &sources).unwrap();
        fs::write(&library, b"legitimate operator update").unwrap();
        guard.check(&grants, &sources).unwrap();
        let upgraded = fixture.0.join("upgraded");
        fs::create_dir(&upgraded).unwrap();
        fs::write(upgraded.join("libexample.so"), b"new operator dependency").unwrap();
        fs::remove_file(writable_root.join("slot")).unwrap();
        std::os::unix::fs::symlink(&upgraded, writable_root.join("slot")).unwrap();
        let mut refreshed = Guard::default();
        refreshed.installed(&known).unwrap();
        refreshed.check(&grants, &sources).unwrap();
        let (grants, sources) = writable(&writable_root);
        assert!(refreshed.check(&grants, &sources).is_err());
    }
}

#[test]
fn elf_static_images_are_distinguished_from_invalid_or_oversized_headers() {
    let fixture = Fixture::new();
    let image = fixture.0.join("static-image");
    let mut bytes = vec![0; 120];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
    bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
    bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
    fs::write(&image, &bytes).unwrap();
    assert!(
        elf_interpreter(File::open(&image).unwrap())
            .unwrap()
            .is_none()
    );
    Guard::default().dependencies(&image).unwrap();
    bytes[56..58].copy_from_slice(&u16::MAX.to_le_bytes());
    fs::write(&image, &bytes).unwrap();
    assert!(elf_interpreter(File::open(&image).unwrap()).is_err());
    fs::write(&image, b"not an ELF launcher").unwrap();
    assert!(Guard::default().dependencies(&image).is_err());
}

#[test]
fn deleted_mapping_inode_and_escaped_path_are_protected_without_old_file() {
    let fixture = Fixture::new();
    let image = fixture.0.join("image with\nnewline");
    let alias = fixture.0.join("mapped-alias");
    fs::write(&image, b"owned mapped code").unwrap();
    fs::hard_link(&image, &alias).unwrap();
    let stat = fs::metadata(&image).unwrap();
    fs::remove_file(&image).unwrap();
    let path = image.to_str().unwrap().replace('\n', "\\012");
    let line = format!(
        "0000-1000 r-xp 00000000 {:02x}:{:02x} {}   {path} (deleted)",
        nix::sys::stat::major(stat.dev()),
        nix::sys::stat::minor(stat.dev()),
        stat.ino()
    );
    let mut guard = Guard::default();
    guard.mapping(line.as_bytes()).unwrap();
    let (grants, sources) = writable(&alias);
    assert!(guard.check(&grants, &sources).is_err());
}

fn trace_fixture_command(mode: &str, fixture: &Fixture) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "enforcement::host_code::tests::fixture_trace_child",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("GALLA_TRACE_FIXTURE", &fixture.0)
        .env("GALLA_TRACE_MODE", mode);
    command
}

#[test]
fn trace_supervision_bounds_both_streams_and_cleans_up_deadline_processes() {
    for mode in ["stdout", "stderr", "deadline"] {
        let fixture = Fixture::new();
        let mut command = trace_fixture_command(mode, &fixture);
        let start = Instant::now();
        let error = capture(&mut command, Duration::from_millis(400))
            .err()
            .expect("oversized or stalled trace was accepted");
        assert!(start.elapsed() < Duration::from_secs(3), "{error}");
        if mode == "deadline" {
            assert!(error.contains("deadline"), "{error}");
        } else {
            assert!(error.contains("budget"), "{error}");
        }
        let pid: i32 = fs::read_to_string(fixture.0.join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            nix::sys::signal::kill(Pid::from_raw(pid), None),
            Err(nix::errno::Errno::ESRCH)
        );
        if mode == "deadline" {
            let descendant: i32 = fs::read_to_string(fixture.0.join("descendant-pid"))
                .unwrap()
                .parse()
                .unwrap();
            if let Ok(stat) = fs::read_to_string(format!("/proc/{descendant}/stat")) {
                assert!(
                    stat.split_once(") ").unwrap().1.starts_with('Z'),
                    "trace descendant remained running: {stat}"
                );
            }
        }
    }
}

#[test]
#[ignore = "launched only by bounded native trace supervision tests"]
fn fixture_trace_child() {
    let root = PathBuf::from(std::env::var_os("GALLA_TRACE_FIXTURE").unwrap());
    fs::write(root.join("pid"), std::process::id().to_string()).unwrap();
    match std::env::var("GALLA_TRACE_MODE").unwrap().as_str() {
        "stdout" => std::io::stdout()
            .write_all(&vec![b'x'; MAX_METADATA + 1])
            .unwrap(),
        "stderr" => std::io::stderr()
            .write_all(&vec![b'x'; MAX_TRACE_ERRORS + 1])
            .unwrap(),
        "deadline" => {
            let mut descendant = Command::new("/usr/bin/sleep")
                .arg("30")
                .env_clear()
                .spawn()
                .unwrap();
            fs::write(root.join("descendant-pid"), descendant.id().to_string()).unwrap();
            descendant.wait().unwrap();
        }
        "fds" => {
            let identities = std::env::var("GALLA_GUARD_IDENTITIES").unwrap();
            for entry in fs::read_dir("/proc/self/fd").unwrap() {
                if let Ok(metadata) = fs::metadata(entry.unwrap().path()) {
                    let identity = format!("{}:{}", metadata.dev(), metadata.ino());
                    assert!(
                        !identities.split(',').any(|expected| expected == identity),
                        "host-code capability survived exec"
                    );
                }
            }
        }
        _ => panic!("unknown owned trace fixture mode"),
    }
}
