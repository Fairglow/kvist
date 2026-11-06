//! Bounded native model/tool loop. All effects are delegated to host interfaces.
use super::session::*;
use super::{ControlMailbox, GuidanceNotice, GuidanceState};
use super::{Error, Result, ToolOutcome, context::ContextManager, retry::RetryPolicy};
use crate::loop_detection::{
    ProgressEvidence, RecoveryBudget, StreamLoopDetector, ToolLoopTracker,
};
use crate::{CancellationToken, ModelRequest, ModelStreamEvent, ModelTransport, ModelTurn};
use std::{
    collections::VecDeque,
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};

#[cfg(test)]
#[path = "accounting_tests.rs"]
mod accounting_tests;

fn control_notice<S: EventSink>(
    sink: &S,
    recorder: &mut Option<&mut dyn Recorder>,
    notice: GuidanceNotice,
) -> Result<()> {
    if let Some(recorder) = recorder.as_mut() {
        recorder.control(&notice)?;
    }
    sink.send(Event::Control(notice))
}

fn deliver_guidance<S: EventSink>(
    session: &mut AgentSession,
    sink: &S,
    recorder: &mut Option<&mut dyn Recorder>,
    controls: &ControlMailbox,
    cancellation: &CancellationToken,
) -> Result<bool> {
    let mut any_delivered = false;
    for id in controls.withdrawals()? {
        control_notice(
            sink,
            recorder,
            GuidanceNotice {
                id,
                state: GuidanceState::Withdrawn,
            },
        )?;
    }
    for message in controls.claim()? {
        control_notice(
            sink,
            recorder,
            GuidanceNotice {
                id: message.id,
                state: GuidanceState::Accepted,
            },
        )?;
        control_notice(
            sink,
            recorder,
            GuidanceNotice {
                id: message.id,
                state: GuidanceState::Claimed,
            },
        )?;
        if controls.is_shutdown() || cancellation.is_cancelled() {
            control_notice(
                sink,
                recorder,
                GuidanceNotice {
                    id: message.id,
                    state: GuidanceState::Dropped,
                },
            )?;
            continue;
        }
        if let Some(recorder) = recorder.as_mut() {
            recorder.on_prompt(&message.text)?;
        }
        let delivered = controls.deliver_if_open(|| {
            if !cancellation.is_cancelled() {
                session.push_user(message.text);
            }
        })? && !cancellation.is_cancelled();
        any_delivered |= delivered;
        // No subsequent provider request or effect may precede durable delivery.
        control_notice(
            sink,
            recorder,
            GuidanceNotice {
                id: message.id,
                state: if delivered {
                    GuidanceState::Delivered
                } else {
                    GuidanceState::Dropped
                },
            },
        )?;
    }
    Ok(any_delivered)
}

/// Mirrors cancellation into a fresh operation token, never resetting the
/// whole-prompt token. Polling works even when the provider emits no events.
pub struct Operation {
    token: CancellationToken,
    stop: Option<std::sync::mpsc::Sender<()>>,
    watcher: Option<std::thread::JoinHandle<()>>,
    steered: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Operation {
    pub fn start(cancellation: &CancellationToken, controls: &ControlMailbox) -> Result<Self> {
        let token = CancellationToken::new();
        let operation_token = token.clone();
        let prompt = cancellation.clone();
        let controls = controls.clone();
        let steered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_steered = steered.clone();
        let (stop, receiver) = std::sync::mpsc::channel();
        let watcher = thread::Builder::new()
            .name("model-operation".into())
            .spawn(move || {
                loop {
                    if prompt.is_cancelled() || controls.is_shutdown() || controls.has_pending() {
                        thread_steered.store(controls.has_pending(), Ordering::SeqCst);
                        operation_token.cancel();
                        break;
                    }
                    if !matches!(
                        receiver.recv_timeout(Duration::from_millis(10)),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    ) {
                        break;
                    }
                }
            })?;
        Ok(Self {
            token,
            stop: Some(stop),
            watcher: Some(watcher),
            steered,
        })
    }

    /// Fresh provider-operation token; never pass it to an executing effect.
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// Whether guidance interrupted this operation, even if then withdrawn.
    pub fn was_steered(&self) -> bool {
        self.steered.load(Ordering::SeqCst)
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(watcher) = self.watcher.take()
            && watcher.join().is_err()
        {
            tracing::error!("model operation watcher failed");
        }
    }
}
/// The multi-turn loop driver.
pub struct NativeRuntime {
    pub max_turns: Option<u32>,
    /// How to retry a turn that ends in a transient, recoverable failure.
    pub retry: RetryPolicy,
    /// Limits shared by the complete prompt rather than reset on retries.
    pub limits: RunLimits,
    generation_loop_detection: bool,
}

impl Default for NativeRuntime {
    fn default() -> Self {
        Self::with_retry(None, RetryPolicy::default())
    }
}

impl NativeRuntime {
    /// Creates a loop driver with an explicit retry policy.
    pub fn with_retry(max_turns: impl Into<Option<u32>>, retry: RetryPolicy) -> Self {
        NativeRuntime {
            max_turns: max_turns.into(),
            retry,
            limits: RunLimits::default(),
            generation_loop_detection: true,
        }
    }

    /// Explicit host exception for intentionally repetitive output.
    /// Tool protection, authority, recording and resource limits remain active.
    pub fn with_generation_loop_detection(mut self, enabled: bool) -> Self {
        self.generation_loop_detection = enabled;
        self
    }

    /// Sets validated whole-prompt limits.
    pub fn with_limits(mut self, limits: RunLimits) -> Result<Self> {
        self.limits = limits.validate()?;
        Ok(self)
    }
}

impl NativeRuntime {
    /// Runs the loop to completion, streaming [`Event`]s through `sink`.
    ///
    /// Each parameter is a distinct collaborator the loop needs: `session` (the
    /// conversation), `transport` (the model), `executor` (sandbox tools),
    /// `sink` (events to the UI), `cancellation` (cooperative interrupt),
    /// `context` (rolling compaction bound), and an optional `recorder` that
    /// captures the durable record (reasoning included) for later inspection.
    /// Compaction only ever trims the model context; the recorder is the
    /// durable source of truth.
    #[allow(clippy::too_many_arguments)]
    pub fn run<M, E, S>(
        &self,
        session: &mut AgentSession,
        transport: &M,
        executor: &E,
        sink: &S,
        cancellation: &CancellationToken,
        context: &mut ContextManager,
        recorder: Option<&mut dyn Recorder>,
    ) -> Result<RunSummary>
    where
        M: ModelTransport,
        E: ToolExecutor,
        S: EventSink,
    {
        self.run_controlled(
            session,
            transport,
            executor,
            sink,
            cancellation,
            context,
            recorder,
            &ControlMailbox::default(),
        )
    }

    /// Runs with a bounded in-process steering capability. Guidance never
    /// resets prompt cancellation or whole-task wall/token accounting.
    #[allow(clippy::too_many_arguments)]
    pub fn run_controlled<M: ModelTransport, E: ToolExecutor, S: EventSink>(
        &self,
        session: &mut AgentSession,
        transport: &M,
        executor: &E,
        sink: &S,
        cancellation: &CancellationToken,
        context: &mut ContextManager,
        mut recorder: Option<&mut dyn Recorder>,
        controls: &ControlMailbox,
    ) -> Result<RunSummary> {
        self.limits.validate()?;
        if self.max_turns == Some(0) {
            return Err(Error::Config {
                path: None,
                reason: "turn limit must be positive when supplied".into(),
            });
        }
        let budget = PromptBudget::start(self.limits.wall_time, cancellation)?;
        controls.begin_run()?;
        let mut summary = RunSummary::default();
        session.answer = None;
        let mut result = (|| {
            if let Some(recorder) = recorder.as_mut() {
                recorder.session_start()?;
            }
            self.run_inner(
                session,
                transport,
                executor,
                sink,
                cancellation,
                context,
                &mut recorder,
                &budget,
                &mut summary,
                controls,
            )
        })();
        // Always attempt the terminal record, including when a final control
        // notice fails; never let cleanup notices enable another task/effect.
        let cleanup = (|| {
            let withdrawals = controls.withdrawals()?;
            let dropped = controls.end_run()?;
            for id in withdrawals {
                control_notice(
                    sink,
                    &mut recorder,
                    GuidanceNotice {
                        id,
                        state: GuidanceState::Withdrawn,
                    },
                )?;
            }
            for id in dropped {
                control_notice(
                    sink,
                    &mut recorder,
                    GuidanceNotice {
                        id,
                        state: GuidanceState::Dropped,
                    },
                )?;
            }
            Ok(())
        })();
        if let Err(error) = cleanup {
            result = Err(error);
        }
        summary.budget_exhausted |= budget.expired.load(Ordering::SeqCst)
            || budget.remaining().is_zero()
            || matches!(result, Err(Error::RunBudget { .. }));
        summary.cancelled |= cancellation.is_cancelled() && !summary.budget_exhausted;
        if let Err(error) = &result {
            summary.failure = Some(error.describe());
        }
        if summary.cancelled || summary.budget_exhausted || summary.failure.is_some() {
            summary.answer = None;
            session.answer = None;
        }
        if let Some(recorder) = recorder.as_mut()
            && let Err(error) = recorder.session_finish(&summary)
        {
            return Err(Error::Recording {
                reason: match &result {
                    Ok(()) => error.describe(),
                    Err(original) => format!(
                        "{}; terminal record: {}",
                        original.describe(),
                        error.describe()
                    ),
                },
            });
        }
        result?;
        let terminal = if let Some(failure) = &summary.failure {
            Event::Failed(failure.clone())
        } else if let Some(answer) = &summary.answer {
            Event::Finished {
                message: answer.clone(),
            }
        } else {
            Event::PromptEnd {
                exhausted: summary.exhausted || summary.budget_exhausted,
                cancelled: summary.cancelled,
            }
        };
        sink.send(terminal)?;
        Ok(summary)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_inner<M: ModelTransport, E: ToolExecutor, S: EventSink>(
        &self,
        session: &mut AgentSession,
        transport: &M,
        executor: &E,
        sink: &S,
        cancellation: &CancellationToken,
        context: &mut ContextManager,
        recorder: &mut Option<&mut dyn Recorder>,
        budget: &PromptBudget,
        summary: &mut RunSummary,
        controls: &ControlMailbox,
    ) -> Result<()> {
        let started_at = budget.started_at;
        let mut usage = AttemptUsage::default();
        let mut generation_seconds = 0.0_f64;
        let mut tokens_remaining = self.limits.max_tokens;
        let mut actions = ToolLoopTracker::default();
        let mut continuity = self
            .generation_loop_detection
            .then(StreamLoopDetector::default);
        let mut generation_recovery = RecoveryBudget::default();
        let mut read_pages = VecDeque::new();
        let mut next_attempt = 1;
        let mut next_reserve = self.limits.response_reserve;
        let mut unattended_turns = 0u32;
        loop {
            if controls.is_shutdown() {
                cancellation.cancel();
            }
            if cancellation.is_cancelled() || budget.remaining().is_zero() {
                break;
            }
            if deliver_guidance(session, sink, recorder, controls, cancellation)? {
                unattended_turns = 0;
                next_attempt = 1;
            }
            if self
                .max_turns
                .is_some_and(|limit| unattended_turns >= limit)
            {
                summary.exhausted = true;
                record_notice(sink, recorder, "unattended turn limit reached; send a follow-up to continue with a fresh allowance".into())?;
                break;
            }
            if next_attempt > self.retry.max_attempts {
                return Err(Error::RunBudget {
                    reason: "generation attempt limit after guidance".into(),
                });
            }
            let Some((request, compaction)) = session.prepare_request(context, next_reserve)?
            else {
                break;
            };
            if let Some(compaction) = compaction {
                record_notice(
                    sink,
                    recorder,
                    format!(
                        "context compacted: {} complete groups in a lossy, non-authoritative summary",
                        compaction.compacted_turns,
                    ),
                )?;
            }
            summary.turns = summary.turns.saturating_add(1);
            unattended_turns = unattended_turns.saturating_add(1);
            sink.send(Event::TurnStart {
                model: session.model_selector().into(),
            })?;
            let driven = self.drive_turn(
                &request,
                transport,
                sink,
                cancellation,
                &LiveProgress {
                    session,
                    context,
                    tool_defs: session.tool_defs.len(),
                    started_at,
                    cumulative_input: usage.input,
                    cumulative_total: usage.total,
                    // Prior turns' cumulative output and decode seconds, so
                    // the live path's average is the whole-session average over
                    // decode time (this turn's streamed estimate is added on).
                    cumulative_output: usage.output,
                    decode_seconds: generation_seconds,
                },
                &mut generation_seconds,
                &mut usage,
                budget,
                &mut tokens_remaining,
                recorder,
                controls,
                next_attempt,
                &|intent| executor.describe(intent),
            );
            let driven = driven.and_then(|(value, seconds, index)| {
                let mut interrupted = false;
                let report = continuity.as_mut().and_then(|detector| {
                    detector.observe_turn_while(&value, &mut || {
                        let open = !cancellation.is_cancelled()
                            && !controls.is_shutdown()
                            && !controls.has_pending()
                            && !budget.remaining().is_zero();
                        interrupted |= !open;
                        open
                    })
                });
                if controls.is_shutdown() {
                    cancellation.cancel();
                }
                if cancellation.is_cancelled() {
                    return Err(crate::Error::ModelTransportCancelled.into());
                }
                if budget.remaining().is_zero() {
                    return Err(Error::RunBudget {
                        reason: "wall time".into(),
                    });
                }
                if interrupted || controls.has_pending() {
                    continuity = self
                        .generation_loop_detection
                        .then(StreamLoopDetector::default);
                    return Err(Error::Steering {
                        attempt: 1,
                        reserve: self.limits.response_reserve,
                    });
                }
                if let Some(report) = report {
                    return Err(Error::RepetitiveGeneration {
                        report,
                        usage: value.usage,
                    });
                }
                Ok((value, seconds, index))
            });
            let (turn_value, turn_index) = match driven {
                Ok((value, _, index)) => (value, index.unwrap_or(summary.turns as usize)),
                Err(Error::Steering { attempt, reserve }) => {
                    next_attempt = attempt;
                    next_reserve = reserve;
                    continue;
                }
                Err(Error::RepetitiveGeneration { report, .. }) => {
                    if controls.is_shutdown() {
                        cancellation.cancel();
                    }
                    if cancellation.is_cancelled() || budget.remaining().is_zero() {
                        break;
                    }
                    next_attempt = 1;
                    next_reserve = self.limits.response_reserve;
                    if controls.has_pending() {
                        continue;
                    }
                    let Some(step) = generation_recovery.take() else {
                        let message = report.exhausted_message();
                        record_notice(sink, recorder, message.clone())?;
                        summary.failure = Some(message);
                        break;
                    };
                    let message = report.feedback(step);
                    record_notice(sink, recorder, message.clone())?;
                    if controls.is_shutdown() {
                        cancellation.cancel();
                    }
                    if cancellation.is_cancelled() || budget.remaining().is_zero() {
                        break;
                    }
                    session.push_user(message);
                    continue;
                }
                Err(error)
                    if !matches!(
                        &error,
                        Error::ModelTransport { .. } | Error::RunBudget { .. }
                    ) =>
                {
                    return Err(error);
                }
                Err(error) => {
                    if matches!(error, Error::RunBudget { .. }) {
                        summary.budget_exhausted = true;
                    } else if !cancellation.is_cancelled() {
                        summary.failure = Some(error.describe());
                    }
                    break;
                }
            };
            next_attempt = 1;
            next_reserve = self.limits.response_reserve;
            if let Err(error) = validate_turn(&turn_value) {
                summary.failure = Some(error.describe());
                break;
            }
            let intents = session.apply_assistant(turn_value);
            let terminal = intents.is_empty();
            let mut batch_report = None;
            for (index, intent) in intents.iter().enumerate() {
                if cancellation.is_cancelled()
                    || summary.failure.is_some()
                    || batch_report.is_some()
                    || controls.has_pending()
                    || controls.is_shutdown()
                {
                    for remaining in &intents[index..] {
                        fold_outcome(
                            session,
                            recorder,
                            sink,
                            executor,
                            turn_index,
                            remaining,
                            &ToolOutcome::rejected_with(
                                "not executed: prompt interrupted, superseded, or awaiting tool-loop correction",
                            ),
                        )?;
                    }
                    break;
                }
                let hash = crate::compute_action_hash(&intent.name, &intent.arguments);
                let native_read = executor.repeatable(intent);
                if native_read {
                    if read_pages.contains(&hash) {
                        if let Some(message) = executor.repeated_observation_notice(intent) {
                            record_notice(sink, recorder, message)?;
                        }
                    } else {
                        if read_pages.len() == 1024 {
                            read_pages.pop_front();
                        }
                        read_pages.push_back(hash.clone());
                    }
                }
                if let Some(report) = actions.propose(intent, native_read) {
                    batch_report = Some(report);
                    let reason = "Repeated identical tool arguments are blocked. Inspect the current state or choose a different action; filesystem change was not observed by this detector.";
                    fold_outcome(
                        session,
                        recorder,
                        sink,
                        executor,
                        turn_index,
                        intent,
                        &ToolOutcome::rejected_with(reason),
                    )?;
                    continue;
                }
                if let Some(recorder) = recorder.as_mut() {
                    recorder.tool_dispatch(turn_index, intent)?;
                }
                // Recording can block; do not dispatch a stale proposal after it.
                if !controls.dispatch_ready()? || cancellation.is_cancelled() {
                    for remaining in &intents[index..] {
                        fold_outcome(
                            session,
                            recorder,
                            sink,
                            executor,
                            turn_index,
                            remaining,
                            &ToolOutcome::rejected_with(
                                "not executed: guidance or cancellation before dispatch",
                            ),
                        )?;
                    }
                    break;
                }
                let outcome = match executor.execute(intent, cancellation) {
                    Ok(outcome) => {
                        summary.tools_executed += 1;
                        outcome
                    }
                    Err(error) => {
                        if matches!(error, Error::ToolPolicy { .. } | Error::ToolRender { .. }) {
                            record_notice(sink, recorder, error.describe())?;
                        } else {
                            summary.failure = Some(error.describe());
                        }
                        ToolOutcome::rejected_with(error.describe())
                    }
                };
                if outcome.cancelled {
                    cancellation.cancel();
                }
                fold_outcome(
                    session, recorder, sink, executor, turn_index, intent, &outcome,
                )?;
                let progress = executor.progress(intent, &outcome)?;
                if progress == ProgressEvidence::StateChanged
                    || (native_read && actions.is_new_observation(intent, &outcome))
                {
                    continuity = self
                        .generation_loop_detection
                        .then(StreamLoopDetector::default);
                }
                batch_report = actions.observe(intent, &outcome, native_read, progress);
            }
            self.emit_progress(
                sink,
                session,
                context,
                usage.input,
                usage.output,
                usage.total,
                usage.provenance(),
                started_at,
                generation_seconds,
            )?;
            usage.dirty = false;
            if let Some(report) = batch_report {
                if summary.failure.is_some() {
                    break;
                }
                if controls.is_shutdown() {
                    cancellation.cancel();
                }
                if cancellation.is_cancelled() || budget.remaining().is_zero() {
                    break;
                }
                if controls.has_pending() {
                    continue;
                }
                let Some(step) = generation_recovery.take() else {
                    let message = report.exhausted_message();
                    record_notice(sink, recorder, message.clone())?;
                    summary.failure = Some(message);
                    break;
                };
                let message = report.feedback(step);
                record_notice(sink, recorder, message.clone())?;
                if controls.is_shutdown() {
                    cancellation.cancel();
                }
                if cancellation.is_cancelled() || budget.remaining().is_zero() {
                    break;
                }
                session.push_user(message);
                continue;
            }
            if terminal
                && summary.failure.is_none()
                && !cancellation.is_cancelled()
                && !controls.finish_if_idle()?
            {
                session.answer = None;
                continue;
            }
            if terminal || summary.failure.is_some() || cancellation.is_cancelled() {
                break;
            }
        }
        if usage.dirty {
            self.emit_progress(
                sink,
                session,
                context,
                usage.input,
                usage.output,
                usage.total,
                usage.provenance(),
                started_at,
                generation_seconds,
            )?;
        }
        summary.answer = session.answer.clone();
        Ok(())
    }

    /// Streams one turn, retrying transient failures with bounded backoff.
    ///
    /// The loop reuses the same [`ModelRequest`] (rebuildable from the session,
    /// which excludes any partial output of a failed attempt) across attempts so
    /// a retry replays the turn from a clean state. Each attempt is a fresh
    /// transport call, so it also gets a fresh per-turn deadline: a turn that
    /// merely ran past its deadline once can finish on the next attempt.
    ///
    ///
    /// While a turn streams, a live progress update is emitted at a bounded rate
    /// so the stats bar tracks working speed, context, and elapsed time as the
    /// model generates, not only after the turn ends.
    ///
    /// A failure is retried only when [`crate::Error::is_retryable`]
    /// reports it as a temporal, recoverable condition (network drop, provider
    /// timeout, transient server error). Cancellation is never retried, and once
    /// the policy's attempt budget is spent the final error is returned so the
    /// caller reports it. Between attempts an [`Event::Note`] is emitted so the
    /// user sees the retry is in progress; the best-effort sink send only means
    /// the UI is gone.
    #[allow(clippy::too_many_arguments)]
    fn drive_turn<M, S>(
        &self,
        request: &ModelRequest,
        transport: &M,
        sink: &S,
        cancellation: &CancellationToken,
        progress: &LiveProgress,
        total_generation_seconds: &mut f64,
        usage: &mut AttemptUsage,
        budget: &PromptBudget,
        tokens_remaining: &mut Option<u64>,
        recorder: &mut Option<&mut dyn Recorder>,
        controls: &ControlMailbox,
        first_attempt: u32,
        describe: &impl Fn(&crate::ToolIntent) -> String,
    ) -> Result<(ModelTurn, f64, Option<usize>)>
    where
        M: ModelTransport,
        S: EventSink,
    {
        // Each retryable attempt is granted a larger budget than the last so a
        // turn that merely ran past one deadline can finish once an attempt has
        // room for the whole generation. The budget grows linearly with the
        // attempt number and is capped at `base × max_attempts`, so a recoverable
        // timeout can never turn a single transport call into an unbounded wait;
        // the first attempt uses the transport's configured deadline unchanged.
        let base = transport.deadline();
        let mut attempt = first_attempt;
        let mut attempt_request = request.clone();
        let mut generation_secs = 0.0_f64;
        loop {
            let attempt_progress = LiveProgress {
                cumulative_input: usage.input,
                cumulative_output: usage.output,
                cumulative_total: usage.total,
                ..*progress
            };
            if controls.is_shutdown() {
                cancellation.cancel();
            }
            if cancellation.is_cancelled() {
                return Err(crate::Error::ModelTransportCancelled.into());
            }
            if controls.has_pending() {
                return Err(Error::Steering {
                    attempt,
                    reserve: attempt_request
                        .max_output_tokens
                        .unwrap_or(self.limits.response_reserve),
                });
            }
            if budget.remaining().is_zero() {
                return Err(Error::RunBudget {
                    reason: "wall time".into(),
                });
            }
            let reserve = attempt_request
                .max_output_tokens
                .unwrap_or(self.limits.response_reserve);
            let charge = (super::context::estimate_request(&attempt_request) as u64)
                .saturating_add(u64::from(reserve));
            if let Some(remaining) = tokens_remaining {
                if charge > *remaining {
                    return Err(Error::RunBudget {
                        reason: "estimated input/output tokens".into(),
                    });
                }
                *remaining -= charge;
            }
            let attempt_deadline = base
                .saturating_mul(attempt)
                .min(base.saturating_mul(self.retry.max_attempts))
                .min(budget.remaining());
            sink.send(Event::AttemptStart { attempt })?;
            if let Some(recorder) = recorder.as_mut() {
                recorder.request(&attempt_request, attempt)?;
            }
            if controls.is_shutdown() {
                cancellation.cancel();
            }
            if cancellation.is_cancelled() {
                return Err(crate::Error::ModelTransportCancelled.into());
            }
            let mut decode_clock = DecodeClock::default();
            // Running output estimate and last live-update time for this attempt,
            // so the stats bar refreshes at a bounded rate while the model streams
            // rather than only after the turn completes.
            let mut attempt_output_chars: u64 = 0;
            let mut last_emit = Instant::now();
            let mut sink_error = None;
            let mut event_interrupted = false;
            let mut detector = self
                .generation_loop_detection
                .then(StreamLoopDetector::default);
            let mut repetition = None;
            let mut rejected_usage = None;
            let operation = Operation::start(cancellation, controls)?;
            let result = transport.stream_with_deadline(
                &attempt_request,
                &operation.token,
                &mut |event| {
                    let observed_at = Instant::now();
                    let result = (|| {
                        event_interrupted |= controls.has_pending();
                        if cancellation.is_cancelled()
                            || event_interrupted
                            || controls.is_shutdown()
                        {
                            operation.token().cancel();
                            return Err(crate::Error::ModelTransportCancelled);
                        }
                        if sink_error.is_some() || repetition.is_some() {
                            operation.token().cancel();
                            return Err(crate::Error::ModelTransportCancelled);
                        }
                        decode_clock.observe(observed_at);
                        if let Some(recorder) = recorder.as_mut()
                            && let Err(error) = recorder.model_event(&event)
                        {
                            sink_error = Some(error);
                            operation.token().cancel();
                            return Err(crate::Error::ModelTransportCancelled);
                        }
                        let report = detector.as_mut().and_then(|detector| {
                            detector.observe_while(&event, &mut || {
                                event_interrupted |= controls.has_pending();
                                !cancellation.is_cancelled()
                                    && !event_interrupted
                                    && !controls.is_shutdown()
                                    && !operation.token().is_cancelled()
                                    && !budget.remaining().is_zero()
                            })
                        });
                        if cancellation.is_cancelled()
                            || event_interrupted
                            || controls.is_shutdown()
                            || budget.remaining().is_zero()
                            || operation.token().is_cancelled()
                        {
                            operation.token().cancel();
                            return Err(crate::Error::ModelTransportCancelled);
                        }
                        if let Some(report) = report {
                            repetition = Some(report);
                            operation.token().cancel();
                            return Err(crate::Error::ModelTransportCancelled);
                        }
                        let emitted = match event {
                            ModelStreamEvent::TextDelta(delta) => {
                                attempt_output_chars = attempt_output_chars
                                    .saturating_add(delta.chars().count() as u64);
                                sink.send(Event::Text(delta))
                            }
                            ModelStreamEvent::ReasoningDelta(delta) => {
                                attempt_output_chars = attempt_output_chars
                                    .saturating_add(delta.chars().count() as u64);
                                sink.send(Event::Reasoning(delta))
                            }
                            ModelStreamEvent::ToolIntent(intent) => {
                                let description = describe(&intent);
                                sink.send(Event::ToolCall {
                                    description,
                                    name: intent.name.clone(),
                                })
                            }
                        };
                        if let Err(error) = emitted {
                            sink_error = Some(error);
                            cancellation.cancel();
                            return Err(crate::Error::ModelTransportCancelled);
                        }
                        // Refresh the live stats bar at a bounded rate so the user
                        // sees the turn making progress while the model streams.
                        let now = observed_at;
                        if now.saturating_duration_since(last_emit) >= LIVE_PROGRESS_INTERVAL {
                            last_emit = now;
                            if let Err(error) = emit_live_progress(
                                sink,
                                &attempt_progress,
                                attempt_output_chars,
                                decode_clock.seconds(),
                                generation_secs,
                                now,
                            ) {
                                sink_error = Some(error);
                                cancellation.cancel();
                                return Err(crate::Error::ModelTransportCancelled);
                            }
                        }
                        Ok(())
                    })();
                    decode_clock.exclude_handling(observed_at.elapsed());
                    result
                },
                attempt_deadline,
            );
            let seconds = decode_clock.seconds();
            generation_secs += seconds;
            *total_generation_seconds += seconds;
            let reported_usage = result.as_ref().ok().and_then(|turn| turn.usage);
            usage.observe(reported_usage);
            let mut reported_exhaustion = false;
            if let Some(reported) = reported_usage
                && let Some(remaining) = tokens_remaining
            {
                let actual = reported
                    .total_tokens
                    .max(reported.input_tokens.saturating_add(reported.output_tokens));
                let extra = actual.saturating_sub(charge);
                reported_exhaustion = extra > *remaining;
                *remaining = remaining.saturating_sub(extra);
            }
            let interrupted = event_interrupted || operation.was_steered();
            let operation_cancelled = operation.token().is_cancelled();
            drop(operation);
            if let Some(error) = sink_error {
                return Err(error);
            }
            let turn_index = if let Ok(turn) = &result
                && let Some(recorder) = recorder.as_mut()
            {
                let index = recorder.turn_start(turn)?;
                recorder.turn_finish(index, &turn.usage, finish_reason_str(&turn.finish_reason))?;
                Some(index)
            } else {
                None
            };
            if controls.is_shutdown() {
                cancellation.cancel();
            }
            if (interrupted || controls.has_pending()) && !cancellation.is_cancelled() {
                return Err(Error::Steering {
                    attempt: attempt + 1,
                    reserve,
                });
            }
            if cancellation.is_cancelled() {
                return Err(crate::Error::ModelTransportCancelled.into());
            }
            if budget.remaining().is_zero() {
                return Err(Error::RunBudget {
                    reason: "wall time".into(),
                });
            }
            if operation_cancelled && repetition.is_none() {
                return Err(crate::Error::ModelTransportCancelled.into());
            }
            if reported_exhaustion {
                return Err(Error::RunBudget {
                    reason: "provider-reported model tokens".into(),
                });
            }
            if let Ok(turn) = &result {
                let report = repetition.or_else(|| {
                    detector.as_mut().and_then(|detector| {
                        detector.finish_while(turn, &mut || {
                            event_interrupted |= controls.has_pending();
                            !cancellation.is_cancelled()
                                && !event_interrupted
                                && !controls.is_shutdown()
                                && !budget.remaining().is_zero()
                        })
                    })
                });
                if controls.is_shutdown() {
                    cancellation.cancel();
                }
                if cancellation.is_cancelled() {
                    return Err(crate::Error::ModelTransportCancelled.into());
                }
                if budget.remaining().is_zero() {
                    return Err(Error::RunBudget {
                        reason: "wall time".into(),
                    });
                }
                if event_interrupted || controls.has_pending() {
                    return Err(Error::Steering {
                        attempt: attempt + 1,
                        reserve,
                    });
                }
                if let Some(report) = report {
                    rejected_usage = turn.usage;
                    repetition = Some(report);
                }
            }
            if let Some(report) = repetition {
                return Err(Error::RepetitiveGeneration {
                    report,
                    usage: rejected_usage,
                });
            }
            match result {
                Ok(turn)
                    if matches!(turn.finish_reason, crate::FinishReason::Length)
                        && attempt < self.retry.max_attempts
                        && !cancellation.is_cancelled() =>
                {
                    let mut enlarged = attempt_request.clone();
                    let next_reserve = reserve.saturating_mul(2).min(1_048_576);
                    enlarged.max_output_tokens = Some(next_reserve);
                    if next_reserve <= reserve
                        || super::context::estimate_request(&enlarged)
                            .saturating_add(next_reserve as usize)
                            > progress.context.limit_tokens()
                    {
                        return Ok((turn, generation_secs, turn_index));
                    }
                    record_notice(
                        sink,
                        recorder,
                        format!(
                            "model generation ended with length; regenerating {}/{}, reserving {} output tokens before any effects; earlier text is provisional",
                            attempt + 1,
                            self.retry.max_attempts,
                            next_reserve,
                        ),
                    )?;
                    attempt_request = enlarged;
                    attempt += 1;
                }
                Ok(turn) => return Ok((turn, generation_secs, turn_index)),
                // Retry only temporal, recoverable failures, and only while the
                // attempt budget remains and the turn was not cancelled.
                Err(error) if error.is_retryable() => {
                    if attempt >= self.retry.max_attempts || cancellation.is_cancelled() {
                        return Err(error.into());
                    }
                    let next = attempt + 1;
                    let delay = self.retry.backoff_delay(next).min(budget.remaining());
                    let delay_secs = delay.as_secs_f64();
                    let next_budget_secs = base
                        .saturating_mul(next)
                        .min(budget.remaining())
                        .as_secs_f64();
                    // Show the turn is recovering and that a longer budget is
                    // being granted, not just another identical try.
                    record_notice(
                        sink,
                        recorder,
                        format!(
                            "model request failed ({error}); retrying {next}/{max} in {delay_secs:.1}s with at most {next_budget_secs:.0}s; earlier streamed text is provisional",
                            max = self.retry.max_attempts,
                        ),
                    )?;
                    attempt = next;
                    let wake_at = Instant::now() + delay;
                    while Instant::now() < wake_at {
                        if cancellation.is_cancelled() {
                            return Err(crate::Error::ModelTransportCancelled.into());
                        }
                        if controls.has_pending() {
                            return Err(Error::Steering { attempt, reserve });
                        }
                        if budget.remaining().is_zero() {
                            return Err(Error::RunBudget {
                                reason: "wall time during retry".into(),
                            });
                        }
                        thread::sleep(
                            wake_at
                                .saturating_duration_since(Instant::now())
                                .min(Duration::from_millis(25)),
                        );
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Emits a single accounting event so the UI can update the speed stat, the
    /// context bargraph, and the compaction progress bar. A closed sink stops work.
    #[allow(clippy::too_many_arguments)]
    fn emit_progress<S: EventSink>(
        &self,
        sink: &S,
        session: &AgentSession,
        context: &ContextManager,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
        token_accounting: TokenAccounting,
        started_at: Instant,
        generation_seconds: f64,
    ) -> Result<()> {
        // Whole-session average over decode time: output tokens from every
        // turn so far over their combined decode seconds, on the same basis as
        // the live `t/s`, so the two are comparable (prompt tokens and
        // inter-turn gaps excluded).
        let average = token_rate(output_tokens, generation_seconds);
        let context_tokens = session.estimate_context_tokens(session.tool_definitions().len());
        sink.send(Event::Progress {
            token_accounting,
            input_tokens,
            output_tokens,
            context_tokens,
            context_limit: context.limit_tokens(),
            context_utilization: session.utilization(context, session.tool_definitions().len()),
            compaction_progress: session
                .compaction_progress(context, session.tool_definitions().len()),
            compactions: context.compactions(),
            generation_tokens_per_sec: average,
            average_tokens_per_sec: average,
            total_tokens,
            elapsed_secs: started_at.elapsed().as_secs_f64().max(1e-9),
        })
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
