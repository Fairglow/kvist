//! Per-call protection of host-executed isolation code, not a content freeze.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use nix::fcntl::{FcntlArg, OFlag, fcntl, open};
use nix::sys::signal::{Signal, killpg};
use nix::sys::stat::{Mode, SFlag, fstat, makedev};
use nix::sys::wait::{Id, WaitPidFlag, WaitStatus, waitid};
use nix::unistd::Pid;

use crate::protocol::{Access, Grant};

const MAX_METADATA: usize = 1_048_576;
const MAX_TRACE_ERRORS: usize = 65_536;
const TRACE_DEADLINE: Duration = Duration::from_secs(3);
const MAX_SELECTOR_SLOTS: usize = 16_384;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
}

struct Origin {
    path: PathBuf,
    identity: Option<Identity>,
    _fd: Option<OwnedFd>,
}

#[derive(Default)]
pub(super) struct Guard {
    origins: Vec<Origin>,
}

fn regular_identity(fd: &OwnedFd) -> Result<Identity, String> {
    let stat =
        fstat(fd).map_err(|error| format!("cannot inspect host-code descriptor: {error}"))?;
    if SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT != SFlag::S_IFREG {
        return Err("host enforcer code must be a regular file; repair its installation".into());
    }
    Ok(Identity {
        device: stat.st_dev,
        inode: stat.st_ino,
    })
}

fn code_fd(path: &Path) -> Result<OwnedFd, String> {
    open(path, OFlag::O_PATH | OFlag::O_CLOEXEC, Mode::empty()).map_err(|error| {
        format!(
            "cannot retain host enforcer `{}`: {error}; repair the installed runner/backend/prlimit or its loader dependencies before retrying the affected command",
            path.display()
        )
    })
}

impl Guard {
    fn running_image(&mut self) -> Result<(), String> {
        // Follow only the kernel executable reference, including an unlinked image.
        let fd = code_fd(Path::new("/proc/self/exe"))?;
        let identity = regular_identity(&fd)?;
        let kernel_path = fs::read_link("/proc/self/exe")
            .map_err(|error| format!("cannot locate the kernel running image: {error}"))?;
        if !kernel_path.is_absolute() {
            return Err(
                "kernel running image has no absolute origin; repair the runner installation"
                    .into(),
            );
        }
        self.origins.push(Origin {
            path: kernel_path.clone(),
            identity: Some(identity),
            _fd: Some(fd),
        });
        self.optional(&kernel_path)?;
        if let Some(original) = kernel_path
            .as_os_str()
            .as_bytes()
            .strip_suffix(b" (deleted)")
        {
            let original = PathBuf::from(std::ffi::OsString::from_vec(original.to_vec()));
            self.optional(&original)?;
        }
        Ok(())
    }

    fn optional(&mut self, path: &Path) -> Result<(), String> {
        self.origins.push(Origin {
            path: path.to_owned(),
            identity: None,
            _fd: None,
        });
        match path.canonicalize() {
            Ok(path) => self.installed(&path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "cannot resolve host-code origin `{}`: {error}; repair its installation before retrying the affected command",
                path.display()
            )),
        }
    }

    fn installed(&mut self, path: &Path) -> Result<(), String> {
        let canonical = path.canonicalize().map_err(|error| {
            format!(
                "cannot resolve host enforcer `{}`: {error}; repair the installed runner/backend/prlimit and loader dependencies before retrying the affected command",
                path.display()
            )
        })?;
        let fd = code_fd(&canonical)?;
        let identity = regular_identity(&fd)?;
        self.origins.push(Origin {
            path: path.to_owned(),
            identity: Some(identity),
            _fd: None,
        });
        self.origins.push(Origin {
            path: canonical,
            identity: Some(identity),
            _fd: Some(fd),
        });
        Ok(())
    }

    fn dependencies(&mut self, path: &Path) -> Result<(), String> {
        let canonical = path.canonicalize().map_err(|error| {
            format!(
                "cannot resolve host enforcer `{}`: {error}; repair its installation",
                path.display()
            )
        })?;
        let fd = code_fd(&canonical)?;
        let image = File::open(format!("/proc/self/fd/{}", fd.as_raw_fd()))
            .map_err(|error| format!("cannot inspect host enforcer ELF image: {error}"))?;
        let Some(interpreter) = elf_interpreter(image).map_err(|error| {
            format!("cannot inspect host enforcer `{}`: {error}; install a supported native ELF launcher", canonical.display())
        })? else {
            // A static ELF image has no runtime loader/dependency closure to trace.
            return Ok(());
        };
        self.installed(&interpreter)?;
        let interpreter = interpreter.canonicalize().map_err(|error| {
            format!("cannot resolve host enforcer loader: {error}; repair the native loader installation")
        })?;
        let mut command = Command::new(&canonical);
        command
            .env_clear()
            .env("LD_TRACE_LOADED_OBJECTS", "1")
            .arg("--version");
        let output = capture(&mut command, TRACE_DEADLINE)?;
        if !output.status.success() || !output.stderr.is_empty() {
            return Err(format!(
                "host enforcer `{}` did not produce a clean native loader trace (status {}); repair its dependencies/loader or install a trace-capable non-setuid launcher outside writable scopes",
                canonical.display(),
                output.status
            ));
        }
        let paths = trace_paths(&output.stdout).map_err(|error| {
            format!("cannot inspect host enforcer `{}` dependency closure: {error}; repair its loader/dependencies or install a supported native launcher outside writable scopes", canonical.display())
        })?;
        if !paths.contains(&interpreter) {
            return Err(format!(
                "host enforcer `{}` trace omitted its ELF interpreter; install a trace-capable native launcher outside writable scopes",
                canonical.display()
            ));
        }
        for path in paths {
            self.installed(&path)?;
        }
        Ok(())
    }

    fn mappings(&mut self) -> Result<(), String> {
        let file = File::open("/proc/self/maps")
            .map_err(|error| format!("cannot inspect active host-code mappings: {error}"))?;
        let mut bytes = Vec::new();
        file.take((MAX_METADATA + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("cannot read active host-code mappings: {error}"))?;
        if bytes.len() > MAX_METADATA {
            return Err("active host-code mappings exceed the bounded metadata budget; narrow the host launcher environment before retrying".into());
        }
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            self.mapping(line)?;
        }
        Ok(())
    }

    fn mapping(&mut self, line: &[u8]) -> Result<(), String> {
        let mut fields = line.splitn(6, |byte| *byte == b' ');
        let _address = fields.next();
        let permissions = fields
            .next()
            .ok_or("malformed active host-code mapping permissions")?;
        let _offset = fields
            .next()
            .ok_or("malformed active host-code mapping offset")?;
        let device = fields
            .next()
            .ok_or("malformed active host-code mapping device")?;
        let inode = fields
            .next()
            .ok_or("malformed active host-code mapping inode")?;
        let Some(path) = fields.next() else {
            return Ok(());
        };
        let path = path.trim_ascii_start();
        if !permissions.contains(&b'x') || !path.starts_with(b"/") {
            return Ok(());
        }
        let device = std::str::from_utf8(device).map_err(|_| "invalid mapping device")?;
        let (major, minor) = device.split_once(':').ok_or("invalid mapping device")?;
        let major = u64::from_str_radix(major, 16).map_err(|_| "invalid mapping device")?;
        let minor = u64::from_str_radix(minor, 16).map_err(|_| "invalid mapping device")?;
        let inode = std::str::from_utf8(inode)
            .map_err(|_| "invalid mapping inode")?
            .parse()
            .map_err(|_| "invalid mapping inode")?;
        let path = path.strip_suffix(b" (deleted)").unwrap_or(path);
        // proc maps escapes newlines, while ordinary spaces remain literal.
        let mut decoded = Vec::with_capacity(path.len());
        let mut index = 0;
        while index < path.len() {
            if path[index..].starts_with(b"\\012") {
                decoded.push(b'\n');
                index += 4;
            } else {
                decoded.push(path[index]);
                index += 1;
            }
        }
        let path = PathBuf::from(std::ffi::OsString::from_vec(decoded));
        self.origins.push(Origin {
            path: path.clone(),
            identity: Some(Identity {
                device: makedev(major, minor),
                inode,
            }),
            _fd: None,
        });
        self.optional(&path)
    }

    fn check(&self, grants: &[Grant], sources: &[Option<OwnedFd>]) -> Result<(), String> {
        if grants.len() != sources.len() {
            return Err("host-code protection requires one pinned origin per grant".into());
        }
        if !grants.iter().any(|grant| grant.access == Access::ReadWrite) {
            return Ok(());
        }
        let selectors = self.selector_slots()?;
        for (grant, source) in grants
            .iter()
            .zip(sources)
            .filter(|(grant, _)| grant.access == Access::ReadWrite)
        {
            let source = source
                .as_ref()
                .ok_or("writable grant has no pinned origin for host-code protection")?;
            let stat = fstat(source)
                .map_err(|error| format!("cannot inspect writable mount origin: {error}"))?;
            let identity = (SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT
                == SFlag::S_IFREG)
                .then_some(Identity {
                    device: stat.st_dev,
                    inode: stat.st_ino,
                });
            let conflict = self
                .origins
                .iter()
                .find(|origin| {
                    origin.path.starts_with(&grant.source)
                        || identity.is_some_and(|identity| origin.identity == Some(identity))
                })
                .map(|origin| &origin.path)
                .or_else(|| {
                    selectors
                        .iter()
                        .find(|slot| slot.starts_with(&grant.source))
                });
            if let Some(path) = conflict {
                return Err(format!(
                    "writable source `{}` includes or directly aliases host enforcer code/selector `{}`; choose a narrower writable scope or install enforcers outside it. Host updates remain allowed; payload edits must not alter subsequently host-executed isolation code",
                    grant.source,
                    path.display()
                ));
            }
        }
        Ok(())
    }

    fn selector_slots(&self) -> Result<BTreeSet<PathBuf>, String> {
        let mut pending = self
            .origins
            .iter()
            .map(|origin| origin.path.clone())
            .collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        let mut slots = BTreeSet::new();
        while let Some(path) = pending.pop() {
            for node in path.ancestors() {
                if !visited.insert(node.to_owned()) {
                    break;
                }
                if visited.len() > MAX_SELECTOR_SLOTS {
                    return Err("known host-code ancestor selectors exceed the bounded inspection budget; narrow the launcher topology or install enforcers outside writable scopes".into());
                }
                let (Some(parent), Some(name)) = (node.parent(), node.file_name()) else {
                    continue;
                };
                slots.insert(node.to_owned());
                let parent = match parent.canonicalize() {
                    Ok(parent) => parent,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(format!(
                            "cannot resolve host-code selector parent `{}`: {error}; repair its installation before retrying the affected command",
                            parent.display()
                        ));
                    }
                };
                // Resolve the parent, not the final slot: its link may lead back outside a grant.
                let slot = parent.join(name);
                slots.insert(slot.clone());
                if slots.len() > MAX_SELECTOR_SLOTS {
                    return Err("known host-code selector slots exceed the bounded inspection budget; narrow the launcher topology or install enforcers outside writable scopes".into());
                }
                match fs::read_link(&slot) {
                    Ok(target) => {
                        // Target ancestors can hide additional writable selectors behind another alias.
                        pending.push(if target.is_absolute() {
                            target
                        } else {
                            parent.join(target)
                        });
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotFound
                        ) => {}
                    Err(error) => {
                        return Err(format!(
                            "cannot inspect host-code selector `{}`: {error}; repair its installation before retrying the affected command",
                            slot.display()
                        ));
                    }
                }
            }
        }
        Ok(slots)
    }
}

// This is a bounded ELF program-header reader, not a dynamic-linker reimplementation.
fn elf_interpreter(mut file: File) -> Result<Option<PathBuf>, String> {
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    let mut header = [0u8; 64];
    file.read_exact(&mut header[..16])
        .map_err(|error| error.to_string())?;
    if &header[..4] != b"\x7fELF" || header[6] != 1 {
        return Err("not a supported ELF image".into());
    }
    let (header_size, entry_size, offset_start, size_start, count_start) = match header[4] {
        1 => (52, 32, 28, 42, 44),
        2 => (64, 56, 32, 54, 56),
        _ => return Err("unsupported ELF class".into()),
    };
    let little = match header[5] {
        1 => true,
        2 => false,
        _ => return Err("unsupported ELF byte order".into()),
    };
    file.read_exact(&mut header[16..header_size])
        .map_err(|error| error.to_string())?;
    let number = |bytes: &[u8]| -> u64 {
        if little {
            bytes
                .iter()
                .rev()
                .fold(0, |value, byte| (value << 8) | u64::from(*byte))
        } else {
            bytes
                .iter()
                .fold(0, |value, byte| (value << 8) | u64::from(*byte))
        }
    };
    if !matches!(number(&header[16..18]), 2 | 3) {
        return Err("ELF is not an executable image".into());
    }
    let offset_width = if header[4] == 1 { 4 } else { 8 };
    let offset = number(&header[offset_start..offset_start + offset_width]);
    let size = number(&header[size_start..size_start + 2]);
    let count = number(&header[count_start..count_start + 2]);
    if size != entry_size
        || count == 0
        || count > 1024
        || offset
            .checked_add(size * count)
            .is_none_or(|end| end > length)
    {
        return Err("ELF program headers exceed supported bounds".into());
    }
    let mut interpreter = None;
    for index in 0..count {
        file.seek(SeekFrom::Start(offset + index * size))
            .map_err(|error| error.to_string())?;
        let mut entry = [0u8; 56];
        file.read_exact(&mut entry[..size as usize])
            .map_err(|error| error.to_string())?;
        if number(&entry[..4]) != 3 {
            continue;
        }
        if interpreter.is_some() {
            return Err("ELF has multiple interpreters".into());
        }
        let (start, length) = if header[4] == 1 {
            (number(&entry[4..8]), number(&entry[16..20]))
        } else {
            (number(&entry[8..16]), number(&entry[32..40]))
        };
        if !(2..=4096).contains(&length) {
            return Err("ELF interpreter exceeds supported bounds".into());
        }
        file.seek(SeekFrom::Start(start))
            .map_err(|error| error.to_string())?;
        let mut bytes = vec![0; length as usize];
        file.read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.pop() != Some(0) || bytes.contains(&0) || !bytes.starts_with(b"/") {
            return Err("ELF interpreter is not one absolute path".into());
        }
        interpreter = Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)));
    }
    Ok(interpreter)
}

fn trace_paths(output: &[u8]) -> Result<Vec<PathBuf>, String> {
    if output.len() > MAX_METADATA {
        return Err("loader trace exceeds the metadata budget".into());
    }
    let text = std::str::from_utf8(output).map_err(|_| "loader trace is not UTF-8")?;
    let mut paths = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let (object, address) = line
            .rsplit_once(" (0x")
            .ok_or("unsupported or unresolved loader trace entry")?;
        let address = address
            .strip_suffix(')')
            .ok_or("malformed loader trace address")?;
        if address.is_empty() || !address.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("malformed loader trace address".into());
        }
        let path = object.split_once(" => ").map_or(object, |(_, path)| path);
        if !path.starts_with('/') {
            if object == "linux-vdso.so.1" || object == "linux-gate.so.1" {
                continue;
            }
            return Err("unresolved or unsupported loader trace entry".into());
        }
        let lookup = PathBuf::from(path);
        let path = lookup
            .canonicalize()
            .map_err(|_| "a resolved loader dependency is no longer available")?;
        // A writable directory containing a loader lookup symlink is code authority too.
        if !paths.contains(&lookup) {
            paths.push(lookup);
        }
        if !paths.contains(&path) {
            paths.push(path);
        }
        if paths.len() > 512 {
            return Err("loader dependency count exceeds the bounded closure budget".into());
        }
    }
    if paths.is_empty() {
        return Err("dynamic ELF produced no resolved loader dependency trace".into());
    }
    Ok(paths)
}

struct TraceOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn nonblocking<F: std::os::fd::AsFd>(fd: &F) -> Result<(), String> {
    let flags = fcntl(fd, FcntlArg::F_GETFL).map_err(|error| error.to_string())?;
    fcntl(
        fd,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn drain(pipe: &mut impl Read, bytes: &mut Vec<u8>, limit: usize) -> Result<bool, String> {
    let mut buffer = [0; 8192];
    // Yield to deadline/process supervision even if a producer never stops.
    for _ in 0..16 {
        match pipe.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(count) => {
                if count > limit.saturating_sub(bytes.len()) {
                    return Err("host loader trace exceeds the bounded stdout/stderr budget; repair the native launcher installation".into());
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("cannot read host loader trace: {error}")),
        }
    }
    Ok(false)
}

fn exited(child: &Child) -> Result<bool, String> {
    let pid =
        i32::try_from(child.id()).map_err(|_| "host loader trace process ID is out of range")?;
    // Leave the leader unreaped so its process-group ID cannot be recycled before cleanup.
    let status = waitid(
        Id::Pid(Pid::from_raw(pid)),
        WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
    )
    .map_err(|error| format!("cannot supervise host loader trace: {error}"))?;
    Ok(status != WaitStatus::StillAlive)
}

fn cleanup(child: &mut Child) -> Result<ExitStatus, String> {
    let pid =
        i32::try_from(child.id()).map_err(|_| "host loader trace process ID is out of range")?;
    match killpg(Pid::from_raw(pid), Signal::SIGKILL) {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "cannot clean up host loader trace process group: {error}"
            ));
        }
    }
    child
        .wait()
        .map_err(|error| format!("cannot reap host loader trace: {error}"))
}

fn capture(command: &mut Command, deadline: Duration) -> Result<TraceOutput, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn().map_err(|error| {
        format!("cannot start host loader dependency trace: {error}; repair the native launcher installation")
    })?;
    let result = (|| {
        let mut stdout = child
            .stdout
            .take()
            .ok_or("host loader trace stdout unavailable")?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or("host loader trace stderr unavailable")?;
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        let start = Instant::now();
        let mut out = Vec::new();
        let mut err = Vec::new();
        loop {
            if start.elapsed() >= deadline {
                return Err("host loader dependency trace exceeded its deadline; repair the native launcher/loader installation before retrying".into());
            }
            let out_done = drain(&mut stdout, &mut out, MAX_METADATA)?;
            let err_done = drain(&mut stderr, &mut err, MAX_TRACE_ERRORS)?;
            if out_done && err_done && exited(&child)? {
                return Ok((out, err));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    })();
    let status = cleanup(&mut child)?;
    result.map(|(stdout, stderr)| TraceOutput {
        status,
        stdout,
        stderr,
    })
}

/// Retain these close-on-exec guards until sandbox execution has completed.
pub(super) fn protect(
    backend: &Path,
    grants: &[Grant],
    sources: &[Option<OwnedFd>],
) -> Result<Guard, String> {
    let mut guard = Guard::default();
    guard.running_image()?;
    guard.installed(backend)?;
    guard.installed(Path::new("/usr/bin/prlimit"))?;
    guard.dependencies(backend)?;
    guard.dependencies(Path::new("/usr/bin/prlimit"))?;
    guard.mappings()?;
    guard.optional(Path::new("/etc/ld.so.cache"))?;
    guard.optional(Path::new("/etc/ld.so.preload"))?;
    guard.check(grants, sources)?;
    Ok(guard)
}

#[cfg(test)]
mod tests;
