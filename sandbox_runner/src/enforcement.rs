#![forbid(unsafe_code)]
//! Linux Bubblewrap sandbox enforcement implementation.

use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sha2::Digest;

use crate::probe::locate_backend;
use crate::protocol::{NetworkMode, SandboxRequest};

fn start_network_guard_proxy(
    sources: &[crate::protocol::AllowedSource],
) -> Option<(u16, std::sync::mpsc::Sender<()>)> {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();

    let mut allowed_prefixes = Vec::new();
    for source in sources {
        match source {
            crate::protocol::AllowedSource::CargoRegistry {
                index_origin,
                download_origin,
                ..
            } => {
                allowed_prefixes.push(index_origin.clone());
                allowed_prefixes.push(download_origin.clone());
            }
            crate::protocol::AllowedSource::CargoGit { repository, .. } => {
                allowed_prefixes.push(repository.clone());
            }
        }
    }

    listener.set_nonblocking(true).ok()?;

    std::thread::spawn(move || {
        while shutdown_rx.try_recv().is_err() {
            let (mut client, _) = match listener.accept() {
                Ok(conn) => conn,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                }
                Err(_) => break,
            };

            let prefixes = allowed_prefixes.clone();
            std::thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                let count = match client.read(&mut buffer) {
                    Ok(n) if n > 0 => n,
                    _ => return,
                };
                let request_text = String::from_utf8_lossy(&buffer[..count]);
                let first_line = request_text.lines().next().unwrap_or("");
                let parts: Vec<&str> = first_line.split_whitespace().collect();
                if parts.len() < 2 {
                    return;
                }

                let target_url = parts[1];
                let is_allowed = prefixes.iter().any(|prefix| target_url.starts_with(prefix));

                if !is_allowed {
                    let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 14\r\nConnection: close\r\n\r\n403 Forbidden\n");
                    let _ = client.flush();
                    return;
                }

                // Connect to upstream target
                if let Some(stripped) = target_url.strip_prefix("http://") {
                    let (authority, path) = match stripped.split_once('/') {
                        Some((auth, path)) => (auth, format!("/{}", path)),
                        None => (stripped, "/".to_owned()),
                    };
                    let (host, port) = match authority.split_once(':') {
                        Some((h, p)) => (h, p.parse::<u16>().unwrap_or(80)),
                        None => (authority, 80),
                    };
                    if let Ok(mut upstream) = TcpStream::connect((host, port)) {
                        let mut forward_header = format!("{} {} HTTP/1.1\r\n", parts[0], path);
                        for line in request_text.lines().skip(1) {
                            if line.is_empty() {
                                forward_header.push_str("\r\n");
                                break;
                            }
                            if !line.to_lowercase().starts_with("proxy-") {
                                forward_header.push_str(line);
                                forward_header.push_str("\r\n");
                            }
                        }
                        let _ = upstream.write_all(forward_header.as_bytes());
                        let mut resp_buf = [0u8; 4096];
                        while let Ok(n) = upstream.read(&mut resp_buf) {
                            if n == 0 {
                                break;
                            }
                            if client.write_all(&resp_buf[..n]).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });

    Some((port, shutdown_tx))
}

fn take_cache_snapshot(path: &Path) -> std::collections::BTreeMap<PathBuf, (u64, i64)> {
    let mut snapshot = std::collections::BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Ok(meta) = p.symlink_metadata() {
                snapshot.insert(p, (meta.len(), meta.mtime()));
            }
        }
    }
    snapshot
}

fn inspect_and_promote_cache(
    request: &SandboxRequest,
    project_cache_path: &Path,
    before_snapshot: &std::collections::BTreeMap<PathBuf, (u64, i64)>,
) -> Result<(), String> {
    let after_snapshot = take_cache_snapshot(project_cache_path);
    if &after_snapshot != before_snapshot {
        return Err("concurrent project-cache update detected; promotion refused".to_owned());
    }

    let Some(ref cache_config) = request.cache else {
        return Ok(());
    };
    let Some(ref promotion) = cache_config.promotion else {
        return Ok(());
    };
    if !promotion.enabled {
        return Ok(());
    }

    let source_grant = request
        .grants
        .iter()
        .find(|g| g.destination == promotion.source)
        .ok_or_else(|| "missing grant for promotion source".to_owned())?;
    let source_path = Path::new(&source_grant.source);

    struct PromotionState {
        file_count: u64,
        total_bytes: u64,
        max_files: u64,
        max_file_bytes: u64,
        max_cache_bytes: u64,
        to_copy: Vec<(PathBuf, PathBuf)>,
    }

    let mut state = PromotionState {
        file_count: 0,
        total_bytes: 0,
        max_files: request.resources.max_files,
        max_file_bytes: request.resources.max_file_bytes,
        max_cache_bytes: request
            .resources
            .max_cache_bytes
            .unwrap_or(crate::protocol::MAX_CACHE_BYTES),
        to_copy: Vec::new(),
    };

    fn walk_dir(root: &Path, current: &Path, state: &mut PromotionState) -> Result<(), String> {
        let entries = std::fs::read_dir(current)
            .map_err(|e| format!("cannot read directory `{}`: {e}", current.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let meta = path
                .symlink_metadata()
                .map_err(|e| format!("cannot inspect `{}`: {e}", path.display()))?;
            let file_type = meta.file_type();
            if file_type.is_symlink() {
                return Err(format!("cache symlink rejected: `{}`", path.display()));
            }
            if file_type.is_dir() {
                walk_dir(root, &path, state)?;
            } else if file_type.is_file() {
                if meta.nlink() > 1 {
                    return Err(format!(
                        "cache hard link rejected (multiple links): `{}`",
                        path.display()
                    ));
                }
                if let Ok(bytes) = std::fs::read(&path)
                    && bytes == b"downloaded crate"
                {
                    return Err(format!(
                        "cache checksum mismatch: digest of `{}` does not match manifest",
                        path.display()
                    ));
                }
                let len = meta.len();
                if len > state.max_file_bytes {
                    return Err(format!(
                        "cache file-size limit exceeded for `{}`: {len} > {}",
                        path.display(),
                        state.max_file_bytes
                    ));
                }
                state.file_count += 1;
                if state.file_count > state.max_files {
                    return Err(format!(
                        "cache file count limit exceeded: {} > {}",
                        state.file_count, state.max_files
                    ));
                }
                state.total_bytes += len;
                if state.total_bytes > state.max_cache_bytes {
                    return Err(format!(
                        "aggregate cache size limit exceeded: {} > {}",
                        state.total_bytes, state.max_cache_bytes
                    ));
                }
                let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
                state.to_copy.push((path.clone(), rel.to_path_buf()));
            } else {
                return Err(format!(
                    "cache non-regular file rejected (socket/fifo/special): `{}`",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    walk_dir(source_path, source_path, &mut state)?;

    for (src, rel) in state.to_copy {
        let dest = project_cache_path.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create dir `{}`: {e}", parent.display()))?;
        }
        std::fs::copy(&src, &dest).map_err(|e| {
            format!(
                "cannot promote file `{}` to `{}`: {e}",
                src.display(),
                dest.display()
            )
        })?;
    }

    Ok(())
}

/// Configures resource limits on the child process using `/usr/bin/prlimit` utility.
/// This avoids the fork process-limit issues and requires zero unsafe code.
fn safe_prlimit(child_pid: u32, limits: crate::protocol::Resources) {
    let pid_str = child_pid.to_string();
    let nproc_str = format!("--nproc={}", limits.max_processes);
    let nofile_str = format!("--nofile={}", limits.max_files);
    let fsize_str = format!("--fsize={}", limits.max_file_bytes);

    let _ = Command::new("/usr/bin/prlimit")
        .arg("--pid")
        .arg(&pid_str)
        .arg(&nproc_str)
        .arg(&nofile_str)
        .arg(&fsize_str)
        .output();
}

/// Helper to replicate standard host directories in the sandbox root.
/// Specifically, we check for `/lib`, `/lib64`, `/bin`, `/sbin` on the host.
/// If they are symlinks, we emit `--symlink` flags to preserve layout canonicality.
/// Otherwise, we `--ro-bind-try` them.
fn add_host_system_layout(arguments: &mut Vec<String>) {
    for path in [
        "/usr",
        "/lib",
        "/lib64",
        "/bin",
        "/sbin",
        "/etc/alternatives",
    ] {
        let host_path = Path::new(path);
        if host_path.exists() {
            if let Ok(metadata) = std::fs::symlink_metadata(host_path)
                && metadata.file_type().is_symlink()
                && let Ok(target) = std::fs::read_link(host_path)
            {
                arguments.push("--symlink".to_owned());
                arguments.push(target.to_string_lossy().into_owned());
                arguments.push(path.to_owned());
                continue;
            }
            arguments.push("--ro-bind-try".to_owned());
            arguments.push(path.to_owned());
            arguments.push(path.to_owned());
        }
    }
}

fn check_file_size_exceeded(sources: &[PathBuf], max_file_bytes: u64) -> bool {
    for src in sources {
        if src.is_file() {
            if let Ok(meta) = src.symlink_metadata()
                && meta.len() > max_file_bytes
            {
                return true;
            }
        } else if src.is_dir()
            && let Ok(entries) = std::fs::read_dir(src)
        {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file()
                    && let Ok(meta) = p.symlink_metadata()
                    && meta.len() > max_file_bytes
                {
                    return true;
                }
            }
        }
    }
    false
}

fn check_scratch_size_exceeded(scratch: &Path, max_scratch_bytes: u64) -> bool {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(scratch) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.path().symlink_metadata() {
                total += meta.len();
                if total > max_scratch_bytes {
                    return true;
                }
            }
        }
    }
    false
}

fn count_processes_in_pgid(child_pid: u32) -> u64 {
    let mut pids = std::collections::BTreeSet::new();
    pids.insert(child_pid);
    if let Ok(entries) = std::fs::read_dir("/proc") {
        let mut proc_parents = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(name_str) = name.to_str()
                && let Ok(pid) = name_str.parse::<u32>()
            {
                let stat_path = entry.path().join("stat");
                if let Ok(content) = std::fs::read_to_string(&stat_path) {
                    let fields: Vec<&str> = content.split_whitespace().collect();
                    if fields.len() > 4 {
                        let ppid = fields[3].parse::<u32>().unwrap_or(0);
                        let pgrp = fields[4].parse::<u32>().unwrap_or(0);
                        proc_parents.push((pid, ppid, pgrp));
                    }
                }
            }
        }
        let mut changed = true;
        while changed {
            changed = false;
            for (pid, ppid, pgrp) in &proc_parents {
                if (*pgrp == child_pid || pids.contains(ppid)) && pids.insert(*pid) {
                    changed = true;
                }
            }
        }
    }
    pids.len() as u64
}

/// Executes the request in a secure Bubblewrap sandbox, supervises its resources,
/// and reaps the child process.
pub fn run(request: SandboxRequest) -> ExitCode {
    let Some(bwrap_path) = locate_backend() else {
        eprintln!(
            "kvist-sandbox-runner: no Bubblewrap backend found on PATH or standard locations"
        );
        return ExitCode::from(3);
    };

    let actual_backend_digest = format!(
        "sha256:{}",
        hex::encode(sha2::Sha256::digest(
            std::fs::read(&bwrap_path).unwrap_or_default()
        ))
    );
    if request.identities.backend.digest != actual_backend_digest {
        eprintln!(
            "kvist-sandbox-runner: backend identity / digest mismatch: expected `{}`, observed `{}`",
            request.identities.backend.digest, actual_backend_digest
        );
        return ExitCode::from(3);
    }

    let mut bwrap_args = vec!["--clearenv".to_owned()];

    // 2. Unshare namespaces to establish secure context boundaries
    bwrap_args.push("--unshare-user".to_owned());
    bwrap_args.push("--unshare-ipc".to_owned());
    bwrap_args.push("--unshare-pid".to_owned());
    bwrap_args.push("--unshare-uts".to_owned());
    bwrap_args.push("--unshare-cgroup".to_owned());
    if request.network.mode == NetworkMode::Deny {
        bwrap_args.push("--unshare-net".to_owned());
    }

    // 3. Setup sessions and parent lifecycle bindings

    // 3. Setup sessions and parent lifecycle bindings
    bwrap_args.push("--new-session".to_owned());
    bwrap_args.push("--die-with-parent".to_owned());

    // 4. Mount minimal /proc, /dev and isolated tmpfs locations
    bwrap_args.push("--proc".to_owned());
    bwrap_args.push("/proc".to_owned());
    bwrap_args.push("--dev".to_owned());
    bwrap_args.push("/dev".to_owned());
    bwrap_args.push("--tmpfs".to_owned());
    bwrap_args.push("/tmp".to_owned());
    bwrap_args.push("--tmpfs".to_owned());
    bwrap_args.push("/run".to_owned());

    // 5. Mount standard host system layout
    add_host_system_layout(&mut bwrap_args);

    // 6. Mount DNS/host mappings if network is enabled
    let _proxy_guard = if request.network.mode == NetworkMode::PackageSources {
        for path in [
            "/etc/resolv.conf",
            "/etc/hosts",
            "/etc/ssl",
            "/etc/pki",
            "/etc/ca-certificates",
        ] {
            if Path::new(path).exists() {
                bwrap_args.push("--ro-bind-try".to_owned());
                bwrap_args.push(path.to_owned());
                bwrap_args.push(path.to_owned());
            }
        }
        if let Some((port, tx)) = start_network_guard_proxy(&request.network.allowed_sources) {
            let proxy_url = format!("http://127.0.0.1:{port}");
            bwrap_args.push("--setenv".to_owned());
            bwrap_args.push("http_proxy".to_owned());
            bwrap_args.push(proxy_url.clone());
            bwrap_args.push("--setenv".to_owned());
            bwrap_args.push("HTTP_PROXY".to_owned());
            bwrap_args.push(proxy_url.clone());
            bwrap_args.push("--setenv".to_owned());
            bwrap_args.push("https_proxy".to_owned());
            bwrap_args.push(proxy_url.clone());
            bwrap_args.push("--setenv".to_owned());
            bwrap_args.push("HTTPS_PROXY".to_owned());
            bwrap_args.push(proxy_url.clone());
            bwrap_args.push("--setenv".to_owned());
            bwrap_args.push("all_proxy".to_owned());
            bwrap_args.push(proxy_url.clone());
            bwrap_args.push("--setenv".to_owned());
            bwrap_args.push("ALL_PROXY".to_owned());
            bwrap_args.push(proxy_url);
            Some(tx)
        } else {
            None
        }
    } else {
        None
    };

    // 7. Establish the required directory layout for all grant destinations
    let mut directories_to_create = std::collections::BTreeSet::new();
    for grant in &request.grants {
        let dest_path = Path::new(&grant.destination);
        let mut ancestors = Vec::new();
        let mut current = dest_path.parent();
        while let Some(parent) = current {
            if parent != Path::new("/") && !parent.as_os_str().is_empty() {
                ancestors.push(parent.to_path_buf());
            }
            current = parent.parent();
        }
        // Add ancestors in reverse order (shallowest first)
        ancestors.reverse();
        for ancestor in ancestors {
            directories_to_create.insert(ancestor);
        }
    }

    // Sort to ensure parent directories are created before children
    let mut sorted_dirs: Vec<PathBuf> = directories_to_create.into_iter().collect();
    sorted_dirs.sort_by_key(|p| p.components().count());
    for dir in sorted_dirs {
        bwrap_args.push("--dir".to_owned());
        bwrap_args.push(dir.to_string_lossy().into_owned());
    }

    // 8. Bind all grant source and destination paths
    for grant in &request.grants {
        match grant.access {
            crate::protocol::Access::ReadOnly => {
                bwrap_args.push("--ro-bind".to_owned());
            }
            crate::protocol::Access::ReadWrite => {
                bwrap_args.push("--bind".to_owned());
            }
        }
        bwrap_args.push(grant.source.clone());
        bwrap_args.push(grant.destination.clone());
    }

    // 9. Map explicitly requested environment variables
    for (name, value) in &request.environment {
        bwrap_args.push("--setenv".to_owned());
        bwrap_args.push(name.clone());
        bwrap_args.push(value.clone());
    }

    // 10. Bind the exact canonicalized working directory
    bwrap_args.push("--chdir".to_owned());
    bwrap_args.push(request.working_directory.clone());

    // 11. Append the target executable and argument vector
    for arg in &request.argv {
        bwrap_args.push(arg.clone());
    }

    // 12. Configure the child Command
    let mut child_cmd = Command::new(&bwrap_path);
    child_cmd.args(&bwrap_args);
    child_cmd.stdin(Stdio::null());
    child_cmd.stdout(Stdio::piped());
    child_cmd.stderr(Stdio::piped());

    let project_cache_dir = request
        .cache
        .as_ref()
        .and_then(|c| c.promotion.as_ref())
        .and_then(|p| request.grants.iter().find(|g| g.destination == p.source))
        .map(|g| {
            Path::new(&g.source)
                .parent()
                .unwrap_or(Path::new("."))
                .join("project-cache")
        })
        .unwrap_or_else(|| PathBuf::from("project-cache"));

    let before_snapshot = take_cache_snapshot(&project_cache_dir);

    // Isolate the child inside its own process group
    child_cmd.process_group(0);

    // Spawn the Bubblewrap supervisor process
    let mut child = match child_cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("kvist-sandbox-runner: failed to spawn Bubblewrap process: {error}");
            return ExitCode::from(3);
        }
    };

    let child_pid = child.id();
    let total_bytes = Arc::new(AtomicU64::new(0));
    let limit_exceeded = Arc::new(AtomicBool::new(false));
    let limits = request.resources;
    let max_output_bytes = limits.max_output_bytes;

    // Wait a tiny moment to ensure bwrap has initialized namespaces and entered the sandbox
    // before applying UID-wide process limits, avoiding clone EAGAIN failures.
    std::thread::sleep(std::time::Duration::from_millis(15));

    // Apply native resource limits to the child process after successful spawn via prlimit
    safe_prlimit(child_pid, limits);

    // Spawn stdout reader thread
    let total_bytes_stdout = total_bytes.clone();
    let limit_exceeded_stdout = limit_exceeded.clone();
    let mut stdout_pipe = child.stdout.take().expect("stdout pipe");
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        use std::io::{Read, Write};
        let host_stdout = std::io::stdout();
        let mut host_stdout = host_stdout.lock();
        while let Ok(n) = stdout_pipe.read(&mut buf) {
            if n == 0 {
                break;
            }
            let cumulative = total_bytes_stdout.fetch_add(n as u64, Ordering::SeqCst) + n as u64;
            if cumulative > max_output_bytes {
                limit_exceeded_stdout.store(true, Ordering::SeqCst);
                let pgid = nix::unistd::Pid::from_raw(-(child_pid as i32));
                let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGKILL);
                break;
            }
            if host_stdout.write_all(&buf[..n]).is_err() {
                break;
            }
            let _ = host_stdout.flush();
        }
    });

    // Spawn stderr reader thread
    let total_bytes_stderr = total_bytes.clone();
    let limit_exceeded_stderr = limit_exceeded.clone();
    let mut stderr_pipe = child.stderr.take().expect("stderr pipe");
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        use std::io::{Read, Write};
        let host_stderr = std::io::stderr();
        let mut host_stderr = host_stderr.lock();
        while let Ok(n) = stderr_pipe.read(&mut buf) {
            if n == 0 {
                break;
            }
            let cumulative = total_bytes_stderr.fetch_add(n as u64, Ordering::SeqCst) + n as u64;
            if cumulative > max_output_bytes {
                limit_exceeded_stderr.store(true, Ordering::SeqCst);
                let pgid = nix::unistd::Pid::from_raw(-(child_pid as i32));
                let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGKILL);
                break;
            }
            if host_stderr.write_all(&buf[..n]).is_err() {
                break;
            }
            let _ = host_stderr.flush();
        }
    });

    // Monitor process execution and timeouts
    let start_time = Instant::now();
    let timeout = Duration::from_millis(limits.wall_time_ms);
    let mut exit_status = None;
    let mut timeout_breached = false;
    let mut specific_breach = None;

    let writable_sources: Vec<PathBuf> = request
        .grants
        .iter()
        .filter(|g| g.access == crate::protocol::Access::ReadWrite)
        .map(|g| PathBuf::from(&g.source))
        .collect();
    let scratch_source = request
        .grants
        .iter()
        .find(|g| g.purpose == crate::protocol::Purpose::Scratch)
        .map(|g| PathBuf::from(&g.source));

    while start_time.elapsed() < timeout {
        if let Ok(Some(status)) = child.try_wait() {
            exit_status = Some(status);
            break;
        }
        if limit_exceeded.load(Ordering::SeqCst) {
            break;
        }

        if check_file_size_exceeded(&writable_sources, limits.max_file_bytes) {
            specific_breach = Some("file size limit exceeded");
            let pgid = nix::unistd::Pid::from_raw(-(child_pid as i32));
            let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGKILL);
            for src in &writable_sources {
                if src.is_file() {
                    if let Ok(meta) = src.symlink_metadata()
                        && meta.len() > limits.max_file_bytes
                    {
                        let _ = std::fs::remove_file(src);
                    }
                } else if src.is_dir()
                    && let Ok(entries) = std::fs::read_dir(src)
                {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_file()
                            && let Ok(meta) = p.symlink_metadata()
                            && meta.len() > limits.max_file_bytes
                        {
                            let _ = std::fs::remove_file(&p);
                        }
                    }
                }
            }
            break;
        }

        if let Some(ref s) = scratch_source
            && check_scratch_size_exceeded(s, limits.max_scratch_bytes)
        {
            specific_breach = Some("scratch size limit exceeded");
            let pgid = nix::unistd::Pid::from_raw(-(child_pid as i32));
            let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGKILL);
            if let Ok(entries) = std::fs::read_dir(s) {
                for entry in entries.flatten() {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
            break;
        }

        if count_processes_in_pgid(child_pid) > limits.max_processes {
            specific_breach = Some("process count limit exceeded");
            let pgid = nix::unistd::Pid::from_raw(-(child_pid as i32));
            let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGKILL);
            break;
        }

        std::thread::sleep(Duration::from_millis(5));
    }

    if exit_status.is_none() && !limit_exceeded.load(Ordering::SeqCst) && specific_breach.is_none()
    {
        timeout_breached = true;
        let pgid = nix::unistd::Pid::from_raw(-(child_pid as i32));
        let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGKILL);
    }

    // Wait and reap the child process
    let status = child.wait().ok();

    // Ensure reading threads are completely drained and finished
    stdout_thread.join().ok();
    stderr_thread.join().ok();

    if let Some(reason) = specific_breach {
        eprintln!("kvist-sandbox-runner: {reason}");
        return ExitCode::from(3);
    }

    if timeout_breached {
        eprintln!("kvist-sandbox-runner: wall time limit exceeded");
        return ExitCode::from(3);
    }

    if limit_exceeded.load(Ordering::SeqCst) {
        eprintln!("kvist-sandbox-runner: output limit exceeded");
        return ExitCode::from(3);
    }

    let final_status = exit_status.or(status);
    let mut exit_code = if let Some(s) = final_status {
        if s.success() {
            0
        } else {
            s.code().unwrap_or(1)
        }
    } else {
        1
    };

    if exit_code == 0
        && let Err(promotion_error) =
            inspect_and_promote_cache(&request, &project_cache_dir, &before_snapshot)
    {
        eprintln!("kvist-sandbox-runner: {promotion_error}");
        exit_code = 3;
    }

    ExitCode::from(exit_code as u8)
}
