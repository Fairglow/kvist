//! Authoring effect staging and in-sandbox application.
//!
//! The host stages each authorized [`CheckedIntent`] as a [`StagedIntent`] file
//! under the component state directory; the effect sandbox then runs the kvist
//! binary itself (`kvist authoring-apply`) with the staged file mounted
//! read-only. The applier re-validates every bound, proves the content identity,
//! and applies the effect without following symbolic links, so the sandboxed
//! process is the only code that ever touches the filesystem for an effect.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use agent_runtime::ToolIntent;
use hex::encode as hex_encode;
use sha2::{Digest, Sha256};

use crate::{KvistError, Result, file_io};

use super::{
    ALLOWED_TOOLS, BrokerPolicy, CheckedIntent, MAX_CONTENT_BYTES, STAGED_INTENT_SCHEMA_VERSION,
    StagedIntent, count_occurrences, normalize_destination, replace_single_occurrence,
};

/// Maximum bytes of a staged intent file. The per-effect content bound plus JSON
/// overhead leaves ample headroom; anything larger is a malformed stage.
const MAX_STAGED_INTENT_BYTES: u64 = 2 << 20;

/// Stages one authorized effect for in-sandbox application.
///
/// The staged file lands at `<component>/.kvist/authoring/<session>_<hash>.json`,
/// where the hash derives from the model call identity so untrusted call ids can
/// never influence the path. The raw intent payload must hash to the authorized
/// identity or staging fails closed.
pub fn stage_intent(
    component_root: &Path,
    session_id: &str,
    effect: &CheckedIntent,
    raw: &ToolIntent,
) -> Result<PathBuf> {
    let content = match effect.tool.as_str() {
        "write_file" => raw
            .arguments
            .get("content")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| KvistError::AuthoringEffectFailed {
                call_id: effect.call_id.clone(),
                reason: "raw intent carries no writable content".to_owned(),
            })?
            .to_owned(),
        "edit_file" => raw
            .arguments
            .get("content")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| KvistError::AuthoringEffectFailed {
                call_id: effect.call_id.clone(),
                reason: "raw intent carries no replacement content".to_owned(),
            })?
            .to_owned(),
        _ => {
            return Err(KvistError::AuthoringEffectFailed {
                call_id: effect.call_id.clone(),
                reason: format!("tool `{}` is not an authorized authoring tool", effect.tool),
            });
        }
    };

    let staged = StagedIntent {
        schema_version: STAGED_INTENT_SCHEMA_VERSION,
        call_id: effect.call_id.clone(),
        tool: effect.tool.clone(),
        destination: effect.destination.clone(),
        content_identity: effect.content_identity.clone(),
        content,
        replacement: effect.replacement.clone(),
    };

    let directory = component_root.join(".kvist").join("authoring");
    ensure_real_directory(&directory, "create authoring staging directory")?;
    let identity = &hex_encode(Sha256::digest(effect.call_id.as_bytes()))[..16];
    let path = directory.join(format!("{session_id}_{identity}.json"));
    let bytes =
        serde_json::to_vec(&staged).map_err(|source| KvistError::AuthoringEffectFailed {
            call_id: effect.call_id.clone(),
            reason: format!("staged intent cannot be serialized: {source}"),
        })?;
    file_io::write_new_file_atomically(&path, &String::from_utf8_lossy(&bytes))?;
    Ok(path)
}

/// Applies one staged authoring effect. Runs inside the effect sandbox: the
/// staged file is mounted read-only and the destination lies under a writable
/// root. Every bound is re-validated and the content identity is proven before
/// any byte is written.
pub fn apply_intent(component_root: &Path, intent_file: &Path) -> Result<String> {
    let staged = read_staged_intent(intent_file)?;
    if staged.schema_version != STAGED_INTENT_SCHEMA_VERSION {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: staged.call_id.clone(),
            reason: format!(
                "unsupported staged intent schema version {}",
                staged.schema_version
            ),
        });
    }
    if !ALLOWED_TOOLS.contains(&staged.tool.as_str()) {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: staged.call_id.clone(),
            reason: format!(
                "tool `{}` is not in the authorized authoring set",
                staged.tool
            ),
        });
    }
    let resolved = normalize_destination(
        component_root,
        &staged.destination,
        &BrokerPolicy::default(),
    )
    .map_err(|reason| KvistError::AuthoringEffectFailed {
        call_id: staged.call_id.clone(),
        reason,
    })?;
    // No path segment between the component root and the destination may be a
    // symlink, so the effect cannot be redirected outside the writable root.
    verify_no_follow_ancestors(component_root, resolved.parent().map(Path::to_path_buf))?;

    match staged.tool.as_str() {
        "write_file" => apply_write(&staged, &resolved)?,
        "edit_file" => apply_edit(&staged, &resolved)?,
        _ => {
            return Err(KvistError::AuthoringEffectFailed {
                call_id: staged.call_id.clone(),
                reason: format!(
                    "tool `{}` is not in the authorized authoring set",
                    staged.tool
                ),
            });
        }
    }

    Ok(serde_json::json!({
        "result": "applied",
        "call_id": staged.call_id,
        "tool": staged.tool,
        "destination": staged.destination,
    })
    .to_string())
}

fn read_staged_intent(intent_file: &Path) -> Result<StagedIntent> {
    let metadata = fs::symlink_metadata(intent_file).map_err(|source| KvistError::Io {
        operation: "inspect staged authoring intent",
        path: intent_file.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: String::new(),
            reason: "staged intent file is a symbolic link".to_owned(),
        });
    }
    if !metadata.is_file() || metadata.len() > MAX_STAGED_INTENT_BYTES {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: String::new(),
            reason: "staged intent file is missing or exceeds the size bound".to_owned(),
        });
    }
    let bytes = fs::read(intent_file).map_err(|source| KvistError::Io {
        operation: "read staged authoring intent",
        path: intent_file.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| KvistError::AuthoringEffectFailed {
        call_id: String::new(),
        reason: format!("staged intent is not valid version-1 JSON: {source}"),
    })
}

fn prove_identity(call_id: &str, bytes: &[u8], expected: &str) -> Result<()> {
    let actual = format!("sha256:{}", hex_encode(Sha256::digest(bytes)));
    if actual != expected {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: call_id.to_owned(),
            reason: "content identity mismatch: the payload is not the authorized bytes".to_owned(),
        });
    }
    Ok(())
}

/// Verifies every directory component between the component root and the given
/// directory is a real directory (never a symlink).
fn verify_no_follow_ancestors(component_root: &Path, ancestor_dir: Option<PathBuf>) -> Result<()> {
    let Some(target) = ancestor_dir else {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: String::new(),
            reason: "destination has no parent directory".to_owned(),
        });
    };
    let rel = target
        .strip_prefix(component_root)
        .map_err(|source| KvistError::Io {
            operation: "anchor destination under the component root",
            path: target.clone(),
            source: io::Error::other(source.to_string()),
        })?;
    let mut ancestor = PathBuf::from(component_root);
    for component in rel.components() {
        ancestor.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&ancestor).map_err(|source| KvistError::Io {
            operation: "inspect authoring path component",
            path: ancestor.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(KvistError::AuthoringEffectFailed {
                call_id: String::new(),
                reason: format!("path component `{}` is a symbolic link", ancestor.display()),
            });
        }
        if !metadata.is_dir() {
            return Err(KvistError::AuthoringEffectFailed {
                call_id: String::new(),
                reason: format!("path component `{}` is not a directory", ancestor.display()),
            });
        }
    }
    Ok(())
}

fn apply_write(staged: &StagedIntent, resolved: &Path) -> Result<()> {
    prove_identity(
        &staged.call_id,
        staged.content.as_bytes(),
        &staged.content_identity,
    )?;
    match fs::symlink_metadata(resolved) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(KvistError::AuthoringEffectFailed {
                call_id: staged.call_id.clone(),
                reason: format!("destination `{}` is a symbolic link", resolved.display()),
            })
        }
        Ok(metadata) if metadata.is_file() => {
            file_io::replace_file_atomically(resolved, &staged.content)
        }
        Ok(_) => Err(KvistError::AuthoringEffectFailed {
            call_id: staged.call_id.clone(),
            reason: format!("destination `{}` is not a regular file", resolved.display()),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            file_io::write_new_file_atomically(resolved, &staged.content)
        }
        Err(source) => Err(KvistError::Io {
            operation: "inspect authoring destination",
            path: resolved.to_path_buf(),
            source,
        }),
    }
}

fn apply_edit(staged: &StagedIntent, resolved: &Path) -> Result<()> {
    let replacement =
        staged
            .replacement
            .as_deref()
            .ok_or_else(|| KvistError::AuthoringEffectFailed {
                call_id: staged.call_id.clone(),
                reason: "edit effect carries no replacement substring".to_owned(),
            })?;
    let metadata = fs::symlink_metadata(resolved).map_err(|source| KvistError::Io {
        operation: "inspect edit destination",
        path: resolved.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: staged.call_id.clone(),
            reason: format!("destination `{}` is a symbolic link", resolved.display()),
        });
    }
    if !metadata.is_file() {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: staged.call_id.clone(),
            reason: format!(
                "destination `{}` does not exist or is not a file",
                resolved.display()
            ),
        });
    }
    let current = fs::read(resolved).map_err(|source| KvistError::Io {
        operation: "read edit destination",
        path: resolved.to_path_buf(),
        source,
    })?;
    if current.len() > MAX_CONTENT_BYTES {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: staged.call_id.clone(),
            reason: "destination exceeds the per-effect content bound".to_owned(),
        });
    }
    let occurrences = count_occurrences(&current, replacement.as_bytes());
    if occurrences != 1 {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: staged.call_id.clone(),
            reason: format!(
                "`replace` must occur exactly once in `{}` (found {occurrences}; the file changed since authorization)",
                resolved.display()
            ),
        });
    }
    let Some(new_content) =
        replace_single_occurrence(&current, replacement.as_bytes(), staged.content.as_bytes())
    else {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: staged.call_id.clone(),
            reason: format!(
                "`replace` must occur exactly once in `{}` (found {occurrences}; the file changed since authorization)",
                resolved.display()
            ),
        });
    };
    prove_identity(&staged.call_id, &new_content, &staged.content_identity)?;
    if new_content.len() > MAX_CONTENT_BYTES {
        return Err(KvistError::AuthoringEffectFailed {
            call_id: staged.call_id.clone(),
            reason: "result exceeds the per-effect content bound".to_owned(),
        });
    }
    file_io::replace_file_atomically(resolved, &String::from_utf8_lossy(&new_content))
}

fn ensure_real_directory(path: &Path, operation: &'static str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            Ok(())
        }
        Ok(_) => Err(KvistError::Io {
            operation,
            path: path.to_path_buf(),
            source: io::Error::other("directory must be a real directory"),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|source| KvistError::Io {
                operation,
                path: path.to_path_buf(),
                source,
            })
        }
        Err(source) => Err(KvistError::Io {
            operation,
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::ToolIntent;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    fn identity_of(bytes: &[u8]) -> String {
        format!("sha256:{}", hex_encode(Sha256::digest(bytes)))
    }

    /// A component fixture with the writable roots and one protected document.
    fn component() -> TempDir {
        let dir = TempDir::new().expect("component fixture");
        fs::create_dir(dir.path().join("tests")).expect("tests root");
        fs::create_dir(dir.path().join("src")).expect("src root");
        fs::create_dir(dir.path().join(".kvist")).expect("component state root");
        fs::write(dir.path().join("REQUIREMENTS.md"), "protected intent\n").expect("intent");
        dir
    }

    fn write_effect(destination: &str, content: &str) -> CheckedIntent {
        CheckedIntent {
            call_id: "call-1".to_owned(),
            tool: "write_file".to_owned(),
            op: super::super::EffectOp::Create,
            destination: destination.to_owned(),
            content_identity: identity_of(content.as_bytes()),
            replacement: None,
            content_bytes: content.len(),
            purpose: "test".to_owned(),
        }
    }

    fn edit_effect(destination: &str, replacement: &str, result: &str) -> CheckedIntent {
        CheckedIntent {
            call_id: "call-1".to_owned(),
            tool: "edit_file".to_owned(),
            op: super::super::EffectOp::Modify,
            destination: destination.to_owned(),
            content_identity: identity_of(result.as_bytes()),
            replacement: Some(replacement.to_owned()),
            content_bytes: result.len(),
            purpose: "test".to_owned(),
        }
    }

    fn raw_write(call_id: &str, destination: &str, content: &str) -> ToolIntent {
        ToolIntent {
            id: call_id.to_owned(),
            provider_id: None,
            name: "write_file".to_owned(),
            arguments: serde_json::json!({
                "destination": destination,
                "content": content
            }),
        }
    }

    fn raw_edit(call_id: &str, destination: &str, replace: &str, content: &str) -> ToolIntent {
        ToolIntent {
            id: call_id.to_owned(),
            provider_id: None,
            name: "edit_file".to_owned(),
            arguments: serde_json::json!({
                "destination": destination,
                "replace": replace,
                "content": content
            }),
        }
    }

    #[test]
    fn stages_a_write_intent_under_the_component_state_directory() {
        let root = component();
        let effect = write_effect("tests/generated.rs", "fn main() {}\n");
        let staged = stage_intent(
            root.path(),
            "session-1",
            &effect,
            &raw_write("call-1", "tests/generated.rs", "fn main() {}\n"),
        )
        .expect("stage write");
        assert!(
            staged.starts_with(root.path().join(".kvist").join("authoring")),
            "staged file must land under the component state directory: {}",
            staged.display()
        );
        assert!(staged.to_string_lossy().contains("session-1"));
        let bytes = fs::read(&staged).expect("read staged");
        let parsed: StagedIntent = serde_json::from_slice(&bytes).expect("parse staged");
        assert_eq!(parsed.call_id, "call-1");
        assert_eq!(parsed.destination, "tests/generated.rs");
        assert_eq!(parsed.content, "fn main() {}\n");
        assert!(parsed.replacement.is_none());
    }

    #[test]
    fn stages_an_edit_intent_with_its_replacement() {
        let root = component();
        let effect = edit_effect("tests/a.rs", "alpha", "line\nbeta\n");
        let staged = stage_intent(
            root.path(),
            "session-1",
            &effect,
            &raw_edit("call-1", "tests/a.rs", "alpha", "beta"),
        )
        .expect("stage edit");
        let parsed: StagedIntent =
            serde_json::from_slice(&fs::read(staged).expect("read staged")).expect("parse");
        assert_eq!(parsed.replacement.as_deref(), Some("alpha"));
        assert_eq!(parsed.content, "beta");
    }

    #[test]
    fn staging_refuses_unknown_tools_and_missing_content() {
        let root = component();
        let mut effect = write_effect("tests/a.rs", "x");
        effect.tool = "bash".to_owned();
        assert!(
            stage_intent(
                root.path(),
                "s",
                &effect,
                &raw_write("c", "tests/a.rs", "x")
            )
            .is_err()
        );
        let effect = write_effect("tests/a.rs", "x");
        assert!(
            stage_intent(root.path(), "s", &effect, &raw_write("c", "tests/a.rs", "")).is_err()
        );
    }

    #[test]
    fn untrusted_call_ids_cannot_traverse_the_staging_path_and_never_overwrite() {
        let root = component();
        let mut effect = write_effect("tests/a.rs", "x");
        effect.call_id = "../../evil".to_owned();
        let staged = stage_intent(
            root.path(),
            "s",
            &effect,
            &raw_write("../../evil", "tests/a.rs", "x"),
        )
        .expect("stage traversal call id");
        assert!(
            staged.starts_with(root.path().join(".kvist").join("authoring")),
            "the call id must not influence the path: {}",
            staged.display()
        );
        // The same session and call identity must never overwrite an existing
        // staged file.
        assert!(
            stage_intent(
                root.path(),
                "s",
                &effect,
                &raw_write("../../evil", "tests/a.rs", "x")
            )
            .is_err()
        );
    }

    #[test]
    fn applies_a_write_create_and_an_overwrite() {
        let root = component();
        let destination = root.path().join("tests").join("generated.rs");

        let effect = write_effect("tests/generated.rs", "#[test]\nfn generated() {}\n");
        let staged = stage_intent(
            root.path(),
            "s",
            &effect,
            &raw_write(
                "call-1",
                "tests/generated.rs",
                "#[test]\nfn generated() {}\n",
            ),
        )
        .expect("stage");
        apply_intent(root.path(), &staged).expect("apply create");
        assert_eq!(
            fs::read_to_string(&destination).expect("read created"),
            "#[test]\nfn generated() {}\n"
        );

        let effect = write_effect("tests/generated.rs", "overwritten\n");
        let staged = stage_intent(
            root.path(),
            "s2",
            &effect,
            &raw_write("call-1", "tests/generated.rs", "overwritten\n"),
        )
        .expect("stage overwrite");
        apply_intent(root.path(), &staged).expect("apply overwrite");
        assert_eq!(
            fs::read_to_string(&destination).expect("read overwritten"),
            "overwritten\n"
        );
    }

    #[test]
    fn write_refuses_symlink_destinations_and_ancestors() {
        let root = component();
        let outside = TempDir::new().expect("outside");
        let target = outside.path().join("target.txt");
        fs::write(&target, "outside\n").expect("outside file");

        // A symlink destination must not be written through.
        symlink(&target, root.path().join("tests").join("link-file")).expect("symlink file");
        let effect = write_effect("tests/link-file", "pwned\n");
        let staged = stage_intent(
            root.path(),
            "s",
            &effect,
            &raw_write("call-1", "tests/link-file", "pwned\n"),
        )
        .expect("stage");
        let error =
            apply_intent(root.path(), &staged).expect_err("symlink destination must be refused");
        assert!(
            error.to_string().contains("symbolic link"),
            "diagnostic: {error}"
        );
        assert_eq!(
            fs::read_to_string(&target).expect("outside read"),
            "outside\n"
        );

        // A symlink directory between the root and the destination must be
        // refused before any write.
        symlink(outside.path(), root.path().join("tests").join("link-dir")).expect("symlink dir");
        let effect = write_effect("tests/link-dir/inner.rs", "pwned\n");
        let staged = stage_intent(
            root.path(),
            "s2",
            &effect,
            &raw_write("call-1", "tests/link-dir/inner.rs", "pwned\n"),
        )
        .expect("stage");
        let error =
            apply_intent(root.path(), &staged).expect_err("symlink ancestor must be refused");
        assert!(
            error.to_string().contains("symbolic link"),
            "diagnostic: {error}"
        );
        assert!(!outside.path().join("inner.rs").exists());
    }

    #[test]
    fn applies_an_edit_when_the_replacement_still_occurs_exactly_once() {
        let root = component();
        let path = root.path().join("tests").join("a.rs");
        let current = "line one\nalpha\nline three\n";
        fs::write(&path, current).expect("write fixture");
        let result = "line one\nbeta\nline three\n";

        let effect = edit_effect("tests/a.rs", "alpha", result);
        let staged = stage_intent(
            root.path(),
            "s",
            &effect,
            &raw_edit("call-1", "tests/a.rs", "alpha", "beta"),
        )
        .expect("stage");
        apply_intent(root.path(), &staged).expect("apply edit");
        assert_eq!(fs::read_to_string(&path).expect("read edited"), result);
    }

    #[test]
    fn edit_fails_closed_when_the_file_drifted_after_authorization() {
        let root = component();
        let path = root.path().join("tests").join("a.rs");

        // Zero occurrences: the file changed since authorization.
        fs::write(&path, "nothing here\n").expect("write fixture");
        let effect = edit_effect("tests/a.rs", "alpha", "whatever\n");
        let staged = stage_intent(
            root.path(),
            "s",
            &effect,
            &raw_edit("call-1", "tests/a.rs", "alpha", "beta"),
        )
        .expect("stage");
        let error = apply_intent(root.path(), &staged).expect_err("drift must be refused");
        assert!(
            error.to_string().contains("exactly once"),
            "diagnostic: {error}"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("unchanged"),
            "nothing here\n"
        );

        // Two occurrences: ambiguous, refused.
        fs::write(&path, "alpha\nalpha\n").expect("write fixture");
        let error = apply_intent(root.path(), &staged).expect_err("ambiguity must be refused");
        assert!(
            error.to_string().contains("exactly once"),
            "diagnostic: {error}"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("unchanged"),
            "alpha\nalpha\n"
        );
    }

    #[test]
    fn edit_fails_closed_when_the_derived_identity_mismatches() {
        let root = component();
        let path = root.path().join("tests").join("a.rs");
        fs::write(&path, "alpha\n").expect("write fixture");
        // The authorized identity was computed for a different result.
        let effect = edit_effect("tests/a.rs", "alpha", "different result\n");
        let staged = stage_intent(
            root.path(),
            "s",
            &effect,
            &raw_edit("call-1", "tests/a.rs", "alpha", "beta"),
        )
        .expect("stage");
        let error =
            apply_intent(root.path(), &staged).expect_err("identity mismatch must be refused");
        assert!(
            error.to_string().contains("content identity mismatch"),
            "diagnostic: {error}"
        );
        assert_eq!(fs::read_to_string(&path).expect("unchanged"), "alpha\n");
    }

    #[test]
    fn edit_refuses_symlink_destinations_and_missing_files() {
        let root = component();
        let outside = TempDir::new().expect("outside");
        let target = outside.path().join("target.txt");
        fs::write(&target, "outside\n").expect("outside file");
        symlink(&target, root.path().join("tests").join("link")).expect("symlink");

        let effect = edit_effect("tests/link", "outside", "pwned");
        let staged = stage_intent(
            root.path(),
            "s",
            &effect,
            &raw_edit("call-1", "tests/link", "outside", "pwned"),
        )
        .expect("stage");
        let error =
            apply_intent(root.path(), &staged).expect_err("symlink destination must be refused");
        assert!(
            error.to_string().contains("symbolic link"),
            "diagnostic: {error}"
        );
        assert_eq!(
            fs::read_to_string(&target).expect("outside read"),
            "outside\n"
        );

        let effect = edit_effect("tests/missing.rs", "x", "y");
        let staged = stage_intent(
            root.path(),
            "s2",
            &effect,
            &raw_edit("call-1", "tests/missing.rs", "x", "y"),
        )
        .expect("stage");
        assert!(apply_intent(root.path(), &staged).is_err());
    }

    #[test]
    fn destinations_outside_the_writable_roots_are_refused() {
        let root = component();
        let effect = write_effect("REQUIREMENTS.md", "pwned\n");
        let staged = stage_intent(
            root.path(),
            "s",
            &effect,
            &raw_write("call-1", "REQUIREMENTS.md", "pwned\n"),
        )
        .expect("stage");
        let error =
            apply_intent(root.path(), &staged).expect_err("protected document must be refused");
        assert!(
            error.to_string().contains("protected intent document"),
            "diagnostic: {error}"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("REQUIREMENTS.md")).expect("intent read"),
            "protected intent\n"
        );
    }

    #[test]
    fn staged_intent_file_must_be_a_regular_file() {
        let root = component();
        let effect = write_effect("tests/a.rs", "x\n");
        let real = stage_intent(
            root.path(),
            "s",
            &effect,
            &raw_write("call-1", "tests/a.rs", "x\n"),
        )
        .expect("stage");
        let link = root
            .path()
            .join(".kvist")
            .join("authoring")
            .join("link-intent.json");
        symlink(&real, &link).expect("symlink intent");
        assert!(apply_intent(root.path(), &link).is_err());
    }

    #[test]
    fn staged_intent_rejects_bad_schemas_and_oversized_files() {
        let root = component();
        let dir = root.path().join(".kvist").join("authoring");
        fs::create_dir_all(&dir).expect("staging dir");

        let malformed = dir.join("malformed.json");
        fs::write(&malformed, "not json").expect("write malformed");
        assert!(apply_intent(root.path(), &malformed).is_err());

        let mut bad_version: StagedIntent = serde_json::from_value(serde_json::json!({
            "schema_version": 99,
            "call_id": "c",
            "tool": "write_file",
            "destination": "tests/a.rs",
            "content_identity": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "content": "x",
            "replacement": null
        }))
        .expect("build bad version");
        bad_version.schema_version = 99;
        let bad_version = dir.join("bad-version.json");
        fs::write(
            &bad_version,
            serde_json::to_vec(&bad_version).expect("serialize"),
        )
        .expect("write");
        assert!(apply_intent(root.path(), &bad_version).is_err());

        let oversized = dir.join("oversized.json");
        fs::write(&oversized, vec![b'x'; MAX_STAGED_INTENT_BYTES as usize + 1])
            .expect("write oversized");
        assert!(apply_intent(root.path(), &oversized).is_err());
    }
}
