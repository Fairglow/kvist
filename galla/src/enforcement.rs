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

use crate::origin::OriginPolicy;
use crate::probe::locate_backend;
use crate::protocol::{Access, Grant, NetworkMode, Phase, Purpose, SandboxRequest};

mod system_layout;

fn add_workspace_vendor_mask(
    arguments: &mut Vec<String>,
    phase: Phase,
    grants: &[Grant],
) -> Result<(), String> {
    if phase != Phase::Authoring
        || !grants.iter().any(|grant| {
            grant.destination == "/rust/vendor"
                && grant.access == Access::ReadOnly
                && grant.purpose == Purpose::Toolchain
        })
    {
        return Ok(());
    }
    let Some(workspace) = grants.iter().find(|grant| {
        grant.destination == "/workspace"
            && grant.access == Access::ReadWrite
            && grant.purpose == Purpose::Authoring
    }) else {
        return Ok(());
    };
    for relative in [".kvist", ".kvist/vendored"] {
        let source = Path::new(&workspace.source).join(relative);
        match std::fs::symlink_metadata(&source) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(format!(
                    "workspace vendor mask requires regular non-link directories: `{}`",
                    source.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(format!(
                    "cannot inspect workspace vendor mask path `{}`: {error}",
                    source.display()
                ));
            }
        }
    }
    // Mask only after the workspace bind; never chmod the host directory.
    arguments.extend(
        [
            "--perms",
            "000",
            "--tmpfs",
            "/workspace/.kvist/vendored",
            "--remount-ro",
            "/workspace/.kvist/vendored",
            "--cap-drop",
            "ALL",
        ]
        .map(str::to_owned),
    );
    Ok(())
}

/// The parsed form of a network-guard request line.
///
/// The guard accepts two request shapes: an absolute-form request URI (used for
/// plain HTTP, e.g. `GET http://host/path HTTP/1.1`) and the CONNECT method
/// used to tunnel HTTPS through the proxy (e.g. `CONNECT host:443 HTTP/1.1`).
enum ParsedRequestLine {
    /// An absolute-form request targeting a plain-`http` upstream.
    HttpAbsolute {
        /// The upstream host.
        host: String,
        /// The upstream port (defaults to 80 when absent).
        port: u16,
        /// The request path forwarded verbatim to the upstream.
        path: String,
    },
    /// A CONNECT tunnel request targeting an HTTPS upstream.
    ConnectTunnel {
        /// The upstream host.
        host: String,
        /// The upstream port (defaults to 443 when absent).
        port: u16,
    },
}

/// Parses the first line of an HTTP request received by the network guard.
///
/// Returns [`ParsedRequestLine::HttpAbsolute`] for an absolute-form request URI
/// (plain HTTP) and [`ParsedRequestLine::ConnectTunnel`] for a `CONNECT`
/// request line (HTTPS tunneling). Any other method, an empty authority, or a
/// malformed authority/port yields `Err`.
fn parse_request_line(first_line: &str) -> Result<ParsedRequestLine, ()> {
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(());
    }
    let method = parts[0];
    let target = parts[1];

    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = match target.split_once(':') {
            Some((host, port)) => {
                if host.is_empty() {
                    return Err(());
                }
                let Ok(port) = port.parse::<u16>() else {
                    return Err(());
                };
                validate_connect_host(host)?;
                (host.to_owned(), port)
            }
            // A bare `host` defaults to the HTTPS port.
            None => {
                validate_connect_host(target)?;
                (target.to_owned(), 443)
            }
        };
        return Ok(ParsedRequestLine::ConnectTunnel { host, port });
    }

    if (method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD"))
        && let Some(stripped) = target.strip_prefix("http://")
    {
        let (authority, path) = match stripped.split_once('/') {
            Some((auth, path)) => (auth, format!("/{}", path)),
            None => (stripped, "/".to_owned()),
        };
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) if !host.is_empty() => {
                let Ok(port) = port.parse::<u16>() else {
                    return Err(());
                };
                (host.to_owned(), port)
            }
            // A bare `host` defaults to the HTTP port.
            _ if !authority.is_empty() => (authority.to_owned(), 80),
            _ => return Err(()),
        };
        return Ok(ParsedRequestLine::HttpAbsolute { host, port, path });
    }

    Err(())
}

/// Validates a `CONNECT` authority host: a registered name or IP literal that
/// cannot contain path separators, userinfo, or characters that would let a
/// malformed host masquerade as a different origin.
fn validate_connect_host(host: &str) -> Result<(), ()> {
    if host.is_empty() {
        return Err(());
    }
    if host.contains(['/', '@', ' ']) {
        return Err(());
    }
    // A colon is only legal inside a bracketed IPv6 literal, which the
    // `split_once(':')` caller would have already rejected as malformed.
    if host.contains(':') {
        return Err(());
    }
    if !host
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err(());
    }
    if host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return Err(());
    }
    Ok(())
}

impl ParsedRequestLine {
    /// The policy target string the origin matcher authorizes against.
    ///
    /// CONNECT tunnels are reauthorized as canonical HTTPS origins that always
    /// carry an explicit port. Absolute-form HTTP requests are reauthorized as
    /// canonical HTTP origins whose default port (80) is omitted so the matcher
    /// sees the same canonical shape it validates elsewhere.
    fn policy_target(&self) -> String {
        match self {
            ParsedRequestLine::ConnectTunnel { host, port } => {
                format!("https://{host}:{port}/")
            }
            ParsedRequestLine::HttpAbsolute { host, port, path } => {
                let port_suffix = if *port == 80 {
                    String::new()
                } else {
                    format!(":{port}")
                };
                format!("http://{host}{port_suffix}{path}")
            }
        }
    }
}

fn start_network_guard_proxy(
    sources: &[crate::protocol::AllowedSource],
) -> Option<(u16, std::sync::mpsc::Sender<()>)> {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();

    let mut policy = OriginPolicy::new();
    for source in sources {
        match source {
            crate::protocol::AllowedSource::CargoRegistry {
                index_origin,
                download_origin,
                ..
            } => {
                if policy.allow_url(index_origin).is_ok() {
                    let _ = policy.allow_url(download_origin);
                }
            }
            crate::protocol::AllowedSource::CargoGit { repository, .. } => {
                let _ = policy.allow_url(repository);
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

            let mut buffer = [0u8; 4096];
            let count = match client.read(&mut buffer) {
                Ok(n) if n > 0 => n,
                _ => return,
            };
            let request_text = String::from_utf8_lossy(&buffer[..count]);
            let first_line = request_text.lines().next().unwrap_or("");
            let parsed = match parse_request_line(first_line) {
                Ok(parsed) => parsed,
                Err(()) => return,
            };

            let policy_target = parsed.policy_target();
            let authorized = match &parsed {
                ParsedRequestLine::HttpAbsolute { .. } => policy.permits(&policy_target),
                // A CONNECT tunnel forwards an encrypted stream whose request
                // path the proxy cannot inspect, so it is authorized on
                // authority alone (scheme, host, port) rather than path-prefix.
                ParsedRequestLine::ConnectTunnel { .. } => {
                    policy.authorizes_authority(&policy_target)
                }
            };

            if !authorized {
                let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 14\r\nConnection: close\r\n\r\n403 Forbidden\n");
                let _ = client.flush();
                return;
            }

            match parsed {
                ParsedRequestLine::ConnectTunnel { host, port } => {
                    // HTTPS tunnel: complete the CONNECT handshake, then
                    // bidirectionally forward the encrypted stream. When the
                    // upstream connection fails, reject the client with a clear
                    // error instead of silently dropping the connection.
                    if let Ok(mut upstream) = TcpStream::connect((host.as_str(), port)) {
                        let _ = client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n");
                        let _ = client.flush();
                        let mut sink = [0u8; 4096];
                        let mut source = [0u8; 4096];
                        loop {
                            let read = match upstream.read(&mut sink) {
                                Ok(0) => break,
                                Ok(n) => (n, true),
                                Err(_) => break,
                            };
                            if client.write_all(&sink[..read.0]).is_err() {
                                break;
                            }
                            let read = match client.read(&mut source) {
                                Ok(0) => break,
                                Ok(n) => (n, false),
                                Err(_) => break,
                            };
                            if upstream.write_all(&source[..read.0]).is_err() {
                                break;
                            }
                        }
                    } else {
                        let _ = client.write_all(
                            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 19\r\nConnection: close\r\n\r\n502 Bad Gateway\n",
                        );
                        let _ = client.flush();
                    }
                }
                ParsedRequestLine::HttpAbsolute { host, port, path } => {
                    if let Ok(mut upstream) = TcpStream::connect((host.as_str(), port)) {
                        let mut forward_header = format!("{} {} HTTP/1.1\r\n", "GET", path);
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
            }
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
/// Note: `--nproc` is intentionally omitted here because Linux RLIMIT_NPROC applies
/// across the entire real host UID (breaking fork for the user in multi-process/CI environments);
/// process count is actively supervised in the process tree monitor via `count_processes_in_pgid`.
fn safe_prlimit(child_pid: u32, limits: crate::protocol::Resources) {
    let pid_str = child_pid.to_string();
    let nofile_str = format!("--nofile={}", limits.max_files);
    let fsize_str = format!("--fsize={}", limits.max_file_bytes);

    let _ = Command::new("/usr/bin/prlimit")
        .arg("--pid")
        .arg(&pid_str)
        .arg(&nofile_str)
        .arg(&fsize_str)
        .output();
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
    if let Err(reason) = crate::context::require_host() {
        eprintln!("galla-runner: {reason}");
        return ExitCode::from(3);
    }
    let Some(bwrap_path) = locate_backend() else {
        eprintln!("galla-runner: no Bubblewrap backend found on PATH or standard locations");
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
            "galla-runner: backend identity / digest mismatch: expected `{}`, observed `{}`",
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
    bwrap_args.push("--tmpfs".to_owned());
    bwrap_args.push("/tmp".to_owned());
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
        if crate::validation::is_filtered_system_grant(grant) {
            match std::fs::symlink_metadata(&grant.source) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) => {
                    eprintln!(
                        "galla-runner: filtered System grant `{}` requires a non-link directory",
                        grant.source
                    );
                    return ExitCode::from(3);
                }
                Err(error) => {
                    eprintln!(
                        "galla-runner: cannot inspect filtered System grant `{}`: {error}",
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

    // 8. Bind all grant source and destination paths
    for grant in &request.grants {
        if crate::validation::is_filtered_system_grant(grant) {
            continue;
        }
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
    if let Err(error) = add_workspace_vendor_mask(&mut bwrap_args, request.phase, &request.grants) {
        eprintln!("galla-runner: {error}");
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
                eprintln!("galla-runner: failed to spawn Bubblewrap process: {error}");
                return ExitCode::from(3);
            }
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
        && let Err(promotion_error) =
            inspect_and_promote_cache(&request, &project_cache_dir, &before_snapshot)
    {
        eprintln!("galla-runner: {promotion_error}");
        exit_code = 3;
    }

    ExitCode::from(exit_code as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::origin::{OriginPolicy, RequestKind};
    use std::fs;
    use std::os::unix::fs::symlink;

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
    fn mask_requires_existing_registry_and_exact_prepared_authoring_topology() {
        let workspace = tempfile::tempdir().unwrap();
        let mut grants = grants(workspace.path());
        let mut arguments = Vec::new();
        add_workspace_vendor_mask(&mut arguments, Phase::Authoring, &grants).unwrap();
        assert!(arguments.is_empty());
        assert!(!workspace.path().join(".kvist").exists());
        fs::create_dir(workspace.path().join(".kvist")).unwrap();
        add_workspace_vendor_mask(&mut arguments, Phase::Authoring, &grants).unwrap();
        assert!(arguments.is_empty());
        assert!(!workspace.path().join(".kvist/vendored").exists());
        fs::create_dir(workspace.path().join(".kvist/vendored")).unwrap();
        add_workspace_vendor_mask(&mut arguments, Phase::Verification, &grants).unwrap();
        assert!(arguments.is_empty());
        grants[1].destination = "/rust/other".into();
        add_workspace_vendor_mask(&mut arguments, Phase::Authoring, &grants).unwrap();
        assert!(arguments.is_empty());
        grants[1].destination = "/rust/vendor".into();
        for (index, access, purpose, destination) in [
            (1, Access::ReadWrite, Purpose::Toolchain, "/rust/vendor"),
            (1, Access::ReadOnly, Purpose::Registry, "/rust/vendor"),
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
            add_workspace_vendor_mask(&mut arguments, Phase::Authoring, &unrelated).unwrap();
            assert!(arguments.is_empty());
        }
        arguments.extend([
            "--bind".into(),
            grants[0].source.clone(),
            "/workspace".into(),
        ]);
        add_workspace_vendor_mask(&mut arguments, Phase::Authoring, &grants).unwrap();
        assert_eq!(
            &arguments[3..],
            [
                "--perms",
                "000",
                "--tmpfs",
                "/workspace/.kvist/vendored",
                "--remount-ro",
                "/workspace/.kvist/vendored",
                "--cap-drop",
                "ALL",
            ]
        );
    }

    #[test]
    fn mask_rejects_links_and_non_directory_paths() {
        for relative in [".kvist", ".kvist/vendored"] {
            let workspace = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            if relative != ".kvist" {
                fs::create_dir(workspace.path().join(".kvist")).unwrap();
            }
            let path = workspace.path().join(relative);
            symlink(outside.path(), &path).unwrap();
            let mut arguments = Vec::new();
            assert!(
                add_workspace_vendor_mask(
                    &mut arguments,
                    Phase::Authoring,
                    &grants(workspace.path()),
                )
                .is_err()
            );
            assert!(arguments.is_empty());
            fs::remove_file(&path).unwrap();
            fs::write(&path, "not a directory").unwrap();
            assert!(
                add_workspace_vendor_mask(
                    &mut arguments,
                    Phase::Authoring,
                    &grants(workspace.path()),
                )
                .is_err()
            );
            assert!(arguments.is_empty());
        }
    }

    // -- Network guard HTTPS CONNECT handling and URL parsing -----------------

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
        // The same authority with a different port is denied.
        assert!(!policy.authorizes_authority("https://static.crates.io:8443/"));
    }

    // -- Full URL parsing through the validated origin matcher ------------------

    #[test]
    fn connect_tunnel_authorizes_by_full_url_parsing() {
        // The guard reauthorizes a CONNECT tunnel through the strict origin
        // parser, not a naive prefix match: scheme, host, port, and path
        // components are all validated.
        let mut policy = OriginPolicy::new();
        policy
            .allow_url("https://index.crates.io/")
            .expect("allow index");

        // A CONNECT tunnel to the allowed authority is authorized even though
        // the CONNECT target carries no path.
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
