//! Build script that determines version, commit SHA, and build date
//! for inclusion in the About dialog.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn git_bytes(arguments: &[&str], directory: Option<&Path>) -> Result<Option<Vec<u8>>, String> {
    let mut command = Command::new("git");
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("query Git build inputs: {error}")),
    };
    let mut bytes = Vec::new();
    let read = match child.stdout.take() {
        Some(stdout) => stdout.take((1 << 20) + 1).read_to_end(&mut bytes),
        None => Err(std::io::Error::other("Git stdout was not piped")),
    };
    if read.is_err() || bytes.len() > 1 << 20 {
        let killed = child.kill();
        let waited = child.wait();
        return Err(format!(
            "Git build-input inventory failed or exceeded 1 MiB; \
             narrow the tracked workspace: read={read:?}, kill={killed:?}, wait={waited:?}"
        ));
    }
    let status = child
        .wait()
        .map_err(|error| format!("wait for Git build inputs: {error}"))?;
    Ok(status.success().then_some(bytes))
}

fn git_path(arguments: &[&str]) -> Result<Option<PathBuf>, String> {
    let Some(bytes) = git_bytes(arguments, None)? else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "Git build-input paths must be UTF-8".to_owned())?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.contains(['\r', '\n', '\0']) || !Path::new(text).is_absolute() {
        return Err("Git build-input paths must be absolute and contain no line breaks".into());
    }
    Ok(Some(PathBuf::from(text)))
}

fn rebuild_inputs() -> Result<(), String> {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let mut inputs = BTreeSet::from([PathBuf::from("build.rs")]);
    if let Some(root) = git_path(&["rev-parse", "--show-toplevel"])? {
        for name in ["HEAD", "index", "refs", "packed-refs"] {
            let path = git_path(&["rev-parse", "--path-format=absolute", "--git-path", name])?
                .ok_or_else(|| format!("cannot resolve Git build input {name}"))?;
            match path.try_exists() {
                Ok(true) => {
                    inputs.insert(path);
                }
                Ok(false) => {}
                Err(error) => return Err(format!("inspect Git build input: {error}")),
            }
        }
        let files = git_bytes(&["ls-files", "--cached", "--full-name", "-z"], Some(&root))?
            .ok_or_else(|| "cannot enumerate tracked Git build inputs".to_owned())?;
        for file in files
            .split(|byte| *byte == 0)
            .filter(|file| !file.is_empty())
        {
            let file = std::str::from_utf8(file)
                .map_err(|_| "tracked Git build-input paths must be UTF-8".to_owned())?;
            if file.contains(['\r', '\n']) {
                return Err("tracked Git build-input paths must contain no line breaks".into());
            }
            let mut path = root.join(file);
            while !path
                .try_exists()
                .map_err(|error| format!("inspect tracked build input: {error}"))?
            {
                if !path.pop() {
                    return Err("tracked build input has no existing ancestor".into());
                }
            }
            inputs.insert(path);
        }
    }
    for path in inputs {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    Ok(())
}

fn get_git_sha() -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8(o.stdout)
            .unwrap_or_default()
            .trim()
            .to_owned(),
        _ => "unknown".to_owned(),
    }
}

fn get_git_dirty() -> bool {
    let output = Command::new("git")
        .args(["diff-index", "--quiet", "HEAD", "--"])
        .output();

    !matches!(output, Ok(o) if o.status.success())
}

fn get_build_date() -> String {
    // Use SOURCE_DATE_EPOCH if set (reproducible builds), otherwise use `date`
    if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH")
        && let Ok(secs) = epoch.parse::<i64>()
    {
        // Convert to ISO format using the `date` command
        let output = Command::new("date")
            .args(["-u", "-d", &format!("@{secs}"), "+%Y-%m-%d %H:%M:%S UTC"])
            .output();
        if let Ok(o) = output
            && o.status.success()
        {
            return String::from_utf8(o.stdout)
                .unwrap_or_default()
                .trim()
                .to_owned();
        }
    }

    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%d %H:%M:%S UTC"])
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8(o.stdout)
            .unwrap_or_default()
            .trim()
            .to_owned(),
        _ => "unknown".to_owned(),
    }
}

fn main() -> Result<(), String> {
    let sha = get_git_sha();
    let dirty = get_git_dirty();
    let date = get_build_date();

    println!("cargo:rustc-env=GIT_SHA={}", sha);
    println!(
        "cargo:rustc-env=GIT_DIRTY={}",
        if dirty { "true" } else { "false" }
    );
    println!("cargo:rustc-env=BUILD_DATE={}", date);

    rebuild_inputs()
}
