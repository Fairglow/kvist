//! Build script that determines version, commit SHA, and build date
//! for inclusion in the About dialog.

use std::process::Command;

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
    if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH") {
        if let Ok(secs) = epoch.parse::<i64>() {
            // Convert to ISO format using the `date` command
            let output = Command::new("date")
                .args(["-u", "-d", "@", &secs.to_string(), "+%Y-%m-%d %H:%M:%S UTC"])
                .output();
            if let Ok(o) = output {
                if o.status.success() {
                    return String::from_utf8(o.stdout)
                        .unwrap_or_default()
                        .trim()
                        .to_owned();
                }
            }
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

fn main() {
    let sha = get_git_sha();
    let dirty = get_git_dirty();
    let date = get_build_date();

    println!("cargo:rustc-env=GIT_SHA={}", sha);
    println!(
        "cargo:rustc-env=GIT_DIRTY={}",
        if dirty { "true" } else { "false" }
    );
    println!("cargo:rustc-env=BUILD_DATE={}", date);

    // Force rebuild on git changes
    println!("cargo:rerun-if-changed=.git/refs/heads");
}
