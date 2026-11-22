#![forbid(unsafe_code)]
//! Command boundary for the Kvist sandbox runner.
//!
//! The executable strictly parses and validates the redefined version-one
//! protocol. It rejects superseded ("legacy"), malformed, oversized, or
//! unknown requests with an actionable diagnostic, and enforces valid requests
//! via Bubblewrap-backed Linux isolation. It never falls back to unconstrained host execution.

use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use galla::probe::{self, ProbeOutcome};
use galla::protocol::{MAX_REQUEST_BYTES, REQUEST_PROTOCOL};
use galla::session::{self, MAX_SESSION_BYTES};
use galla::validation;

mod context;

const PROBE_ARGUMENT: &str = "--kvist-sandbox-probe-v1";
const REQUEST_ARGUMENT: &str = "--kvist-sandbox-request-v1";
const SESSION_ARGUMENT: &str = "--kvist-sandbox-session-v1";
/// The explicit exec keyword. `galla exec <command> <args...>` is the spelled
/// form of the convenience exec mode; a bare `galla <command> <args...>` is
/// equivalent and is the primary form.
const EXEC_ARGUMENT: &str = "exec";
/// The opt-in toolchain root flags. Each flag takes a host directory and
/// mounts it read-only at the expected `$HOME`-relative location (or, for
/// Rust, at the `/rust/*` destinations), so the corresponding tool finds its
/// cache where a normal system would have it.
const RUST_ARGUMENT: &str = "--rust";
const GO_ARGUMENT: &str = "--go";
const NODE_ARGUMENT: &str = "--node";
const PYTHON_ARGUMENT: &str = "--python";

/// Splits the exec command line into the requested toolchain roots and the
/// command.
///
/// A leading `exec` keyword is stripped (it is the explicit form of the exec
/// mode). The `--rust`, `--go`, `--node`, and `--python` flags may each
/// appear anywhere before the command; when present, the following argument
/// is the host root and is removed from the command line. A flag without a
/// following argument is an error.
fn split_exec_command(
    command_line: Vec<std::ffi::OsString>,
) -> Result<(galla::exec::ToolchainRoots, Vec<std::ffi::OsString>), String> {
    // Strip a leading `exec` keyword, if present.
    let mut index = 0;
    if command_line.first().is_some_and(|arg| arg == EXEC_ARGUMENT) {
        index = 1;
    }
    let mut roots = galla::exec::ToolchainRoots::default();
    let mut command = Vec::with_capacity(command_line.len());
    while index < command_line.len() {
        let flag = command_line[index].as_os_str().to_str().unwrap_or("");
        let field = match flag {
            RUST_ARGUMENT => Some(&mut roots.rust),
            GO_ARGUMENT => Some(&mut roots.go),
            NODE_ARGUMENT => Some(&mut roots.node),
            PYTHON_ARGUMENT => Some(&mut roots.python),
            _ => None,
        };
        if let Some(field) = field {
            index += 1;
            let Some(root) = command_line.get(index) else {
                return Err(format!(
                    "the {flag} flag requires a root directory argument"
                ));
            };
            *field = Some(PathBuf::from(root));
        } else {
            command.push(command_line[index].clone());
        }
        index += 1;
    }
    Ok((roots, command))
}

fn main() -> ExitCode {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let Some(first) = arguments.first() else {
        eprintln!("galla: a protocol mode argument is required");
        return ExitCode::from(2);
    };

    // The three protocol modes are fixed tokens that take no further
    // arguments. Anything else is a command line for the exec mode.
    match first.as_os_str().to_str() {
        Some(PROBE_ARGUMENT) | Some(REQUEST_ARGUMENT) | Some(SESSION_ARGUMENT) => {
            if arguments.len() > 1 {
                eprintln!("galla: exactly one protocol mode argument is accepted");
                return ExitCode::from(2);
            }
            match first.as_os_str().to_str() {
                Some(PROBE_ARGUMENT) => run_probe(),
                Some(REQUEST_ARGUMENT) => run_request(),
                _ => run_session(),
            }
        }
        _ => {
            let (roots, command) = match split_exec_command(arguments) {
                Ok(pair) => pair,
                Err(reason) => {
                    eprintln!("galla: {reason}");
                    return ExitCode::from(2);
                }
            };
            run_exec(command, roots)
        }
    }
}

/// Runs the convenience exec mode: `galla <command> <args...>` (or the
/// explicit `galla exec <command> <args...>`).
///
/// The command line is resolved to an absolute host path, the current working
/// directory is mounted read-write at `/workspace`, and the standard system
/// tree is mounted read-only. The resulting authoring-phase request is
/// enforced through the same Bubblewrap-backed isolation as the one-shot
/// request mode. The no-nesting guard, backend identity, and host-code
/// protection all apply unchanged.
fn run_exec(command_line: Vec<std::ffi::OsString>, roots: galla::exec::ToolchainRoots) -> ExitCode {
    if let Err(reason) = context::require_host() {
        eprintln!("galla: {reason}");
        return ExitCode::from(3);
    }

    if command_line.is_empty() {
        eprintln!(
            "galla: exec mode requires a command; usage: galla [--rust <root>] [--go <root>] [--node <root>] [--python <root>] <command> <args...>"
        );
        return ExitCode::from(2);
    }

    let Some(bwrap_path) = galla::probe::locate_backend() else {
        eprintln!("galla: no Bubblewrap backend found on PATH or standard locations");
        return ExitCode::from(3);
    };

    // When a Rust root is mounted, the Rust bin directories are searched so
    // that `cargo`, `rustc`, etc. resolve from the host Rust installation.
    let extra_dirs = roots.extra_dirs();
    let argv = match galla::exec::resolve_argv(&command_line, &extra_dirs) {
        Ok(argv) => argv,
        Err(reason) => {
            eprintln!("galla: {reason}");
            return ExitCode::from(2);
        }
    };

    let workdir = match std::env::current_dir().and_then(|path| path.canonicalize()) {
        Ok(workdir) => workdir,
        Err(error) => {
            eprintln!("galla: cannot resolve the current working directory: {error}");
            return ExitCode::from(2);
        }
    };
    let runner_path = match std::fs::read_link("/proc/self/exe") {
        Ok(path) => path,
        Err(error) => {
            eprintln!("galla: cannot determine the runner binary path: {error}");
            return ExitCode::from(2);
        }
    };
    let runner_digest = match galla::probe::file_sha256(&runner_path) {
        Ok(digest) => digest,
        Err(error) => {
            eprintln!("galla: cannot compute the runner binary digest: {error}");
            return ExitCode::from(2);
        }
    };
    let backend_digest = match galla::probe::file_sha256(&bwrap_path) {
        Ok(digest) => digest,
        Err(error) => {
            eprintln!("galla: cannot compute the backend digest: {error}");
            return ExitCode::from(2);
        }
    };

    let mut request = match galla::exec::build_request_with_toolchains(
        &argv,
        &workdir,
        &bwrap_path,
        &backend_digest,
        &runner_digest,
        &roots,
    ) {
        Ok(request) => request,
        Err(reason) => {
            eprintln!("galla: {reason}");
            return ExitCode::from(2);
        }
    };

    let interactive =
        io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal();
    if interactive {
        let term = match std::env::var("TERM") {
            Ok(term) if !term.is_empty() => term,
            Ok(_) | Err(std::env::VarError::NotPresent) => "dumb".to_owned(),
            Err(std::env::VarError::NotUnicode(_)) => {
                eprintln!("galla: TERM must be valid UTF-8 for interactive exec");
                return ExitCode::from(2);
            }
        };
        request.environment.insert("TERM".to_owned(), term);
    }
    if let Err(error) = validation::validate(&request) {
        eprintln!("galla: {error}");
        return ExitCode::from(2);
    }

    if interactive {
        galla::enforcement::run_terminal(request)
    } else {
        galla::enforcement::run(request)
    }
}

fn run_probe() -> ExitCode {
    match probe::probe() {
        ProbeOutcome::Confirmed(response) => match serde_json::to_string(&response) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("galla: cannot serialize probe response: {error}");
                ExitCode::from(2)
            }
        },
        ProbeOutcome::Unavailable { reason } => {
            eprintln!("galla: {reason}");
            ExitCode::from(3)
        }
    }
}

fn run_request() -> ExitCode {
    if let Err(reason) = context::require_host() {
        eprintln!("galla: {reason}");
        return ExitCode::from(3);
    }

    let mut buffer = Vec::new();
    let mut handle = io::stdin().lock().take((MAX_REQUEST_BYTES as u64) + 1);
    if let Err(error) = handle.read_to_end(&mut buffer) {
        eprintln!("galla: cannot read the request from standard input: {error}");
        return ExitCode::from(2);
    }

    // Name a protocol mismatch before the strict parser rejects an unknown
    // session block. The session mode is a separate, explicit protocol mode
    // beside the one-shot request; feeding session launch metadata to request
    // mode must fail closed with the protocol mismatch named, not with an
    // opaque unknown-field diagnostic.
    if let Some(reason) = request_protocol_mismatch(&buffer) {
        eprintln!("galla: {reason}");
        return ExitCode::from(2);
    }

    match validation::parse_and_validate(&buffer) {
        Ok(request) => galla::enforcement::run(request),
        Err(error) => {
            eprintln!("galla: {error}");
            ExitCode::from(2)
        }
    }
}

/// Names a protocol mismatch for a one-shot request that carries a different
/// protocol identifier, without trusting the rest of the payload.
///
/// The strict parser rejects an unknown session block before the protocol
/// check can run, so this lightweight pre-check reads only the `protocol`
/// field. A missing field, a non-string value, or a non-object top level is
/// not a mismatch: the strict parser names those with its own diagnostic.
fn request_protocol_mismatch(buffer: &[u8]) -> Option<String> {
    // An oversized buffer is named by the strict parser as `TooLarge`; the
    // pre-check must not shadow that diagnostic with a protocol mismatch.
    if buffer.len() > MAX_REQUEST_BYTES {
        return None;
    }
    let value = serde_json::from_slice::<serde_json::Value>(buffer).ok()?;
    let protocol = value.get("protocol")?.as_str()?;
    (protocol != REQUEST_PROTOCOL)
        .then(|| format!("protocol must be `{REQUEST_PROTOCOL}`, not `{protocol}`"))
}

/// Runs the explicit, versioned session mode.
///
/// The session mode reads bounded launch metadata on a dedicated descriptor,
/// never through argv and never through standard input. The embedding host
/// (Skott) holds the session and cancellation capability and passes the
/// metadata on a file descriptor named by the `KVIST_SESSION_FD` environment
/// variable. When the descriptor is absent or unreadable, the runner fails
/// closed: it never falls back to one-shot behavior or host execution, and it
/// emits no payload stdout.
fn run_session() -> ExitCode {
    if let Err(reason) = context::require_host() {
        eprintln!("galla: {reason}");
        return ExitCode::from(3);
    }

    let Some(descriptor) = std::env::var("KVIST_SESSION_FD")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
    else {
        eprintln!(
            "galla: session mode requires the KVIST_SESSION_FD environment variable naming the dedicated launch-metadata descriptor"
        );
        return ExitCode::from(2);
    };

    // Open a new descriptor referring to the same open file description the
    // embedding host handed over. Reading through `/proc/self/fd` keeps the
    // runner free of unsafe code while still consuming the dedicated
    // descriptor rather than argv or standard input.
    let file = match std::fs::File::open(format!("/proc/self/fd/{descriptor}")) {
        Ok(file) => file,
        Err(error) => {
            eprintln!(
                "galla: cannot open the session launch-metadata descriptor {descriptor}: {error}"
            );
            return ExitCode::from(2);
        }
    };

    let mut handle = std::io::BufReader::new(file).take((MAX_SESSION_BYTES as u64) + 1);
    let mut buffer = Vec::new();
    if let Err(error) = handle.read_to_end(&mut buffer) {
        eprintln!(
            "galla: cannot read the session launch metadata from descriptor {descriptor}: {error}"
        );
        return ExitCode::from(2);
    }

    match session::parse_and_validate(&buffer) {
        Ok(launch) => galla::enforcement::run(launch.to_request()),
        Err(error) => {
            eprintln!("galla: {error}");
            ExitCode::from(2)
        }
    }
}
