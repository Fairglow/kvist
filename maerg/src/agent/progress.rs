use sav::loop_detection::{RecoveryBudget, ToolLoopReport};
use sav::runtime::AgentSession;

use super::{LiveRun, RunControls, RunTurn, TurnFailure, redact_bounded};
use crate::Result;

pub(super) fn effect_revision(
    component: &std::path::Path,
    effect: &crate::authoring::CheckedIntent,
) -> Result<Option<String>> {
    let path = crate::authoring::normalize_destination(
        component,
        &effect.destination,
        &crate::authoring::BrokerPolicy::default(),
    )
    .map_err(|reason| crate::KvistError::AuthoringEffectFailed {
        call_id: effect.call_id.clone(),
        reason,
    })?;
    crate::authoring::apply::verify_no_follow_ancestors(
        component,
        path.parent().map(std::path::Path::to_path_buf),
    )?;
    match crate::file_io::read_text_bounded(&path, crate::authoring::MAX_CONTENT_BYTES) {
        Ok(contents) => Ok(Some(hex::encode(<sha2::Sha256 as sha2::Digest>::digest(
            contents.as_bytes(),
        )))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(crate::KvistError::Io {
            operation: "inspect authoring effect revision",
            path,
            source,
        }),
    }
}

pub(super) fn interrupt_batch(
    turn: &RunTurn,
    steering: &RunControls<'_>,
    session: &mut AgentSession,
) -> Result<()> {
    steering.skip_calls_with_reason(
        turn.turn.tool_intents.iter().filter(|intent| {
            !turn.reads.iter().any(|read| read.call_id == intent.id)
                && !turn.effects.iter().any(|effect| effect.call_id == intent.id)
                && !turn.accepted_requests.iter().any(|request| request.call_id == intent.id)
        }),
        session,
        "not executed: remaining batch interrupted before dispatch; reconcile completed results before proposing a different safe strategy",
    )
}

pub(super) fn correct(
    report: &ToolLoopReport,
    recovery: &mut RecoveryBudget,
    corrections_used: &mut u8,
    live: &LiveRun,
    session: &mut AgentSession,
    redactions: &[String],
) -> Result<Option<TurnFailure>> {
    let Some(step) = recovery.take() else {
        live.record(
            &serde_json::json!({"type":"tool_stalled","data":{
                "kind":report.kind,"period":report.period,"copies":report.copies
            }}),
            true,
        )?;
        return Ok(Some(TurnFailure {
            stop_reason: sav::runtime::StopReason::Stalled,
            reason: report.exhausted_message(),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        }));
    };
    let feedback = report.feedback(step);
    *corrections_used = corrections_used.saturating_add(1);
    live.record(
        &serde_json::json!({"type":"tool_recovery","data":{
            "kind":report.kind,"period":report.period,"copies":report.copies,
            "attempt":step.attempt,"limit":step.limit,"feedback":feedback
        }}),
        true,
    )?;
    eprintln!("{}", redact_bounded(&feedback, redactions, 4096));
    session.push_user(feedback);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::{CheckedIntent, EffectOp};
    use std::fs;

    fn effect() -> CheckedIntent {
        CheckedIntent {
            call_id: "fixture".into(),
            tool: "write_file".into(),
            op: EffectOp::Create,
            destination: "src/file.rs".into(),
            content_identity: String::new(),
            replacement: None,
            content_bytes: 0,
            purpose: "fixture".into(),
        }
    }

    #[test]
    fn phase3_revision_tracks_actual_bytes_not_success_claims() {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        fs::create_dir(dir.path().join("src")).expect("source");
        let effect = effect();
        assert_eq!(effect_revision(dir.path(), &effect).expect("absent"), None);
        fs::write(dir.path().join("src/file.rs"), "// initial\n").expect("file");
        let before = effect_revision(dir.path(), &effect).expect("before");
        assert_eq!(
            before,
            effect_revision(dir.path(), &effect).expect("unchanged")
        );
        fs::write(dir.path().join("src/file.rs"), "// changed\n").expect("change");
        assert_ne!(before, effect_revision(dir.path(), &effect).expect("after"));
    }

    #[test]
    fn phase3_revision_refuses_linked_ancestors() {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        let outside = tempfile::tempdir_in(".").expect("outside fixture");
        fs::write(outside.path().join("file.rs"), "// outside grant\n").expect("file");
        std::os::unix::fs::symlink(
            outside.path().canonicalize().expect("canonical"),
            dir.path().join("src"),
        )
        .expect("link");
        assert!(effect_revision(dir.path(), &effect()).is_err());
    }

    #[test]
    fn phase3_revision_read_failure_is_not_progress() {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        fs::create_dir(dir.path().join("src")).expect("source");
        fs::write(
            dir.path().join("src/file.rs"),
            vec![b'x'; crate::authoring::MAX_CONTENT_BYTES + 1],
        )
        .expect("oversize");
        assert!(effect_revision(dir.path(), &effect()).is_err());
    }
}
