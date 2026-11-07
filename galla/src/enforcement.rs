#![forbid(unsafe_code)]
//! Linux Bubblewrap sandbox enforcement implementation.

use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::probe::locate_backend;
use crate::protocol::{Access, Grant, NetworkMode, Phase, Purpose, SandboxRequest};

mod host_code;
mod native_cache;
pub mod source;
pub(crate) mod system_layout;
mod transport;
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

fn count_processes_in_pgid(child_pid: u32) -> u64 {
    let mut pids = std::collections::BTreeSet::new();
    let mut tasks = std::collections::BTreeMap::new();
    pids.insert(child_pid);
    if let Ok(entries) = std::fs::read_dir("/proc") {
        let mut proc_parents = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(name_str) = name.to_str()
                && let Ok(pid) = name_str.parse::<u32>()
            {
                let stat_path = entry.path().join("stat");
                if let Ok(content) = std::fs::read_to_string(&stat_path)
                    && let Ok((ppid, pgrp, threads)) = process_record(&content)
                {
                    tasks.insert(pid, threads);
                    proc_parents.push((pid, ppid, pgrp));
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
    pids.iter()
        .map(|pid| tasks.get(pid).copied().unwrap_or(1))
        .sum()
}

/// Executes the request in a secure Bubblewrap sandbox, supervises its resources,
/// and reaps the child process.
pub fn run(request: SandboxRequest) -> ExitCode {
    if let Err(reason) = crate::context::require_host() {
        eprintln!("galla-runner: {reason}");
        return ExitCode::from(3);
    }
    let Some(bwrap_path) = locate_backend() else {
        eprintln!("galla-runner: no Bubblewrap backend found on PATH or standard locations");
        return ExitCode::from(3);
    };

    let actual_backend_digest = match crate::probe::file_sha256(&bwrap_path) {
        Ok(digest) => digest,
        Err(error) => {
            eprintln!(
                "galla-runner: cannot inspect installed backend `{}`: {error}; repair its host installation and retry the affected command",
                bwrap_path.display()
            );
            return ExitCode::from(3);
        }
    };
    if request.identities.backend.digest != actual_backend_digest {
        eprintln!(
            "galla-runner: backend identity / digest mismatch before execution: expected `{}`, observed `{}`. No payload ran. Refresh the request's enforcer identity against the installed backend and retry only this command; a host upgrade is not a workspace contract violation",
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
        eprintln!("galla-runner: {error}");
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
                eprintln!("galla-runner: required package-source proxy unavailable: {error}");
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
                        "galla-runner: System grant `{}` requires a non-link directory",
                        grant.source
                    );
                    return ExitCode::from(3);
                }
                Err(error) => {
                    eprintln!(
                        "galla-runner: cannot inspect System grant `{}`: {error}",
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
            eprintln!("galla-runner: {error}");
            return ExitCode::from(3);
        }
    };
    let _host_code_guard = match host_code::protect(&bwrap_path, &request.grants, &sources) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("galla-runner: {error}");
            return ExitCode::from(3);
        }
    };
    for source in sources.iter().flatten() {
        if let Err(error) = nix::fcntl::fcntl(
            source,
            nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::empty()),
        ) {
            eprintln!("galla-runner: cannot pass authorized mount origin to backend: {error}");
            return ExitCode::from(3);
        }
    }
    // Final origin descriptors are consumed/closed by Bubblewrap before exec.
    for (grant, source) in request.grants.iter().zip(&sources) {
        if crate::validation::is_system_grant(grant) {
            continue;
        }
        let Some(source) = source else {
            eprintln!("galla-runner: non-system grant has no authorized source descriptor");
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
            eprintln!("galla-runner: {error}");
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
            eprintln!("galla-runner: {error}");
            return ExitCode::from(3);
        }
    };
    if let Some(origin) = &_vendor_origin
        && let Err(error) = nix::fcntl::fcntl(
            origin,
            nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::empty()),
        )
    {
        eprintln!("galla-runner: cannot pass read-only vendor origin to backend: {error}");
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
    let mut child_cmd = Command::new("/usr/bin/prlimit");
    child_cmd.arg(format!("--nofile={}", request.resources.max_files));
    if let Some(maximum) = request.resources.max_file_bytes {
        child_cmd.arg(format!("--fsize={maximum}"));
    }
    child_cmd.arg("--").arg(&bwrap_path);
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
                    "galla-runner: cannot start process-local limits and Bubblewrap: {error}; install /usr/bin/prlimit and the configured backend on the host"
                );
                return ExitCode::from(3);
            }
        }
    };

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
                let mut host_stdout = host_stdout.lock();
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
                    if host_stdout.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = host_stdout.flush();
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
                let mut host_stderr = host_stderr.lock();
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
                    if host_stderr.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = host_stderr.flush();
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
        eprintln!("galla-runner: {reason}");
        return ExitCode::from(3);
    }

    if timeout_breached {
        eprintln!("galla-runner: wall time limit exceeded");
        return ExitCode::from(3);
    }

    if limit_exceeded.load(Ordering::SeqCst) {
        eprintln!("galla-runner: output limit exceeded");
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
        eprintln!("galla-runner: {promotion_error}");
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
        assert!(parse_request_line("POST http://host/path HTTP/1.1").is_err());
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
