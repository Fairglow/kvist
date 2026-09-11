#![forbid(unsafe_code)]
//! Command boundary for the Kvist sandbox runner.
//!
//! The executable strictly parses and validates the redefined version-one
//! protocol. It rejects superseded ("legacy"), malformed, oversized, or
//! unknown requests with an actionable diagnostic, and enforces valid requests
//! via Bubblewrap-backed Linux isolation. It never falls back to unconstrained host execution.

use std::io::{self, Read};
use std::process::ExitCode;

use kvist_sandbox_runner::probe::{self, ProbeOutcome};
use kvist_sandbox_runner::protocol::MAX_REQUEST_BYTES;
use kvist_sandbox_runner::validation;

const PROBE_ARGUMENT: &str = "--kvist-sandbox-probe-v1";
const REQUEST_ARGUMENT: &str = "--kvist-sandbox-request-v1";

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let Some(mode) = arguments.next() else {
        eprintln!("kvist-sandbox-runner: a protocol mode argument is required");
        return ExitCode::from(2);
    };
    if arguments.next().is_some() {
        eprintln!("kvist-sandbox-runner: exactly one protocol mode argument is accepted");
        return ExitCode::from(2);
    }

    if mode == PROBE_ARGUMENT {
        run_probe()
    } else if mode == REQUEST_ARGUMENT {
        run_request()
    } else {
        eprintln!(
            "kvist-sandbox-runner: unknown argument; expected `{PROBE_ARGUMENT}` or `{REQUEST_ARGUMENT}`"
        );
        ExitCode::from(2)
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
                eprintln!("kvist-sandbox-runner: cannot serialize probe response: {error}");
                ExitCode::from(2)
            }
        },
        ProbeOutcome::Unavailable { reason } => {
            eprintln!("kvist-sandbox-runner: {reason}");
            ExitCode::from(3)
        }
    }
}

fn run_request() -> ExitCode {
    let mut buffer = Vec::new();
    let mut handle = io::stdin().lock().take((MAX_REQUEST_BYTES as u64) + 1);
    if let Err(error) = handle.read_to_end(&mut buffer) {
        eprintln!("kvist-sandbox-runner: cannot read the request from standard input: {error}");
        return ExitCode::from(2);
    }

    match validation::parse_and_validate(&buffer) {
        Ok(request) => kvist_sandbox_runner::enforcement::run(request),
        Err(error) => {
            eprintln!("kvist-sandbox-runner: {error}");
            ExitCode::from(2)
        }
    }
}
