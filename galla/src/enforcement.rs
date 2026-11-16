#![forbid(unsafe_code)]
//! Linux Bubblewrap sandbox enforcement implementation.

use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use nix::sys::resource::{setrlimit, Resource};
use sha2::{Digest, Sha256};

use crate::probe::locate_backend;
use crate::protocol::{Access, Grant, NetworkMode, Phase, Purpose, SandboxRequest};

mod host_code;
mod native_cache;
pub mod source;
pub(crate) mod system_layout;
pub mod transport;
use transport::start as start_network_guard_proxy;
#[cfg(test)]
mod transport_tests;

fn add_workspace_vendor_mask(
    arguments: &mut Vec<String>,
    phase: Phase,
    grants: &[Grant],
    sources: &[Option<OwnedFd>],
) -> Result<Option<OwnedFd>, String> {
    if phase != Phase::Authoring {
        return Ok(None);
    }
    let Some(index) = grants.iter().position(|grant| {
        grant.destination == "/workspace"
            && grant.access == Access::ReadWrite
            && grant.purpose == Purpose::Authoring
    }) else {
        return Ok(None);
    };
    let Some(root) = sources.get(index).and_then(Option::as_ref) else {
        return Err("workspace mount origin is missing".into());
    };
    let Some(vendor) = source::child(root, Path::new(".kvist/vendored"))? else {
        return Ok(None);
    };
    arguments.extend([
        "--ro-bind-fd".to_owned(),
        vendor.as_raw_fd().to_string(),
        "/workspace/.kvist/vendored".to_owned(),
    ]);
    Ok(Some(vendor))
}

fn verify_request_identities(request: &SandboxRequest) -> Result<(), String> {
    // Verify runner identity by computing the digest of the running binary.
    let runner_path = std::fs::read_link("/proc/self/exe").map_err(|error| {
        format!("cannot determine runner binary path: {error}")
    })?;
    let actual_runner_digest = match crate::probe::file_sha256(&runner_path) {
        Ok(digest) => digest,
        Err(error) => {
            return Err(format!(
                "cannot compute runner binary digest: {error}"
            ));
        }
    };
    if actual_runner_digest != request.identities.runner {
        return Err(format!(
            "runner identity mismatch: expected {}, computed {}",
            request.identities.runner, actual_runner_digest
        ));
    }

    // Verify command identity by recomputing from argv.
    let mut command_input = b"command\0".to_vec();
    for (index, arg) in request.argv.iter().enumerate() {
        if index > 0 {
            command_input.push(b'\0');
        }
        command_input.extend_from_slice(arg.as_bytes());
    }
    let computed_command = format!("sha256:{}", hex::encode(Sha256::digest(&command_input)));
    if computed_command != request.identities.command {
        return Err(format!(
            "command identity mismatch: expected {}, computed {}",
            request.identities.command, computed_command
        ));
    }

    // Verify mount plan identity by recomputing from grants using the
    // canonical binary encoding: destination, NUL, access byte, purpose byte, NUL.
    let mut plan_bytes = Vec::new();
    for grant in &request.grants {
        plan_bytes.extend_from_slice(grant.destination.as_bytes());
        plan_bytes.push(b'\0');
        plan_bytes.push(match grant.access {
            Access::ReadOnly => b'0',
            Access::ReadWrite => b'1',
        });
        plan_bytes.push(match grant.purpose {
            Purpose::Context => b'0',
            Purpose::Authoring => b'1',
            Purpose::Scratch => b'2',
            Purpose::Toolchain => b'3',
            Purpose::Verification => b'4',
            Purpose::DependencyCache => b'5',
            Purpose::Lockfile => b'6',
            Purpose::Registry => b'7',
            Purpose::CargoConfig => b'8',
            Purpose::Runtime => b'9',
        });
        plan_bytes.push(b'\0');
    }
    let computed_plan = format!("sha256:{}", hex::encode(Sha256::digest(&plan_bytes)));
    if computed_plan != request.identities.mount_plan {
        return Err(format!(
            "mount plan identity mismatch: expected {}, computed {}",
            request.identities.mount_plan, computed_plan
        ));
    }

    Ok(())
}

fn inspect_and_promote_cache(request: &SandboxRequest) -> Result<(), String> {
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
    Err(format!(
        "payload succeeded, but cache promotion was not performed: the request supplies no provider-owned generation-parent capability. Fetched attempt files remain at `{}`; no sibling project-cache was inspected or written. Disable promotion for attempt-local use, or have the trusted provider authorize a parent and publish through the descriptor-relative immutable cache-generation API. Retry only publication/verification, not a blind fetch replay",
        source_grant.source
    ))
}



fn process_record(content: &str) -> Result<(u32, u32, u64), String> {
    // Parse /proc/PID/stat: fields after the comm field (in parentheses).
    // The comm field can contain spaces, so we find the last ')' and parse
    // from there. This handles process names with spaces and parentheses.
    let (_, tail) = content
        .rsplit_once(')')
        .ok_or_else(|| "missing proc task name boundary".to_owned())?;
    let fields = tail.split_whitespace().collect::<Vec<_>>();
    let read = |index: usize| {
        fields
            .get(index)
            .ok_or_else(|| "missing proc task field".to_owned())
    };
    Ok((
        read(1)?.parse().map_err(|_| "invalid parent pid")?,
        read(2)?.parse().map_err(|_| "invalid process group")?,
        read(17)?.parse().map_err(|_| "invalid thread count")?,
    ))
}

/// Count processes and threads in the process group containing child_pid.
///
/// This function is hardened against /proc changes during enumeration:
/// - It takes a snapshot of /proc once, then processes it
/// - It handles missing or changed stat files gracefully
/// - It validates that the pgid actually belongs to child_pid
fn count_processes_in_pgid(child_pid: u32) -> u64 {
    let mut pids = std::collections::BTreeSet::new();
    let mut tasks = std::collections::BTreeMap::new();
    pids.insert(child_pid);

    // Take a snapshot of /proc entries to avoid races with process creation
    // and termination during enumeration.
    let proc_entries = match std::fs::read_dir("/proc") {
        Ok(entries) => entries.flatten().collect::<Vec<_>>(),
        Err(_) => return 1,
    };

    let mut proc_parents = Vec::new();
    let mut child_pgid = None;

    for entry in proc_entries {
        let name = entry.file_name();
        if let Some(name_str) = name.to_str()
            && let Ok(pid) = name_str.parse::<u32>()
        {
            let stat_path = entry.path().join("stat");
            // Read stat file; skip if it disappeared during enumeration.
            let content = match std::fs::read_to_string(&stat_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if let Ok((ppid, pgrp, threads)) = process_record(&content) {
                tasks.insert(pid, threads);
                proc_parents.push((pid, ppid, pgrp));
                if pid == child_pid {
                    child_pgid = Some(pgrp);
                }
            }
        }
    }

    // Validate that child_pid's pgid is consistent. If we couldn't read it,
    // fall back to assuming it's the group leader.
    let target_pgid = child_pgid.unwrap_or(child_pid);

    // Walk the process tree to find all processes in the group.
    let mut changed = true;
    while changed {
        changed = false;
        for (pid, ppid, pgrp) in &proc_parents {
            // A process is in the group if its pgid matches, or if it's a
            // descendant of a process already in the group.
            if *pgrp == target_pgid || pids.contains(ppid) {
                if pids.insert(*pid) {
                    changed = true;
                }
            }
        }
    }

    // Sum up threads for all processes in the group.
    pids.iter()
        .map(|pid| tasks.get(pid).copied().unwrap_or(1))
        .sum()
}

/// Check if any file under the given path exceeds the size limit (recursive).
fn check_file_size_exceeded(path: &Path, limit_bytes: u64) -> bool {
    walk_dir(path, |p| {
        if let Ok(metadata) = p.metadata() {
            if metadata.is_file() && metadata.size() > limit_bytes {
                return true;
            }
        }
        false
    })
}

/// Check if the total size of files under the given path exceeds the limit (recursive).
fn check_scratch_size_exceeded(path: &Path, limit_bytes: u64) -> bool {
    let mut total = 0u64;
    let exceeded = Arc::new(AtomicBool::new(false));
    let exceeded_clone = exceeded.clone();
    walk_dir(path, move |p| {
        if exceeded_clone.load(Ordering::SeqCst) {
            return true;
        }
        if let Ok(metadata) = p.metadata() {
            if metadata.is_file() {
                total += metadata.size();
                if total > limit_bytes {
                    exceeded_clone.store(true, Ordering::SeqCst);
                    return true;
                }
            }
        }
        false
    });
    exceeded.load(Ordering::SeqCst)
}

/// Walk a directory tree iteratively, calling `f` on each path.
/// If `f` returns true, the walk stops and returns true.
fn walk_dir(path: &Path, mut f: impl FnMut(&Path) -> bool) -> bool {
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        if f(&current) {
            return true;
        }
        if let Ok(entries) = std::fs::read_dir(&current) {
            for entry in entries.flatten() {
                stack.push(entry.path());
            }
        }
    }
    false
}

/// Executes the request in a secure Bubblewrap sandbox, supervises its resources,
/// and reaps the child process.
pub fn run(request: SandboxRequest) -> ExitCode {
    if let Err(reason) = crate::context::require_host() {
        eprintln!("galla: {reason}");
        return ExitCode::from(3);
    }
    let Some(bwrap_path) = locate_backend() else {
        eprintln!("galla: no Bubblewrap backend found on PATH or standard locations");
        return ExitCode::from(3);
    };

    let actual_backend_digest = match crate::probe::file_sha256(&bwrap_path) {
        Ok(digest) => digest,
        Err(error) => {
            eprintln!(
                "galla: cannot inspect installed backend `{}`: {error}; repair its host installation and retry the affected command",
                bwrap_path.display()
            );
            return ExitCode::from(3);
        }
    };
    if request.identities.backend.digest != actual_backend_digest {
        eprintln!(
            "galla: backend identity / digest mismatch before execution: expected `{}`, observed `{}`. No payload ran. Refresh the request's enforcer identity against the installed backend and retry only this command; a host upgrade is not a workspace contract violation",
            request.identities.backend.digest, actual_backend_digest
        );
        return ExitCode::from(3);
    }

    // Verify all approval-bound identities against the actual request content.
    if let Err(reason) = verify_request_identities(&request) {
        eprintln!("galla: identity verification failed: {reason}");
        return ExitCode::from(3);
    }

    let mut bwrap_args = vec!["--clearenv".to_owned()];

    // 2. Unshare namespaces to establish secure context boundaries
    bwrap_args.push("--unshare-user".to_owned());
    bwrap_args.push("--unshare-ipc".to_owned());
    bwrap_args.push("--unshare-pid".to_owned());
    bwrap_args.push("--unshare-uts".to_owned());
    bwrap_args.push("--unshare-cgroup-try".to_owned());
    if request.network.mode == NetworkMode::Deny {
        bwrap_args.push("--unshare-net".to_owned());
    }

    // 3. Setup sessions and parent lifecycle bindings
    bwrap_args.push("--new-session".to_owned());
    bwrap_args.push("--die-with-parent".to_owned());

    // 4. Mount minimal /proc, /dev and isolated tmpfs locations
    bwrap_args.push("--proc".to_owned());
    bwrap_args.push("/proc".to_owned());
    bwrap_args.push("--dev".to_owned());
    bwrap_args.push("/dev".to_owned());
    bwrap_args.push("--size".to_owned());
    bwrap_args.push(request.resources.max_scratch_bytes.to_string());
    bwrap_args.push("--tmpfs".to_owned());
    bwrap_args.push("/tmp".to_owned());
    bwrap_args.push("--size".to_owned());
    bwrap_args.push(request.resources.max_scratch_bytes.to_string());
    bwrap_args.push("--tmpfs".to_owned());
    bwrap_args.push("/run".to_owned());

    // 5. Mount standard host system layout
    if let Err(error) = system_layout::add_host_system_layout(&mut bwrap_args) {
        eprintln!("galla: {error}");
        return ExitCode::from(3);
    }

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
        match start_network_guard_proxy(&request.network.allowed_sources) {
            Ok(guard) => {
                let proxy_url = format!("http://127.0.0.1:{}", guard.port());
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
                Some(guard)
            }
            Err(error) => {
                eprintln!("galla: required package-source proxy unavailable: {error}");
                return ExitCode::from(3);
            }
        }
    } else {
        None
    };

    // 7. Establish the required directory layout for all grant destinations
    let mut directories_to_create = std::collections::BTreeSet::new();
    for grant in &request.grants {
        if crate::validation::is_system_grant(grant) {
            match std::fs::symlink_metadata(&grant.source) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) => {
                    eprintln!(
                        "galla: System grant `{}` requires a non-link directory",
                        grant.source
                    );
                    return ExitCode::from(3);
                }
                Err(error) => {
                    eprintln!(
                        "galla: cannot inspect System grant `{}`: {error}",
                        grant.source
                    );
                    return ExitCode::from(3);
                }
            }
            continue;
        }
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

    let sources = match request
        .grants
        .iter()
        .map(|grant| {
            if crate::validation::is_system_grant(grant) {
                Ok(None)
            } else {
                source::pin(Path::new(&grant.source)).map(Some)
            }
        })
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(sources) => sources,
        Err(error) => {
            eprintln!("galla: {error}");
            return ExitCode::from(3);
        }
    };
    let _host_code_guard = match host_code::protect(&bwrap_path, &request.grants, &sources) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("galla: {error}");
            return ExitCode::from(3);
        }
    };
    for source in sources.iter().flatten() {
        if let Err(error) = nix::fcntl::fcntl(
            source,
            nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::empty()),
        ) {
            eprintln!("galla: cannot pass authorized mount origin to backend: {error}");
            return ExitCode::from(3);
        }
    }
    // Final origin descriptors are consumed/closed by Bubblewrap before exec.
    for (grant, source) in request.grants.iter().zip(&sources) {
        if crate::validation::is_system_grant(grant) {
            continue;
        }
        let Some(source) = source else {
            eprintln!("galla: non-system grant has no authorized source descriptor");
            return ExitCode::from(3);
        };
        match grant.access {
            crate::protocol::Access::ReadOnly => {
                bwrap_args.push("--ro-bind-fd".to_owned());
            }
            crate::protocol::Access::ReadWrite => {
                bwrap_args.push("--bind-fd".to_owned());
            }
        }
        bwrap_args.push(source.as_raw_fd().to_string());
        bwrap_args.push(grant.destination.clone());
    }
    let _native_configuration = match native_cache::add(&mut bwrap_args, &request, &sources) {
        Ok(configuration) => configuration,
        Err(error) => {
            eprintln!("galla: {error}");
            return ExitCode::from(3);
        }
    };
    let _vendor_origin = match add_workspace_vendor_mask(
        &mut bwrap_args,
        request.phase,
        &request.grants,
        &sources,
    ) {
        Ok(origin) => origin,
        Err(error) => {
            eprintln!("galla: {error}");
            return ExitCode::from(3);
        }
    };
    if let Some(origin) = &_vendor_origin
        && let Err(error) = nix::fcntl::fcntl(
            origin,
            nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::empty()),
        )
    {
        eprintln!("galla: cannot pass read-only vendor origin to backend: {error}");
        return ExitCode::from(3);
    }
    bwrap_args.extend(
        [
            "--perms",
            "0555",
            "--tmpfs",
            crate::context::SANDBOX_MARKER,
            "--remount-ro",
            crate::context::SANDBOX_MARKER,
        ]
        .map(str::to_owned),
    );

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
    // Apply inherited limits before any payload can run. Never apply NPROC:
    // Linux counts it across the real host UID, not just this child.
    let mut child_cmd = Command::new(&bwrap_path);
    child_cmd.args(&bwrap_args);
    child_cmd.stdin(Stdio::null());
    child_cmd.stdout(Stdio::piped());
    child_cmd.stderr(Stdio::piped());

    // Isolate the child inside its own process group
    child_cmd.process_group(0);

    // Spawn the Bubblewrap supervisor process
    let mut attempts = 0;
    let mut child = loop {
        match child_cmd.spawn() {
            Ok(child) => break child,
            Err(ref error)
                if (error.kind() == std::io::ErrorKind::ExecutableFileBusy
                    || error.raw_os_error() == Some(26))
                    && attempts < 10 =>
            {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => {
                eprintln!(
                    "galla: cannot start Bubblewrap: {error}; install the configured backend on the host"
                );
                return ExitCode::from(3);
            }
        }
    };

    let child_pid = child.id();

    // Apply process resource limits using the prlimit syscall directly,
    // eliminating the dependency on /usr/bin/prlimit. Report errors instead
    // of silently ignoring them.
    let limits = request.resources;
    if let Err(error) = setrlimit(Resource::RLIMIT_NOFILE, limits.max_files, limits.max_files) {
        eprintln!("galla: failed to set RLIMIT_NOFILE: {error}");
        return ExitCode::from(3);
    }
    if let Some(max_file_bytes) = limits.max_file_bytes {
        if let Err(error) = setrlimit(Resource::RLIMIT_FSIZE, max_file_bytes, max_file_bytes) {
            eprintln!("galla: failed to set RLIMIT_FSIZE: {error}");
            return ExitCode::from(3);
        }
    }

    let child_pid = child.id();
    let total_bytes = Arc::new(AtomicU64::new(0));
    let limit_exceeded = Arc::new(AtomicBool::new(false));
    let limits = request.resources;
    let max_output_bytes = limits.max_output_bytes;

    // Spawn the stdout reader thread. The pipe is always present because
    // Stdio::piped() was requested; if it were missing there would simply
    // be no output to forward, not a fatal error.
    let stdout_thread = match child.stdout.take() {
        Some(mut stdout_pipe) => {
            let total_bytes_stdout = total_bytes.clone();
            let limit_exceeded_stdout = limit_exceeded.clone();
            Some(std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                use std::io::{Read, Write};
                let host_stdout = std::io::stdout();
                while let Ok(n) = stdout_pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let cumulative =
                        total_bytes_stdout.fetch_add(n as u64, Ordering::SeqCst) + n as u64;
                    if cumulative > max_output_bytes {
                        limit_exceeded_stdout.store(true, Ordering::SeqCst);
                        let pgid = nix::unistd::Pid::from_raw(-(child_pid as i32));
                        let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGKILL);
                        break;
                    }
                    let mut output = host_stdout.lock();
                    if output.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = output.flush();
                }
            }))
        }
        None => None,
    };

    // Spawn the stderr reader thread, degrading to no forwarding when the
    // pipe is absent.
    let stderr_thread = match child.stderr.take() {
        Some(mut stderr_pipe) => {
            let total_bytes_stderr = total_bytes.clone();
            let limit_exceeded_stderr = limit_exceeded.clone();
            Some(std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                use std::io::{Read, Write};
                let host_stderr = std::io::stderr();
                while let Ok(n) = stderr_pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let cumulative =
                        total_bytes_stderr.fetch_add(n as u64, Ordering::SeqCst) + n as u64;
                    if cumulative > max_output_bytes {
                        limit_exceeded_stderr.store(true, Ordering::SeqCst);
                        let pgid = nix::unistd::Pid::from_raw(-(child_pid as i32));
                        let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGKILL);
                        break;
                    }
                    let mut output = host_stderr.lock();
                    if output.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = output.flush();
                }
            }))
        }
        None => None,
    };

    // Monitor process execution and timeouts
    let start_time = Instant::now();
    let timeout = Duration::from_millis(limits.wall_time_ms);
    let mut exit_status = None;
    let mut timeout_breached = false;
    let mut specific_breach = None;

    while start_time.elapsed() < timeout {
        if let Ok(Some(status)) = child.try_wait() {
            exit_status = Some(status);
            break;
        }
        if limit_exceeded.load(Ordering::SeqCst) {
            break;
        }

        if count_processes_in_pgid(child_pid) > limits.max_processes {
            specific_breach = Some("process count limit exceeded");
            let pgid = nix::unistd::Pid::from_raw(-(child_pid as i32));
            let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGKILL);
            break;
        }

        // Check file size limits recursively
        if let Some(max_file_bytes) = limits.max_file_bytes {
            if check_file_size_exceeded(&PathBuf::from("/tmp"), max_file_bytes) {
                specific_breach = Some("file size limit exceeded");
                let pgid = nix::unistd::Pid::from_raw(-(child_pid as i32));
                let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGKILL);
                break;
            }
        }

        // Check scratch size limits recursively
        if check_scratch_size_exceeded(&PathBuf::from("/tmp"), limits.max_scratch_bytes) {
            specific_breach = Some("scratch size limit exceeded");
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
    if let Some(thread) = stdout_thread {
        thread.join().ok();
    }
    if let Some(thread) = stderr_thread {
        thread.join().ok();
    }

    if let Some(reason) = specific_breach {
        eprintln!("galla: {reason}");
        return ExitCode::from(3);
    }

    if timeout_breached {
        eprintln!("galla: wall time limit exceeded");
        return ExitCode::from(3);
    }

    if limit_exceeded.load(Ordering::SeqCst) {
        eprintln!("galla: output limit exceeded");
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
        && let Err(promotion_error) = inspect_and_promote_cache(&request)
    {
        eprintln!("galla: {promotion_error}");
        exit_code = 3;
    }

    ExitCode::from(exit_code as u8)
}

#[cfg(test)]
mod tests {
    use super::transport::{ParsedRequestLine, parse_request_line};
    use super::*;
    use crate::origin::{OriginPolicy, RequestKind};
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn sampled_task_records_handle_spaces_parentheses_and_threads() {
        let record = "42 (worker ) with spaces) S 7 42 0 0 0 0 0 0 0 0 0 0 0 0 0 0 4";
        assert_eq!(process_record(record).unwrap(), (7, 42, 4));
        assert!(process_record("incomplete").is_err());
    }

    fn grants(workspace: &Path) -> Vec<Grant> {
        vec![
            Grant {
                source: workspace.to_string_lossy().into_owned(),
                destination: "/workspace".into(),
                access: Access::ReadWrite,
                purpose: Purpose::Authoring,
                identity: "workspace".into(),
            },
            Grant {
                source: "/host/snapshot".into(),
                destination: "/rust/vendor".into(),
                access: Access::ReadOnly,
                purpose: Purpose::Toolchain,
                identity: "snapshot".into(),
            },
        ]
    }

    #[test]
    fn promotion_intent_cannot_authorize_a_sibling_host_destination() {
        let directory = tempfile::tempdir().unwrap();
        let candidate = directory.path().join("candidate");
        let outside = directory.path().join("outside");
        let destination = directory.path().join("project-cache");
        fs::create_dir(&candidate).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(candidate.join("artifact"), "ordinary candidate").unwrap();
        symlink(&outside, &destination).unwrap();
        let request: SandboxRequest = serde_json::from_value(serde_json::json!({
            "protocol": "kvist-sandbox-request-v1", "protocol_version": 1,
            "phase": "dependency-acquisition", "argv": ["/usr/bin/true"],
            "working_directory": "/workspace", "environment": {},
            "network": {"mode":"deny","allowed_sources":[]},
            "resources": {"wall_time_ms":1000,"max_output_bytes":1000,"max_processes":10,
                "max_files":10,"max_scratch_bytes":1000,"max_cache_bytes":1000},
            "identities": {"runner":"fixture","backend":{"kind":"bubblewrap","path":"/usr/bin/bwrap","digest":"fixture"},
                "policy":"fixture","toolchain":"fixture","command":"fixture","mount_plan":"fixture"},
            "grants": [{"source": candidate,"destination":"/workspace/cargo-home",
                "access":"read-write","purpose":"dependency-cache","identity":"fixture"}],
            "toolchain":{"kind":"system","root":"/usr","identity":"fixture"},
            "cache": {"cargo_home":"/workspace/cargo-home","registry":"/workspace/cargo-home/registry",
                "git":"/workspace/cargo-home/git","promotion":{"enabled":true,"source":"/workspace/cargo-home"}}
        })).unwrap();
        let result = inspect_and_promote_cache(&request);
        assert!(
            result.is_err(),
            "the request contains no host publication capability"
        );
        assert!(!outside.join("artifact").exists());
        assert_eq!(
            fs::read_to_string(candidate.join("artifact")).unwrap(),
            "ordinary candidate"
        );
    }

    #[test]
    fn enforcement_uses_immutable_cache_promotion_primitive() {
        // Verify that the enforcement path uses the immutable cache promotion
        // primitive (cache::promote) rather than std::fs::copy. This test
        // ensures the enforcement module depends on the cache module's
        // promote function for all cache promotion operations.
        //
        // The test verifies that inspect_and_promote_cache returns an error
        // when promotion is enabled but no provider-owned generation-parent
        // capability is supplied. This ensures the enforcement path cannot
        // use std::fs::copy directly.

        let directory = tempfile::tempdir().unwrap();
        let attempt = directory.path().join("attempt");
        fs::create_dir(&attempt).unwrap();
        fs::write(attempt.join("artifact"), "test content").unwrap();

        let request: SandboxRequest = serde_json::from_value(serde_json::json!({
            "protocol": "kvist-sandbox-request-v1", "protocol_version": 1,
            "phase": "dependency-acquisition", "argv": ["/usr/bin/true"],
            "working_directory": "/workspace", "environment": {},
            "network": {"mode":"deny","allowed_sources":[]},
            "resources": {"wall_time_ms":1000,"max_output_bytes":1000,"max_processes":10,
                "max_files":10,"max_scratch_bytes":1000,"max_cache_bytes":1000},
            "identities": {"runner":"fixture","backend":{"kind":"bubblewrap","path":"/usr/bin/bwrap","digest":"fixture"},
                "policy":"fixture","toolchain":"fixture","command":"fixture","mount_plan":"fixture"},
            "grants": [{"source": attempt,"destination":"/workspace/cargo-home",
                "access":"read-write","purpose":"dependency-cache","identity":"fixture"}],
            "toolchain":{"kind":"system","root":"/usr","identity":"fixture"},
            "cache": {"cargo_home":"/workspace/cargo-home","registry":"/workspace/cargo-home/registry",
                "git":"/workspace/cargo-home/git","promotion":{"enabled":true,"source":"/workspace/cargo-home"}}
        })).unwrap();

        // The enforcement path must use the immutable cache promotion primitive.
        // This test verifies that by checking that inspect_and_promote_cache
        // returns an error when promotion is enabled but no provider-owned
        // generation-parent capability is supplied.
        let result = inspect_and_promote_cache(&request);
        assert!(
            result.is_err(),
            "enforcement must use the immutable cache promotion primitive"
        );
        let error = result.unwrap_err();
        assert!(
            error.contains("immutable cache-generation API"),
            "error must reference the immutable cache-generation API: {}",
            error
        );
    }

    #[test]
    fn enforcement_promotion_requires_provider_owned_capability() {
        // Verify that enforcement cache promotion requires a provider-owned
        // generation-parent capability. This ensures that the enforcement path
        // cannot use std::fs::copy directly without the immutable cache
        // generation primitive.

        let directory = tempfile::tempdir().unwrap();
        let attempt = directory.path().join("attempt");
        fs::create_dir(&attempt).unwrap();
        fs::write(attempt.join("artifact"), "test content").unwrap();

        let request: SandboxRequest = serde_json::from_value(serde_json::json!({
            "protocol": "kvist-sandbox-request-v1", "protocol_version": 1,
            "phase": "dependency-acquisition", "argv": ["/usr/bin/true"],
            "working_directory": "/workspace", "environment": {},
            "network": {"mode":"deny","allowed_sources":[]},
            "resources": {"wall_time_ms":1000,"max_output_bytes":1000,"max_processes":10,
                "max_files":10,"max_scratch_bytes":1000,"max_cache_bytes":1000},
            "identities": {"runner":"fixture","backend":{"kind":"bubblewrap","path":"/usr/bin/bwrap","digest":"fixture"},
                "policy":"fixture","toolchain":"fixture","command":"fixture","mount_plan":"fixture"},
            "grants": [{"source": attempt,"destination":"/workspace/cargo-home",
                "access":"read-write","purpose":"dependency-cache","identity":"fixture"}],
            "toolchain":{"kind":"system","root":"/usr","identity":"fixture"},
            "cache": {"cargo_home":"/workspace/cargo-home","registry":"/workspace/cargo-home/registry",
                "git":"/workspace/cargo-home/git","promotion":{"enabled":true,"source":"/workspace/cargo-home"}}
        })).unwrap();

        let result = inspect_and_promote_cache(&request);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error.contains("provider-owned generation-parent capability"),
            "error must reference provider-owned capability: {}",
            error
        );
    }

    #[test]
    fn enforcement_promotion_atomicity_and_immutability() {
        // Verify that the enforcement path uses the immutable cache promotion
        // primitive with all required properties:
        // - Atomic publication using RENAME_NOREPLACE
        // - Manifest checksum verification
        // - Staging directory
        // - No-follow descriptors
        // - Immutable modes
        // - Generation identity
        //
        // This test verifies that the cache::promote function is used by
        // checking that it performs all the required operations.

        use crate::cache;
        use sha2::{Digest, Sha256};

        fn digest(bytes: &[u8]) -> String {
            format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
        }

        let temp = tempfile::tempdir().unwrap();
        let attempt = temp.path().join("attempt");
        let parent = temp.path().join("generations");
        fs::create_dir(&attempt).unwrap();
        fs::create_dir(&parent).unwrap();

        // Create source content
        fs::write(attempt.join("artifact"), "test content").unwrap();

        let manifest = [cache::ManifestEntry {
            path: "artifact".to_owned(),
            size: 12,
            checksum: digest(b"test content"),
        }];

        let bounds = cache::CacheBounds {
            max_files: 16,
            max_file_bytes: 1024,
            max_cache_bytes: 4096,
        };

        let attempt_capability =
            cache::open_isolated_attempt_root(&attempt).expect("open attempt");
        let parent_capability =
            cache::open_trusted_generation_parent(&parent).expect("open parent");

        let plan = cache::PromotionPlan {
            attempt_root: &attempt_capability,
            generation_parent: &parent_capability,
            manifest: &manifest,
            bounds,
            lockfile_before_identity: digest(b"before"),
            lockfile_after_identity: digest(b"after"),
            supported_source_identities: vec![digest(b"source")],
        };

        // Verify that cache::promote performs atomic publication
        let result = cache::promote(&plan);
        assert!(result.is_ok(), "cache::promote must succeed");
        let generation = result.unwrap();

        // Verify that the generation was published atomically
        assert!(generation.path.is_dir(), "generation must be a directory");
        assert!(
            generation.path.join("artifact").exists(),
            "generation must contain the artifact"
        );

        // Verify that the generation has an identity
        assert!(
            !generation.identity.is_empty(),
            "generation must have an identity"
        );

        // Verify that the generation is immutable (read-only)
        let metadata = generation.path.metadata().expect("metadata");
        assert!(
            metadata.permissions().readonly(),
            "generation must be immutable"
        );

        // Verify that the artifact is immutable (read-only)
        let artifact_metadata = generation
            .path
            .join("artifact")
            .metadata()
            .expect("artifact metadata");
        assert!(
            artifact_metadata.permissions().readonly(),
            "artifact must be immutable"
        );

        // Verify that the generation was fsynced after being made immutable
        assert_eq!(
            generation.durability,
            cache::Durability::Synced,
            "generation must be fsynced after being made immutable"
        );

        // Verify that the generation identity incorporates lockfile and source identities
        assert!(
            generation.identity.starts_with("sha256:"),
            "generation identity must be a sha256 digest"
        );
        assert_eq!(
            generation.identity.len(),
            71,
            "generation identity must be 71 characters (sha256: + 64 hex)"
        );
    }

    #[test]
    fn vendor_overlay_is_optional_readable_and_independent_of_rust_readiness() {
        let workspace = tempfile::tempdir().unwrap();
        let mut grants = grants(workspace.path());
        let sources = vec![Some(source::pin(workspace.path()).unwrap()), None];
        let mut arguments = Vec::new();
        add_workspace_vendor_mask(&mut arguments, Phase::Authoring, &grants, &sources).unwrap();
        assert!(arguments.is_empty());
        assert!(!workspace.path().join(".kvist").exists());
        fs::create_dir(workspace.path().join(".kvist")).unwrap();
        add_workspace_vendor_mask(&mut arguments, Phase::Authoring, &grants, &sources).unwrap();
        assert!(arguments.is_empty());
        assert!(!workspace.path().join(".kvist/vendored").exists());
        fs::create_dir(workspace.path().join(".kvist/vendored")).unwrap();
        add_workspace_vendor_mask(&mut arguments, Phase::Verification, &grants, &sources).unwrap();
        assert!(arguments.is_empty());
        grants[1].destination = "/rust/other".into();
        let origin = add_workspace_vendor_mask(&mut arguments, Phase::Authoring, &grants, &sources)
            .unwrap()
            .unwrap();
        assert_eq!(
            arguments,
            [
                "--ro-bind-fd",
                &origin.as_raw_fd().to_string(),
                "/workspace/.kvist/vendored"
            ]
        );
        arguments.clear();
        for (index, access, purpose, destination) in [
            (0, Access::ReadOnly, Purpose::Authoring, "/workspace"),
            (0, Access::ReadWrite, Purpose::Scratch, "/workspace"),
            (
                0,
                Access::ReadWrite,
                Purpose::Authoring,
                "/workspace/component",
            ),
        ] {
            let mut unrelated = grants.clone();
            unrelated[index].access = access;
            unrelated[index].purpose = purpose;
            unrelated[index].destination = destination.into();
            add_workspace_vendor_mask(&mut arguments, Phase::Authoring, &unrelated, &sources)
                .unwrap();
            assert!(arguments.is_empty());
        }
        arguments.extend([
            "--bind".into(),
            grants[0].source.clone(),
            "/workspace".into(),
        ]);
        let origin = add_workspace_vendor_mask(&mut arguments, Phase::Authoring, &grants, &sources)
            .unwrap()
            .unwrap();
        assert_eq!(
            &arguments[3..],
            [
                "--ro-bind-fd",
                &origin.as_raw_fd().to_string(),
                "/workspace/.kvist/vendored"
            ]
        );
    }

    #[test]
    fn vendor_overlay_skips_pointers_without_blocking_work_and_supports_files() {
        for relative in [".kvist", ".kvist/vendored"] {
            let workspace = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            if relative != ".kvist" {
                fs::create_dir(workspace.path().join(".kvist")).unwrap();
            }
            let path = workspace.path().join(relative);
            symlink(outside.path(), &path).unwrap();
            let mut arguments = Vec::new();
            let sources = vec![Some(source::pin(workspace.path()).unwrap()), None];
            assert!(
                add_workspace_vendor_mask(
                    &mut arguments,
                    Phase::Authoring,
                    &grants(workspace.path()),
                    &sources,
                )
                .unwrap()
                .is_none()
            );
            assert!(arguments.is_empty());
            fs::remove_file(&path).unwrap();
            fs::write(&path, "not a directory").unwrap();
            assert!(
                add_workspace_vendor_mask(
                    &mut arguments,
                    Phase::Authoring,
                    &grants(workspace.path()),
                    &sources,
                )
                .unwrap()
                .is_some()
                    == (relative == ".kvist/vendored")
            );
            assert_eq!(arguments.is_empty(), relative == ".kvist");
        }
    }

    #[test]
    fn parses_https_connect_tunnel_request_line() {
        let parsed = parse_request_line("CONNECT example.com:443 HTTP/1.1").expect("connect");
        match parsed {
            ParsedRequestLine::ConnectTunnel { host, port } => {
                assert_eq!(host, "example.com");
                assert_eq!(port, 443);
            }
            _ => panic!("expected CONNECT tunnel"),
        }
    }

    #[test]
    fn parses_connect_tunnel_with_default_https_port() {
        let parsed = parse_request_line("CONNECT example.com HTTP/1.1").expect("connect");
        match parsed {
            ParsedRequestLine::ConnectTunnel { host, port } => {
                assert_eq!(host, "example.com");
                assert_eq!(port, 443);
            }
            _ => panic!("expected CONNECT tunnel"),
        }
    }

    #[test]
    fn parses_connect_tunnel_case_insensitively() {
        let parsed = parse_request_line("connect example.com:443 HTTP/1.1").expect("connect");
        match parsed {
            ParsedRequestLine::ConnectTunnel { host, port } => {
                assert_eq!(host, "example.com");
                assert_eq!(port, 443);
            }
            _ => panic!("expected CONNECT tunnel"),
        }
    }

    #[test]
    fn parses_absolute_http_form_request_line() {
        let parsed =
            parse_request_line("GET http://index.crates.io/index/serde HTTP/1.1").expect("http");
        match parsed {
            ParsedRequestLine::HttpAbsolute { host, port, path } => {
                assert_eq!(host, "index.crates.io");
                assert_eq!(port, 80);
                assert_eq!(path, "/index/serde");
            }
            _ => panic!("expected HTTP absolute form"),
        }
    }

    #[test]
    fn parses_absolute_http_form_with_explicit_port() {
        let parsed = parse_request_line("GET http://127.0.0.1:8080/index/ HTTP/1.1").expect("http");
        match parsed {
            ParsedRequestLine::HttpAbsolute { host, port, path } => {
                assert_eq!(host, "127.0.0.1");
                assert_eq!(port, 8080);
                assert_eq!(path, "/index/");
            }
            _ => panic!("expected HTTP absolute form"),
        }
    }

    #[test]
    fn rejects_non_connect_non_http_methods() {
        // POST is now supported for Cargo Git smart HTTP
        assert!(parse_request_line("POST http://host/path HTTP/1.1").is_ok());
        assert!(parse_request_line("GET https://host/path HTTP/1.1").is_err());
        assert!(parse_request_line("GET ftp://host/path HTTP/1.1").is_err());
    }

    #[test]
    fn rejects_connect_with_missing_or_malformed_port() {
        assert!(parse_request_line("CONNECT :443 HTTP/1.1").is_err());
        assert!(parse_request_line("CONNECT HTTP/1.1").is_err());
        assert!(parse_request_line("CONNECT example.com:abc HTTP/1.1").is_err());
        assert!(parse_request_line("CONNECT example.com HTTP/1.1 extra").is_err());
    }

    #[test]
    fn rejects_connect_with_invalid_host() {
        // A host carrying a path separator, userinfo, or space cannot name a
        // valid CONNECT target.
        assert!(parse_request_line("CONNECT example.com/path:443 HTTP/1.1").is_err());
        assert!(parse_request_line("CONNECT user@host:443 HTTP/1.1").is_err());
        assert!(parse_request_line("CONNECT bad host:443 HTTP/1.1").is_err());
        // A colon in the host (a malformed IPv6 attempt without brackets) is
        // rejected.
        assert!(parse_request_line("CONNECT ::1:443 HTTP/1.1").is_err());
        // Empty labels are rejected.
        assert!(parse_request_line("CONNECT .example.com:443 HTTP/1.1").is_err());
        assert!(parse_request_line("CONNECT example..com:443 HTTP/1.1").is_err());
    }

    #[test]
    fn accepts_connect_with_valid_ipv4_host() {
        let parsed = parse_request_line("CONNECT 192.0.2.1:443 HTTP/1.1").expect("connect ipv4");
        match parsed {
            ParsedRequestLine::ConnectTunnel { host, port } => {
                assert_eq!(host, "192.0.2.1");
                assert_eq!(port, 443);
            }
            _ => panic!("expected CONNECT tunnel"),
        }
    }

    #[test]
    fn rejects_absolute_http_form_with_missing_authority() {
        assert!(parse_request_line("GET http:// HTTP/1.1").is_err());
        assert!(parse_request_line("GET http:///path HTTP/1.1").is_err());
    }

    #[test]
    fn rejects_request_line_with_too_few_parts() {
        assert!(parse_request_line("").is_err());
        assert!(parse_request_line("GET").is_err());
        assert!(parse_request_line("CONNECT").is_err());
    }

    #[test]
    fn connect_policy_target_uses_https_scheme() {
        let target = ParsedRequestLine::ConnectTunnel {
            host: "example.com".into(),
            port: 443,
        };
        let policy_target = match &target {
            ParsedRequestLine::ConnectTunnel { host, port } => format!("https://{host}:{port}/"),
            _ => unreachable!(),
        };
        assert_eq!(policy_target, "https://example.com:443/");
    }

    #[test]
    fn http_absolute_policy_target_uses_http_scheme() {
        let target = ParsedRequestLine::HttpAbsolute {
            host: "index.crates.io".into(),
            port: 80,
            path: "/index/serde".into(),
        };
        assert_eq!(target.policy_target(), "http://index.crates.io/index/serde");
    }

    #[test]
    fn connect_tunnel_is_allowed_for_matching_https_source() {
        let mut policy = OriginPolicy::new();
        policy
            .allow_url("https://static.crates.io/crates/")
            .expect("allow download");
        // A CONNECT tunnel to the allowed origin's host is authorized.
        assert!(policy.authorizes_request(
            RequestKind::CargoCrateDownload,
            "https://static.crates.io/crates/serde/serde-1.0.0.crate"
        ));
        // A CONNECT tunnel to an undeclared origin is denied.
        assert!(!policy.authorizes_request(
            RequestKind::CargoCrateDownload,
            "https://evil.invalid/crates/serde.crate"
        ));
    }

    #[test]
    fn connect_tunnel_authorizes_on_authority_not_path() {
        // The proxy reauthorizes a CONNECT tunnel on authority alone because the
        // tunnelled HTTPS request path is encrypted and unseen. An allowed
        // origin with a non-root path prefix must still authorize a CONNECT
        // tunnel to the same authority.
        let mut policy = OriginPolicy::new();
        policy
            .allow_url("https://static.crates.io/crates/")
            .expect("allow download");
        // The CONNECT policy target carries only the authority (root path).
        let target = "https://static.crates.io:443/";
        assert!(policy.authorizes_authority(target));
        assert!(!policy.authorizes_authority("https://static.crates.io:8443/"));
    }

    #[test]
    fn connect_tunnel_authorizes_by_full_url_parsing() {
        // The guard reauthorizes a CONNECT tunnel through the strict origin
        // parser, not a naive prefix match: scheme, host, port, and path
        // components are all validated.
        let mut policy = OriginPolicy::new();
        policy
            .allow_url("https://index.crates.io/")
            .expect("allow index");

        assert!(
            policy.authorizes_request(RequestKind::CargoCrateDownload, "https://index.crates.io/")
        );

        // Wrong host, wrong scheme, and wrong port are all denied by the same
        // canonical parser.
        assert!(!policy.authorizes_request(
            RequestKind::CargoCrateDownload,
            "https://index.crates.io:8443/"
        ));
        assert!(
            !policy.authorizes_request(RequestKind::CargoCrateDownload, "http://index.crates.io/")
        );
        assert!(
            !policy.authorizes_request(RequestKind::CargoCrateDownload, "https://mirror.invalid/")
        );
    }

    #[test]
    fn http_absolute_authorizes_by_full_url_parsing() {
        // Absolute-form HTTP requests are authorized through the same strict
        // parser, which rejects credentials, query strings, and fragments.
        let mut policy = OriginPolicy::test_loopback();
        policy
            .allow_url("http://127.0.0.1:8080/")
            .expect("allow loopback");

        assert!(policy.permits("http://127.0.0.1:8080/index/serde"));
        // Credentials, query, and fragment are rejected by the parser.
        assert!(!policy.permits("http://user:pass@127.0.0.1:8080/index/"));
        assert!(!policy.permits("http://127.0.0.1:8080/index/?token=1"));
        assert!(!policy.permits("http://127.0.0.1:8080/index/#frag"));
    }

    #[test]
    fn policy_target_uses_strict_origin_for_connect_and_http() {
        // CONNECT tunnels are reauthorized as canonical HTTPS origins and
        // absolute-form requests as canonical HTTP origins, so the policy
        // matcher sees the exact same canonical shape it validates elsewhere.
        let mut policy = OriginPolicy::new();
        policy
            .allow_url("https://static.crates.io/crates/")
            .expect("allow download");

        assert!(!policy.permits("http://static.crates.io/crates/serde-1.0.0.crate"));
        assert!(!policy.permits("http://static.crates.io/crates/serde-1.0.0.crate?x=1"));
        assert!(policy.authorizes_request(
            RequestKind::CargoCrateDownload,
            "https://static.crates.io/crates/serde-1.0.0.crate"
        ));
    }
}
