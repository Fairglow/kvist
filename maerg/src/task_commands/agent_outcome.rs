//! Shared durable dispositions for initial authoring and verification repair.

use super::*;
use crate::agent::AgentRunResult;

pub(super) struct AgentAttempt<'a> {
    pub context: &'a TaskContext,
    pub lock: &'a TaskLock,
    pub task_id: &'a str,
    pub attempt_id: &'a str,
    pub journal: &'a Path,
    pub redactions: &'a [String],
}

impl AgentAttempt<'_> {
    pub fn record(
        &self,
        phase: &str,
        execution: Result<AgentRunResult>,
        prior_cause: Option<&str>,
        sequence: Option<&crate::agent::AuthoringSequence>,
    ) -> Result<AgentRunResult> {
        let at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
        let (success, timed_out, output_limit_exceeded, stdout, stderr) = match &execution {
            Ok(run) => (
                run.success,
                run.timed_out,
                run.output_limit_exceeded,
                run.stdout.as_str(),
                run.stderr.clone(),
            ),
            Err(error) => {
                let model_error = match error {
                    KvistError::Sav(source)
                    | KvistError::AgentModelRequestFailed { source, .. } => Some(source),
                    _ => None,
                };
                (
                    false,
                    model_error.is_some_and(|source| {
                        matches!(
                            source,
                            sav::Error::ModelTransportTimedOut
                                | sav::Error::SlotAllocationTimedOut { .. }
                                | sav::Error::TtftTimedOut { .. }
                                | sav::Error::InterTokenCadenceTimedOut { .. }
                        )
                    }),
                    model_error.is_some_and(|source| {
                        matches!(source, sav::Error::ModelResponseLimitExceeded { .. })
                    }),
                    "",
                    error.to_string(),
                )
            }
        };
        let diagnostics = sequence.map(|sequence| {
            sequence.diagnostics(
                execution
                    .as_ref()
                    .err()
                    .map_or(sequence.last_stop, |error| match sequence.last_stop {
                        sav::runtime::StopReason::NoWork | sav::runtime::StopReason::Completed => {
                            crate::agent::error_stop(error)
                        }
                        frozen => frozen,
                    }),
            )
        });
        let diagnostic_text = diagnostics
            .as_ref()
            .map(|diagnostics| diagnostics.message());
        let stderr = if !success {
            match diagnostic_text {
                Some(diagnostic) => format!("{stderr}\n{diagnostic}"),
                None => stderr,
            }
        } else {
            stderr
        };
        let reason = self.cause(prior_cause, &stderr);
        let log_path = execution
            .as_ref()
            .ok()
            .map(|run| run.log_path.as_path())
            .or_else(|| sequence.and_then(|sequence| sequence.current_log_path.as_deref()));
        append_agent_execution(
            self.journal,
            AgentExecutionRecord {
                schema_version: ATTEMPT_SCHEMA_VERSION,
                attempt_id: self.attempt_id,
                phase,
                task_id: self.task_id,
                timestamp: &at,
                success,
                timed_out,
                output_limit_exceeded,
                stdout,
                stderr: &reason,
                cancelled: sequence.is_some_and(|sequence| {
                    sequence.last_stop == sav::runtime::StopReason::Cancelled
                }) || execution.as_ref().err().is_some_and(|error| {
                    crate::agent::error_stop(error) == sav::runtime::StopReason::Cancelled
                }),
                surfaced_decision: execution.as_ref().is_ok_and(|run| run.surfaced_decision),
                model_sequence: diagnostics.map(serde_json::to_value).transpose().map_err(
                    |error| KvistError::AgentEvidenceFailed {
                        reason: format!("cannot encode sequence diagnostics: {error}"),
                    },
                )?,
                log_path: log_path
                    .map(|path| redact_bounded(path.display().to_string(), self.redactions, 4096)),
            },
        )
        .map_err(|recording| {
            unresolved_run_error(
                self.context,
                self.task_id,
                self.attempt_id,
                &reason,
                &recording,
            )
        })?;
        match execution {
            Ok(run) => Ok(run),
            Err(_) => Err(self.block(&reason, log_path)?),
        }
    }

    pub fn cause(&self, prior: Option<&str>, cause: &str) -> String {
        redact_bounded(
            match prior {
                Some(prior) => format!(
                    "{}\nVerification repair: {}",
                    redact_bounded(
                        prior.to_owned(),
                        self.redactions,
                        MAX_VERIFICATION_EVIDENCE_BYTES / 2
                    ),
                    redact_bounded(
                        cause.to_owned(),
                        self.redactions,
                        MAX_VERIFICATION_EVIDENCE_BYTES / 2 - 64
                    )
                ),
                None => cause.to_owned(),
            },
            self.redactions,
            MAX_VERIFICATION_EVIDENCE_BYTES,
        )
    }

    pub fn block(&self, reason: &str, log: Option<&Path>) -> Result<KvistError> {
        let at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
        transition_locked(
            self.context,
            self.lock,
            self.task_id,
            TaskStatus::Blocked,
            Some(reason),
            &at,
            Some(self.attempt_id),
        )
        .map_err(|recording| {
            unresolved_run_error(
                self.context,
                self.task_id,
                self.attempt_id,
                reason,
                &recording,
            )
        })?;
        Ok(blocked_run_error(
            self.context,
            self.task_id,
            self.attempt_id,
            reason,
            log,
        ))
    }

    pub fn digests(&self, prior: &str, log: &Path) -> Result<(String, String)> {
        match source_test_digests(&self.context.component_dir) {
            Ok(digests) => Ok(digests),
            Err(error) => {
                let reason = self.cause(
                    Some(prior),
                    &format!("source/test digest evidence failed: {error}"),
                );
                Err(self.block(&reason, Some(log))?)
            }
        }
    }

    pub fn completion_boundary(
        &self,
        sequence: &crate::agent::AuthoringSequence,
        prior: &str,
        log: &Path,
    ) -> Result<()> {
        if let Err(error) = sequence.check() {
            let reason = self.cause(
                Some(prior),
                &format!(
                    "aggregate sequence interrupted at final evidence boundary: {error}\n{}",
                    sequence
                        .diagnostics(crate::agent::error_stop(&error))
                        .message(),
                ),
            );
            return Err(self.block(&reason, Some(log))?);
        }
        Ok(())
    }

    pub fn decision(&self, run: &AgentRunResult, kind: TaskKind) -> Result<String> {
        let evidence_dir = if matches!(kind, TaskKind::SecurityAudit | TaskKind::ComplianceReview) {
            self.context.component_dir.join(".kvist/continuous-reviews")
        } else {
            self.context
                .component_dir
                .join(".kvist/authoring/proposals")
        };
        let summary = attention::decision(
            self.context,
            self.task_id,
            self.attempt_id,
            &run.stdout,
            &evidence_dir,
            self.redactions,
            self.lock,
        )?;
        let reason = format!(
            "one or more impactful, uncovered decisions surfaced and require a human decision before implementation continues. Human-readable summary: {}. Raw proposal evidence: {}.",
            summary.display(),
            evidence_dir.display(),
        );
        let at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
        transition_locked(
            self.context,
            self.lock,
            self.task_id,
            TaskStatus::AwaitingDecision,
            Some(&reason),
            &at,
            Some(self.attempt_id),
        )?;
        let component = decisions::component_argument(&self.context.component_path);
        Ok(format!(
            "# Decision required\n\ntask `{}` paused and transitioned to awaiting-decision. A decision is required before implementation continues.\n\nHuman-readable summary: {}\n\nOpen this Markdown file for alternatives, recommendations and response instructions.\nRaw proposal evidence: {}.\nLogs written to: {}\nNext Step: use `kvist decision {component} show {}` to discuss and stage the complete resolution; inspect its exact approval command before applying.",
            self.task_id,
            summary.display(),
            evidence_dir.display(),
            run.log_path.display(),
            self.task_id,
        ))
    }
}

pub(super) fn source_test_digests(component: &Path) -> Result<(String, String)> {
    Ok((
        digest_path_tree(&component.join("src"))?,
        digest_path_tree(&component.join("tests"))?,
    ))
}

pub(super) fn verification_cause(result: &VerificationResult) -> String {
    format!(
        "test-command verification failed. Command: '{}', Exit code: {:?}.\n---\nStdout:\n{}\n---\nStderr:\n{}",
        result.command, result.exit_code, result.stdout, result.stderr,
    )
}

pub(super) fn repairable(result: &VerificationResult) -> bool {
    !result.success
        && !result.timed_out
        && !result.output_limit_exceeded
        && !result.cancelled
        && result.exit_code.is_some_and(|code| code != 0)
        && (!result.stdout.trim().is_empty() || !result.stderr.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_known_completed_verification_failure_authorizes_repair() {
        let result = VerificationResult {
            success: false,
            command: "fixture".into(),
            exit_code: Some(1),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
            stdout: "failure diagnostics".into(),
            stderr: String::new(),
        };
        assert!(repairable(&result));
        for unsafe_outcome in [
            VerificationResult {
                cancelled: true,
                ..result.clone()
            },
            VerificationResult {
                timed_out: true,
                ..result.clone()
            },
            VerificationResult {
                output_limit_exceeded: true,
                ..result.clone()
            },
            VerificationResult {
                exit_code: None,
                ..result.clone()
            },
            VerificationResult {
                exit_code: Some(0),
                ..result.clone()
            },
            VerificationResult {
                success: true,
                ..result.clone()
            },
        ] {
            assert!(!repairable(&unsafe_outcome));
        }
    }
}
