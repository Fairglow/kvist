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
            "cancelled":outcome.cancelled, "stdout":outcome.stdout,
            "stderr":outcome.stderr, "revisions":revisions,
        });
        !self
            .signatures
            .insert(digest(signature.to_string().as_bytes()))
    }

    pub(super) fn remember(&mut self, cause: &str, redactions: &[String]) {
        // Five verification results and four repairs are the outer bound.
        if self.causes.len() < 9 {
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
            stdout: "failure".into(),
            stderr: String::new(),
        }
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
