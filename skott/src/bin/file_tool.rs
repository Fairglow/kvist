//! Native file primitive helper, not a standalone sandbox.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::process::ExitCode;

use skott::file_tools::{MAX_REQUEST_BYTES, execute_file_request, parse_file_request};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args
        .next()
        .ok_or("expected exactly one bounded JSON request file")?;
    if args.next().is_some() {
        return Err("expected exactly one bounded JSON request file".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(&path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_REQUEST_BYTES as u64 {
        return Err("request must be a regular non-link file within the byte bound".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let request = parse_file_request(&bytes)?;
    let outcome = execute_file_request(&request)?;
    let encoded = serde_json::to_vec(&outcome)?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&encoded)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("native file tool failed: {error}");
            ExitCode::from(2)
        }
    }
}
