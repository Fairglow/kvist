use sav::loop_detection::{ToolLoopKind, ToolLoopReport};
use sav::runtime::AgentSession;

use super::{LiveRun, RunControls, RunTurn, redact_bounded};
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

pub(super) fn steer(
    report: &ToolLoopReport,
    interventions: &mut u64,
    live: &LiveRun,
    session: &mut AgentSession,
    redactions: &[String],
) -> Result<()> {
    let attempt = interventions.saturating_add(1);
    let (cause, next_step) = match report.kind {
        ToolLoopKind::RepeatedObservation => (
            "The same tool call returned unchanged information",
            "Use the result already in context. If a specific fact is still missing, use `read_file` with a focused `search` or a new `start_line`/`end_line` range, or inspect a different relevant path.",
        ),
        ToolLoopKind::RepeatedFailure => (
            "Tool calls returned the same error",
            "Read the error in the tool result and correct its cause before trying again, or choose another supported tool or path.",
        ),
        ToolLoopKind::RepeatedAction => (
            "An effect was proposed again without an observed change",
            "Check the prior result and current state. If a change is still needed, propose a small targeted edit based on what is actually present.",
        ),
        ToolLoopKind::ActionCycle => (
            "A sequence of tool calls and results repeated",
            "Stop repeating that sequence. Use the results already obtained to make the next targeted change or check one specific missing fact with a different call.",
        ),
    };
    let refocus = if attempt > 2 {
        " Revisit the next unmet requirement and prefer a concrete step over more exploration."
    } else {
        ""
    };
    let feedback = format!(
        "Kvist recovery: {cause}. {next_step}{refocus} \
         Continue the original task. Keep completed changes; no effects ran for calls marked not executed. \
         Do not replay completed or uncertain effects. \
         If the work is complete, report the actual result in plain text without tool calls. \
         Use `propose_decision` only for a specific choice requiring human authority; explain why and give concrete alternatives."
    );
    let notice = attempt.is_power_of_two().then(|| {
        format!(
            "{cause} (tool steering #{attempt}). Kvist is guiding the model to a different step and continuing the task."
        )
    });
    live.record(
        &serde_json::json!({"type":"tool_recovery","data":{
            "kind":report.kind,"period":report.period,"copies":report.copies,
            "attempt":attempt,"feedback":feedback,"notice":notice
        }}),
        true,
    )?;
    if let Some(notice) = notice
        && !live.presenting()
    {
        eprintln!("\n{}", redact_bounded(&notice, redactions, 4096));
    }
    session.push_user(feedback);
    *interventions = attempt;
    Ok(())
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
            execution_tier: crate::agn::ExecutionTier::Sandboxed,
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
