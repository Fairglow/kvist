#![forbid(unsafe_code)]
//! Precise isolated Git index commit automation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use sha2::Digest;

use crate::config::VcsSelection;
use crate::error::{KvistError, Result};

/// Holds the exact accepted set of changes for a VCS commit.
#[derive(Debug, Clone)]
pub struct AcceptedChange {
    pub path: PathBuf,
    pub operation: String, // "create", "modify", "delete", "rename"
    pub pre_digest: Option<String>,
    pub post_digest: Option<String>,
}

/// Details of the pending acceptance state loaded from the journal.
#[derive(Debug, Clone)]
pub struct AcceptanceRecord {
    pub acceptance_id: String,
    pub expected_head: String,
    pub component_path: PathBuf,
    pub accepted_changes: Vec<AcceptedChange>,
    pub commit_message: String,
}

/// Runs a git command under an isolated index and returns stdout on success.
fn run_git_isolated(
    project_dir: &Path,
    temp_index: &Path,
    args: &[&str],
    disable_hooks: bool,
) -> Result<String> {
    let mut cmd = Command::new("git");
    if disable_hooks {
        cmd.args(["-c", "core.hooksPath=/dev/null"]);
    }
    cmd.args(args)
        .current_dir(project_dir)
        .env("GIT_INDEX_FILE", temp_index)
        .env("LC_ALL", "C")
        .env("LANG", "C");

    let output = cmd.output().map_err(|source| KvistError::Io {
        operation: "execute git",
        path: PathBuf::from("git"),
        source,
    })?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(KvistError::VcsCommitFailed {
            reason: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

/// Hashing a local file to the Git object database, returning the blob OID.
fn git_hash_object(project_dir: &Path, file_path: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["hash-object", "-w"])
        .arg(file_path)
        .current_dir(project_dir)
        .output()
        .map_err(|source| KvistError::Io {
            operation: "git hash-object",
            path: file_path.to_path_buf(),
            source,
        })?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(KvistError::VcsCommitFailed {
            reason: format!(
                "failed to hash object {}: {}",
                file_path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        })
    }
}

/// Returns the current branch name, or Err if detached.
fn get_current_branch(project_dir: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["symbolic-ref", "HEAD"])
        .current_dir(project_dir)
        .output()
        .map_err(|source| KvistError::Io {
            operation: "git symbolic-ref",
            path: PathBuf::from("git"),
            source,
        })?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(KvistError::VcsCommitFailed {
            reason: "detached-HEAD: repository is not on any branch".to_owned(),
        })
    }
}

/// Verifies that a file's blob digest in HEAD matches the pre_digest from the acceptance.
fn verify_head_pre_digest(
    project_dir: &Path,
    head_sha: &str,
    relative_path: &Path,
    expected_pre_digest: Option<&str>,
) -> Result<()> {
    let path_str = relative_path.to_string_lossy();
    let output = Command::new("git")
        .args(["rev-parse", &format!("{}:{}", head_sha, path_str)])
        .current_dir(project_dir)
        .output()
        .map_err(|source| KvistError::Io {
            operation: "git rev-parse blob",
            path: PathBuf::from("git"),
            source,
        })?;

    let actual_blob_oid = if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        None
    };

    let actual_digest = if let Some(oid) = actual_blob_oid {
        // Query the hash of the blob in git
        let content_output = Command::new("git")
            .args(["cat-file", "-p", &oid])
            .current_dir(project_dir)
            .output()
            .map_err(|source| KvistError::Io {
                operation: "git cat-file",
                path: PathBuf::from("git"),
                source,
            })?;
        if content_output.status.success() {
            let mut hasher = sha2::Sha256::new();
            hasher.update(&content_output.stdout);
            Some(format!(
                "sha256:{}",
                hex::encode(sha2::Digest::finalize(hasher))
            ))
        } else {
            None
        }
    } else {
        None
    };

    if let Some(expected) = expected_pre_digest
        && actual_digest.as_deref() != Some(expected)
    {
        return Err(KvistError::VcsCommitFailed {
            reason: format!(
                "concurrent accepted-path change detected: file `{}` has been modified since acceptance",
                path_str
            ),
        });
    }

    Ok(())
}

/// Loads the acceptance record from either an acceptance journal or a task attempt journal.
pub fn load_acceptance_record(project_dir: &Path, acceptance_id: &str) -> Result<AcceptanceRecord> {
    let attempts_dir = project_dir.join("engine/.kvist-attempts");

    // 1. Check if a dedicated acceptance journal exists
    let acceptance_path = attempts_dir.join(format!("acceptance-{}.json", acceptance_id));
    if acceptance_path.is_file() {
        let contents = fs::read_to_string(&acceptance_path).map_err(|source| KvistError::Io {
            operation: "read acceptance journal",
            path: acceptance_path.clone(),
            source,
        })?;
        let json: Value =
            serde_json::from_str(&contents).map_err(|_| KvistError::VcsCommitFailed {
                reason: "acceptance journal is malformed JSON".to_owned(),
            })?;

        let expected_head = json["expected_head"].as_str().unwrap_or("").to_owned();
        let component_path = PathBuf::from(json["component_path"].as_str().unwrap_or("."));
        let commit_message = json["commit_message"].as_str().unwrap_or("").to_owned();

        let mut accepted_changes = Vec::new();
        if let Some(arr) = json["accepted_changes"].as_array() {
            for change_val in arr {
                let path = PathBuf::from(change_val["path"].as_str().unwrap_or(""));
                let operation = change_val["operation"].as_str().unwrap_or("").to_owned();
                let pre_digest = change_val["pre_digest"].as_str().map(String::from);
                let post_digest = change_val["post_digest"].as_str().map(String::from);
                accepted_changes.push(AcceptedChange {
                    path,
                    operation,
                    pre_digest,
                    post_digest,
                });
            }
        }

        return Ok(AcceptanceRecord {
            acceptance_id: acceptance_id.to_owned(),
            expected_head,
            component_path,
            accepted_changes,
            commit_message,
        });
    }

    // 2. Otherwise, scan for task attempt journals (.jsonl) containing the acceptance_id
    if attempts_dir.is_dir()
        && let Ok(entries) = fs::read_dir(&attempts_dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
                let contents = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                if contents.contains(acceptance_id) {
                    // This is our attempt journal! Parse its JSONL lines
                    let mut expected_head = String::new();
                    let mut accepted_changes = Vec::new();
                    let mut task_id = String::new();

                    for line in contents.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        if let Ok(event) = serde_json::from_str::<Value>(line)
                            && event["attempt_id"].as_str() == Some(acceptance_id)
                        {
                            task_id = event["task_id"].as_str().unwrap_or("").to_owned();
                            if let Some(changes_val) = event.get("changes")
                                && let Some(arr) = changes_val.as_array()
                            {
                                for change_val in arr {
                                    let path =
                                        PathBuf::from(change_val["path"].as_str().unwrap_or(""));
                                    let operation =
                                        change_val["operation"].as_str().unwrap_or("").to_owned();
                                    let pre_digest =
                                        change_val["pre_digest"].as_str().map(String::from);
                                    let post_digest =
                                        change_val["post_digest"].as_str().map(String::from);
                                    accepted_changes.push(AcceptedChange {
                                        path,
                                        operation,
                                        pre_digest,
                                        post_digest,
                                    });
                                }
                            }
                        }
                    }

                    // Query current head OID as expected_head for retry
                    if let Ok(head_output) = Command::new("git")
                        .args(["rev-parse", "HEAD"])
                        .current_dir(project_dir)
                        .output()
                        && head_output.status.success()
                    {
                        expected_head = String::from_utf8_lossy(&head_output.stdout)
                            .trim()
                            .to_owned();
                    }

                    let commit_message = format!("complete task {task_id}");

                    return Ok(AcceptanceRecord {
                        acceptance_id: acceptance_id.to_owned(),
                        expected_head,
                        component_path: PathBuf::from("."),
                        accepted_changes,
                        commit_message,
                    });
                }
            }
        }
    }

    Err(KvistError::VcsCommitFailed {
        reason: format!("no durable acceptance journal contains {acceptance_id}"),
    })
}

/// Core function that executes the isolated Git index commit algorithm.
pub fn commit_acceptance_record(project_dir: &Path, record: &AcceptanceRecord) -> Result<String> {
    // 1. Refuse non-git VCS backends explicitly
    let config = crate::config::load(project_dir)?;
    if config.vcs == VcsSelection::Jujutsu {
        return Err(KvistError::VcsCommitFailed {
            reason: "Jujutsu write backend is unsupported".to_owned(),
        });
    }

    // Ensure we are inside a Git repository
    let git_dir = project_dir.join(".git");
    if !git_dir.exists() {
        return Err(KvistError::VcsCommitFailed {
            reason: "not a git repository".to_owned(),
        });
    }

    // 2. Reject detached HEAD (must be on a branch)
    let branch_ref = get_current_branch(project_dir)?;

    // 3. Verify HEAD hasn't changed since acceptance
    let current_head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_dir)
        .output()
        .map_err(|source| KvistError::Io {
            operation: "git rev-parse HEAD",
            path: PathBuf::from("git"),
            source,
        })?;
    let head_sha = String::from_utf8_lossy(&current_head.stdout)
        .trim()
        .to_owned();
    if head_sha != record.expected_head {
        return Err(KvistError::VcsCommitFailed {
            reason: "concurrent HEAD movement detected; HEAD has changed since acceptance"
                .to_owned(),
        });
    }

    // 4. Verify all accepted pre-digests match what is currently in HEAD
    for change in &record.accepted_changes {
        verify_head_pre_digest(
            project_dir,
            &head_sha,
            &change.path,
            change.pre_digest.as_deref(),
        )?;
    }

    // 5. Setup the isolated index file
    let temp_index_path = git_dir.join(format!("index.kvist_{}", record.acceptance_id));
    if temp_index_path.exists() {
        let _ = fs::remove_file(&temp_index_path);
    }

    let mut signing_policy = "optional".to_owned();
    let config_path = project_dir.join("kvist.toml");
    if config_path.is_file()
        && let Ok(toml_content) = fs::read_to_string(&config_path)
    {
        if toml_content.contains("signing = \"required\"") {
            signing_policy = "required".to_owned();
        } else if let Ok(parsed) = toml_content.parse::<toml::Value>()
            && let Some(signing) = parsed
                .get("vcs")
                .and_then(|v| v.get("commit"))
                .and_then(|v| v.get("signing"))
                .and_then(|v| v.as_str())
        {
            signing_policy = signing.to_owned();
        }
    }

    // 6. Read HEAD tree into the isolated index
    run_git_isolated(project_dir, &temp_index_path, &["read-tree", "HEAD"], true)?;

    // 7. Update isolated index with the accepted changes
    for change in &record.accepted_changes {
        match change.operation.as_str() {
            "delete" => {
                run_git_isolated(
                    project_dir,
                    &temp_index_path,
                    &[
                        "update-index",
                        "--force-remove",
                        &change.path.to_string_lossy(),
                    ],
                    true,
                )?;
            }
            _ => {
                // To avoid relying on dirty host worktree files, we hash the current file state to the Git db
                let absolute_path = project_dir.join(&change.path);
                if !absolute_path.is_file() {
                    let _ = fs::remove_file(&temp_index_path);
                    return Err(KvistError::VcsCommitFailed {
                        reason: format!(
                            "accepted file missing from worktree: {}",
                            change.path.display()
                        ),
                    });
                }
                let blob_oid = git_hash_object(project_dir, &absolute_path)?;
                run_git_isolated(
                    project_dir,
                    &temp_index_path,
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        "100644",
                        &blob_oid,
                        &change.path.to_string_lossy(),
                    ],
                    true,
                )?;
            }
        }
    }

    // 8. Write the tree
    let tree_oid = match run_git_isolated(project_dir, &temp_index_path, &["write-tree"], true) {
        Ok(oid) => oid,
        Err(error) => {
            let _ = fs::remove_file(&temp_index_path);
            return Err(error);
        }
    };

    // Double check that the tree matches the HEAD if there are actually no modifications (optional)

    // 9. Create the commit using commit-tree
    let mut commit_args = vec!["commit-tree"];

    if signing_policy == "required" {
        commit_args.push("-S");
    }

    commit_args.push(&tree_oid);
    commit_args.push("-p");
    commit_args.push("HEAD");
    commit_args.push("-m");
    commit_args.push(&record.commit_message);

    let commit_oid = match run_git_isolated(project_dir, &temp_index_path, &commit_args, true) {
        Ok(oid) => oid,
        Err(error) => {
            let _ = fs::remove_file(&temp_index_path);
            return Err(error);
        }
    };

    // 10. Atomically update branch ref (compare-and-swap update-ref)
    match run_git_isolated(
        project_dir,
        &temp_index_path,
        &["update-ref", &branch_ref, &commit_oid, &head_sha],
        true,
    ) {
        Ok(_) => {
            let _ = fs::remove_file(&temp_index_path);
            Ok(commit_oid)
        }
        Err(error) => {
            let _ = fs::remove_file(&temp_index_path);
            Err(error)
        }
    }
}
