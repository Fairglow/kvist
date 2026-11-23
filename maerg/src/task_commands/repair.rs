use super::{VerificationResult, digest};

/// Bounded history for one attempt, not a global cache or a resource allowance.
#[derive(Default)]
pub(super) struct RepairHistory {
    signatures: std::collections::BTreeSet<String>,
    causes: Vec<String>,
}

impl RepairHistory {
    pub(super) fn recurring(
        &mut self,
        outcome: &VerificationResult,
        revisions: &(String, String),
    ) -> bool {
        // Every complete outcome field matters, not just a stderr tail.
        let signature = serde_json::json!({
            "command":outcome.command, "success":outcome.success,
            "exit_code":outcome.exit_code, "timed_out":outcome.timed_out,
            "output_limit_exceeded":outcome.output_limit_exceeded,
            "process_limit_exceeded":outcome.process_limit_exceeded,
            "nested_sandbox":outcome.nested_sandbox,
            "cancelled":outcome.cancelled, "stdout":outcome.stdout,
            "stderr":outcome.stderr, "revisions":revisions,
        });
        !self
            .signatures
            .insert(digest(signature.to_string().as_bytes()))
    }

    pub(super) fn remember(&mut self, cause: &str, redactions: &[String]) {
        // Five ordinary outcomes, four repairs, and two focused recoveries
        // with their outcomes. The current cause is reported separately on a stop.
        if self.causes.len() < 13 {
            self.causes.push(super::redact_bounded(
                cause.to_owned(),
                redactions,
                super::MAX_VERIFICATION_EVIDENCE_BYTES / 10,
            ));
        }
    }

    pub(super) fn cause(&self) -> String {
        self.causes.join("\n\n")
    }
}

pub(super) fn resource_guidance(serial: bool) -> &'static str {
    if serial {
        "Kvist is already using one Cargo job and one libtest thread for this attempt. That does not limit pools or subprocesses started by the code under test."
    } else {
        "The approved verifier controls its own parallelism; the model cannot change its sandbox limits."
    }
}

pub(super) const PROCESS_RECOVERY_ACTION: &str = "Use the preserved edits and inspect only task-owned code for excessive worker pools, nested parallel jobs, or children that are not joined/reaped. Bound their concurrency or lifetime where the evidence supports it. Do not skip, ignore, or weaken checks or change sandbox policy. If no task-owned fix is supported, report the concrete prerequisite rather than guessing.";

pub(super) const NESTING_RECOVERY_ACTION: &str = "Run portable assertions directly in the existing sandbox. Preserve assertions that genuinely launch Galla in an established, mandatory outer Linux host lane, if the approved scope permits. Do not skip, ignore, or weaken assertions to hide failures, change named regression expectations, remove the nesting marker, or change sandbox policy. If the required host lane is outside task scope, report that prerequisite.";

pub(super) fn reports_nesting(stdout: &[u8], stderr: &[u8]) -> bool {
    let diagnostic = galla::context::NESTING_DIAGNOSTIC.as_bytes();
    [stdout, stderr].into_iter().any(|stream| {
        stream
            .windows(diagnostic.len())
            .any(|window| window == diagnostic)
    })
}

pub(super) fn failure_excerpt(value: String, redactions: &[String], limit: usize) -> String {
    const OMITTED: &str = "\n[... middle omitted ...]\n";
    let value = super::redact_bounded(value, redactions, usize::MAX);
    if value.len() <= limit {
        return value;
    }
    if limit <= OMITTED.len() {
        return value[..value.floor_char_boundary(limit)].to_owned();
    }
    let available = limit - OMITTED.len();
    let head = value.floor_char_boundary(available / 2);
    let tail = value.ceil_char_boundary(value.len() - (available - available / 2));
    format!("{}{OMITTED}{}", &value[..head], &value[tail..])
}

pub(super) fn failure_details(outcome: &VerificationResult, redactions: &[String]) -> String {
    let stdout = failure_excerpt(outcome.stdout.clone(), redactions, 16384);
    let stderr = failure_excerpt(outcome.stderr.clone(), redactions, 16384);
    format!("--- Verification Stdout ---\n{stdout}\n--- Verification Stderr ---\n{stderr}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome() -> VerificationResult {
        VerificationResult {
            success: false,
            command: "test".into(),
            exit_code: Some(1),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
            process_limit_exceeded: false,
            nested_sandbox: false,
            stdout: "failure".into(),
            stderr: String::new(),
        }
    }

    #[test]
    fn failure_excerpt_keeps_final_assertions_and_redacts_across_clip_boundaries() {
        let input = format!(
            "cargo test\n{}{}\nassertion failed: final case\n",
            "secret".repeat(38),
            "é".repeat(50)
        );
        let excerpt = failure_excerpt(input, &["secret".into()], 100);
        assert!(excerpt.len() <= 100);
        assert!(!excerpt.contains("secret"));
        assert!(excerpt.starts_with("cargo test\n"));
        assert!(excerpt.ends_with("assertion failed: final case\n"));
    }

    #[test]
    fn phase4_recurrence_binds_every_complete_outcome_field_and_revisions() {
        let original = outcome();
        let revisions = ("source".to_owned(), "tests".to_owned());
        let mut history = RepairHistory::default();
        assert!(!history.recurring(&original, &revisions));
        for changed in [
            VerificationResult {
                command: "other test".into(),
                ..original.clone()
            },
            VerificationResult {
                exit_code: Some(2),
                ..original.clone()
            },
            VerificationResult {
                success: true,
                ..original.clone()
            },
            VerificationResult {
                timed_out: true,
                ..original.clone()
            },
            VerificationResult {
                output_limit_exceeded: true,
                ..original.clone()
            },
            VerificationResult {
                cancelled: true,
                ..original.clone()
            },
            VerificationResult {
                stdout: "new stdout".into(),
                ..original.clone()
            },
            VerificationResult {
                stderr: "new stderr".into(),
                ..original.clone()
            },
        ] {
            assert!(!history.recurring(&changed, &revisions));
            assert!(history.recurring(&changed, &revisions));
        }
        assert!(!history.recurring(&original, &("changed source".into(), "tests".into())));
        assert!(!history.recurring(&original, &("source".into(), "changed tests".into())));
        assert!(
            history.recurring(&original, &revisions),
            "cycling back is not progress"
        );
    }
}
