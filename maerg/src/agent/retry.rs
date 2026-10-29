use std::{
    io,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use sav::{CancellationToken, Error as ModelError, runtime::RetryPolicy};
use serde_json::json;

use super::{LiveRun, TurnBudget, redact_bounded};
use crate::{KvistError, Result};

pub(super) struct ModelRetry<'a> {
    pub policy: RetryPolicy,
    pub budget: &'a TurnBudget,
    pub cancellation: &'a CancellationToken,
    pub live: Option<&'a LiveRun>,
    pub phase: &'static str,
    pub model: &'a str,
    pub endpoint: &'a str,
    pub redactions: &'a [String],
}

impl ModelRetry<'_> {
    pub fn run<T>(
        &self,
        attempts: &mut u32,
        mut operation: impl FnMut() -> (Result<T>, bool),
    ) -> Result<T> {
        let mut last_failure = None;
        loop {
            if self.cancellation.is_cancelled() {
                return Err(KvistError::Sav(ModelError::ModelTransportCancelled));
            }
            if self.budget.remaining().is_zero() {
                return match last_failure {
                    Some(source) => {
                        self.stopped(*attempts, "approved task wall budget exhausted", source)
                    }
                    None => Err(KvistError::Sav(ModelError::ModelTransportTimedOut)),
                };
            }
            if *attempts >= self.policy.max_attempts {
                return Err(sav::runtime::Error::RunBudget {
                    reason:
                        "model generation attempt limit exhausted, including steering interruptions"
                            .into(),
                }
                .into());
            }
            *attempts += 1;
            self.record(json!({"type":"model_attempt","data":{
                "phase":self.phase,"model":self.model,"endpoint":self.endpoint,
                "attempt":*attempts,"max_attempts":self.policy.max_attempts
            }}))
            .map_err(|error| match last_failure.as_ref() {
                Some(source) => self.failure_context(source, error),
                None => error,
            })?;
            let (result, published) = operation();
            let source = match result {
                Ok(value) => return Ok(value),
                Err(KvistError::Sav(source)) => source,
                Err(error) => return Err(error),
            };
            if self.cancellation.is_cancelled()
                || matches!(
                    source,
                    ModelError::Cancelled | ModelError::ModelTransportCancelled
                )
            {
                return Err(KvistError::Sav(ModelError::ModelTransportCancelled));
            }
            let stop = if !super::is_retryable_transport_error(&source) {
                Some("non-transient failure; retry refused")
            } else if self.budget.remaining().is_zero() {
                Some("approved task wall budget exhausted")
            } else if *attempts >= self.policy.max_attempts {
                Some("configured attempt limit exhausted")
            } else {
                None
            };
            if let Some(stop) = stop {
                return self.stopped(*attempts, stop, source);
            }
            let entropy = getrandom::u64().map_err(|error| {
                self.failure_context(
                    &source,
                    KvistError::Io {
                        operation: "obtain model retry jitter",
                        path: PathBuf::from("OS random source"),
                        source: io::Error::other(error),
                    },
                )
            })?;
            let delay = jittered_delay(self.policy.backoff_delay(*attempts + 1), entropy)
                .min(self.budget.remaining());
            let cause = redact_bounded(&source.to_string(), self.redactions, 1024);
            if published {
                self.record(json!({"type":"provisional_output_discarded","data":{
                    "phase":self.phase,"model":self.model,"attempt":*attempts,
                    "reason":cause,"proposals_executed":false
                }}))
                .map_err(|error| self.failure_context(&source, error))?;
                eprintln!(
                    "Discarding provisional streamed progress from the failed model attempt; no proposals from that incomplete response were executed."
                );
            }
            self.record(json!({"type":"model_retry","data":{
                "phase":self.phase,"model":self.model,"endpoint":self.endpoint,
                "failed_attempt":*attempts,"next_attempt":*attempts+1,
                "max_attempts":self.policy.max_attempts,"cause":cause,
                "delay_ms":delay.as_millis(),"remaining_ms":self.budget.remaining().as_millis()
            }}))
            .map_err(|error| self.failure_context(&source, error))?;
            eprintln!(
                "{}",
                redact_bounded(
                    &format!(
                        "Transient model failure during {} for `{}` at {} (attempt {}/{}): {}. Retrying automatically in {:.3}s; {:.1}s task budget remains.",
                        self.phase,
                        self.model,
                        self.endpoint,
                        attempts,
                        self.policy.max_attempts,
                        cause,
                        delay.as_secs_f64(),
                        self.budget.remaining().as_secs_f64(),
                    ),
                    self.redactions,
                    4096
                )
            );
            let until = Instant::now() + delay;
            while Instant::now() < until && !self.cancellation.is_cancelled() {
                thread::sleep(
                    Duration::from_millis(10).min(until.saturating_duration_since(Instant::now())),
                );
            }
            last_failure = Some(source);
        }
    }

    fn record(&self, event: serde_json::Value) -> Result<()> {
        if let Some(live) = self.live {
            live.record(&event, true)?;
        }
        Ok(())
    }

    fn failure_context(&self, source: &ModelError, error: KvistError) -> KvistError {
        KvistError::AgentEvidenceFailed {
            reason: redact_bounded(
                &format!(
                    "{} for model `{}` at {} failed: {source}; required retry preparation/recording also failed: {error}",
                    self.phase, self.model, self.endpoint
                ),
                self.redactions,
                4096,
            ),
        }
    }

    fn stopped<T>(&self, attempts: u32, stop: &str, source: ModelError) -> Result<T> {
        let reason = format!(
            "{} for model `{}` at {} stopped after {}/{} attempt(s), {:.3}s elapsed: {}",
            self.phase,
            self.model,
            self.endpoint,
            attempts,
            self.policy.max_attempts,
            self.budget.started.elapsed().as_secs_f64(),
            stop,
        );
        self.record(json!({"type":"model_request_stopped","data":{
            "phase":self.phase,"model":self.model,"endpoint":self.endpoint,
            "attempts":attempts,"max_attempts":self.policy.max_attempts,"reason":stop,
            "cause":redact_bounded(&source.to_string(),self.redactions,1024)
        }}))
        .map_err(|error| self.failure_context(&source, error))?;
        Err(KvistError::AgentModelRequestFailed { reason, source })
    }
}

fn jittered_delay(ceiling: Duration, entropy: u64) -> Duration {
    let floor = ceiling / 2;
    floor
        + ceiling
            .saturating_sub(floor)
            .mul_f64(entropy as f64 / u64::MAX as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sav::observation::JournalFollower;

    #[test]
    fn jitter_stays_in_upper_half_of_each_exponential_ceiling() {
        let policy = RetryPolicy::new(8, Duration::from_secs(2), Duration::from_secs(30));
        for attempt in 2..=10 {
            let ceiling = policy.backoff_delay(attempt);
            for entropy in [0, 1, u64::MAX / 2, u64::MAX] {
                let delay = jittered_delay(ceiling, entropy);
                assert!(delay >= ceiling / 2 && delay <= ceiling);
            }
            assert_eq!(jittered_delay(ceiling, 0), ceiling / 2);
            assert_eq!(jittered_delay(ceiling, u64::MAX), ceiling);
        }
    }

    #[test]
    fn discovery_honors_full_attempt_policy_and_records_redacted_retries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("retry.events.jsonl");
        let redactions = vec!["private-model".into()];
        let live = LiveRun::create(&path, "retry", "task", "private-model", &redactions).unwrap();
        let budget = TurnBudget::new(Duration::from_secs(5));
        let cancellation = CancellationToken::new();
        let retry = ModelRetry {
            policy: RetryPolicy::new(5, Duration::from_millis(1), Duration::from_millis(2)),
            budget: &budget,
            cancellation: &cancellation,
            live: Some(&live),
            phase: "capacity discovery",
            model: "private-model",
            endpoint: "http://127.0.0.1:9931",
            redactions: &redactions,
        };
        let mut attempts = 0;
        let mut requests = 0;
        retry
            .run(&mut attempts, || {
                requests += 1;
                if requests < 5 {
                    (
                        Err(KvistError::Sav(ModelError::ModelProviderStatus {
                            status: 503,
                        })),
                        false,
                    )
                } else {
                    (Ok(32768), false)
                }
            })
            .unwrap();
        assert_eq!(attempts, 5);
        live.finish("completed").unwrap();
        let mut follower = JournalFollower::open(&path).unwrap();
        let events = follower.poll().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event["type"] == "model_retry")
                .count(),
            4
        );
        assert!(
            !std::fs::read_to_string(path)
                .unwrap()
                .contains("private-model")
        );
    }

    #[test]
    fn cancellation_during_backoff_prevents_another_request() {
        let cancellation = CancellationToken::new();
        let budget = TurnBudget::new(Duration::from_secs(5));
        let retry = ModelRetry {
            policy: RetryPolicy::default(),
            budget: &budget,
            cancellation: &cancellation,
            live: None,
            phase: "inference",
            model: "fixture",
            endpoint: "loopback",
            redactions: &[],
        };
        thread::scope(|scope| {
            let (sent, received) = std::sync::mpsc::channel();
            let cancellation = &cancellation;
            scope.spawn(move || {
                received.recv().unwrap();
                thread::sleep(Duration::from_millis(50));
                cancellation.cancel();
            });
            let mut attempts = 0;
            let started = Instant::now();
            let result: Result<()> = retry.run(&mut attempts, || {
                sent.send(()).unwrap();
                (
                    Err(KvistError::Sav(ModelError::ModelProviderStatus {
                        status: 503,
                    })),
                    false,
                )
            });
            assert!(matches!(
                result,
                Err(KvistError::Sav(ModelError::ModelTransportCancelled))
            ));
            assert_eq!(attempts, 1);
            assert!(started.elapsed() < Duration::from_secs(1));
        });
    }

    #[test]
    fn exhausted_wall_budget_retains_last_cause_and_stops_before_another_request() {
        let budget = TurnBudget::new(Duration::from_millis(100));
        let cancellation = CancellationToken::new();
        let retry = ModelRetry {
            policy: RetryPolicy::default(),
            budget: &budget,
            cancellation: &cancellation,
            live: None,
            phase: "capacity discovery",
            model: "fixture",
            endpoint: "http://127.0.0.1:9931",
            redactions: &[],
        };
        let mut attempts = 0;
        let result: Result<()> = retry.run(&mut attempts, || {
            (
                Err(KvistError::Sav(ModelError::ModelProviderStatus {
                    status: 503,
                })),
                false,
            )
        });
        let error = result.unwrap_err().to_string();
        assert_eq!(attempts, 1);
        assert!(
            error.contains("approved task wall budget exhausted"),
            "{error}"
        );
        assert!(error.contains("HTTP status 503"), "{error}");
        assert!(error.contains("capacity discovery"), "{error}");
        assert!(error.contains("http://127.0.0.1:9931"), "{error}");
    }

    #[test]
    fn recording_failure_prevents_provider_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let live = LiveRun::create(
            &dir.path().join("run.events.jsonl"),
            "retry",
            "task",
            "fixture",
            &[],
        )
        .unwrap();
        live.finish("failed").unwrap();
        let budget = TurnBudget::new(Duration::from_secs(5));
        let cancellation = CancellationToken::new();
        let retry = ModelRetry {
            policy: RetryPolicy::default(),
            budget: &budget,
            cancellation: &cancellation,
            live: Some(&live),
            phase: "inference",
            model: "fixture",
            endpoint: "loopback",
            redactions: &[],
        };
        let mut dispatched = false;
        let result = retry.run(&mut 0, || {
            dispatched = true;
            (Ok(()), false)
        });
        assert!(result.is_err());
        assert!(!dispatched);
    }

    #[test]
    fn recording_failure_after_a_model_error_preserves_both_causes() {
        for (status, published) in [(400, false), (503, false), (503, true)] {
            let dir = tempfile::tempdir().unwrap();
            let live = LiveRun::create(
                &dir.path().join("run.events.jsonl"),
                "retry",
                "task",
                "fixture",
                &[],
            )
            .unwrap();
            let budget = TurnBudget::new(Duration::from_secs(5));
            let cancellation = CancellationToken::new();
            let retry = ModelRetry {
                policy: RetryPolicy::default(),
                budget: &budget,
                cancellation: &cancellation,
                live: Some(&live),
                phase: "inference",
                model: "fixture",
                endpoint: "loopback",
                redactions: &[],
            };
            let mut attempts = 0;
            let result: Result<()> = retry.run(&mut attempts, || {
                live.finish("failed").unwrap();
                (
                    Err(KvistError::Sav(ModelError::ModelProviderStatus { status })),
                    published,
                )
            });
            let error = result.unwrap_err().to_string();
            assert!(error.contains(&format!("HTTP status {status}")), "{error}");
            assert!(error.contains("record"), "{error}");
            assert_eq!(attempts, 1);
        }
    }
}
