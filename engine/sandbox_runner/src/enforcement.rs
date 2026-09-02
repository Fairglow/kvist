#![forbid(unsafe_code)]
//! Linux Bubblewrap sandbox enforcement implementation.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::probe::locate_backend;
use crate::protocol::{NetworkMode, SandboxRequest};

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
    for path in ["/lib", "/lib64", "/bin", "/sbin"] {
        let host_path = Path::new(path);
        if host_path.exists() {
            if let Ok(metadata) = std::fs::symlink_metadata(host_path) {
                if metadata.file_type().is_symlink() {
                    if let Ok(target) = std::fs::read_link(host_path) {
                        arguments.push("--symlink".to_owned());
                        arguments.push(target.to_string_lossy().into_owned());
                        arguments.push(path.to_owned());
                        continue;
                    }
                }
            }
            arguments.push("--ro-bind-try".to_owned());
            arguments.push(path.to_owned());
            arguments.push(path.to_owned());
        }
    }
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

    let mut bwrap_args = Vec::new();

    // 1. Clear ambient host environment variables
    bwrap_args.push("--clearenv".to_owned());

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
    if request.network.mode == NetworkMode::PackageSources {
        for path in ["/etc/resolv.conf", "/etc/hosts"] {
            if Path::new(path).exists() {
                bwrap_args.push("--ro-bind-try".to_owned());
                bwrap_args.push(path.to_owned());
                bwrap_args.push(path.to_owned());
            }
        }
    }

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

    while start_time.elapsed() < timeout {
        if let Ok(Some(status)) = child.try_wait() {
            exit_status = Some(status);
            break;
        }
        if limit_exceeded.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    if exit_status.is_none() && !limit_exceeded.load(Ordering::SeqCst) {
        timeout_breached = true;
        let pgid = nix::unistd::Pid::from_raw(-(child_pid as i32));
        let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGKILL);
    }

    // Wait and reap the child process
    let status = child.wait().ok();

    // Ensure reading threads are completely drained and finished
    stdout_thread.join().ok();
    stderr_thread.join().ok();

    if timeout_breached {
        eprintln!("kvist-sandbox-runner: wall time limit exceeded");
        return ExitCode::from(3);
    }

    if limit_exceeded.load(Ordering::SeqCst) {
        eprintln!("kvist-sandbox-runner: output limit exceeded");
        return ExitCode::from(3);
    }

    let final_status = exit_status.or(status);
    let exit_code = if let Some(s) = final_status {
        if s.success() {
            0
        } else {
            s.code().unwrap_or(1)
        }
    } else {
        1
    };

    ExitCode::from(exit_code as u8)
}
