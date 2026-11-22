//! Brokered tool and provider mediation tests.
//!
//! These tests pin the complete-mediation contract the host broker consumes,
//! independently of any provider: a tool or provider action is usable only
//! because the host explicitly enabled its capability and supplied its grant;
//! canonical arguments and resources are bound to the approved identity, not
//! the model's; the provider network path is loopback-only; credentials are
//! references, never inline; idempotency evidence is host-supplied, not
//! self-certified; retries are budgeted and side-effect-aware; and runtime
//! events are bounded and redacted before they reach a durable record.
//!
//! The runtime never invents a grant, a destination, or a retry. Every
//! rejection below is a typed, not-executed refusal that fails closed before
//! any effect.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sav::loop_detection::{ProgressEvidence, ToolLoopKind, ToolLoopTracker};
use sav::registry::{Backend, ExternalAgent, ExternalProvider, Registry};
use sav::runtime::{
    AgentSession, CapabilityDescriptor, CapabilityState, ContextManager, EffectCertainty,
    ModelIdentity, NativeRuntime, RecoveryAction, RedactedEvent, RetryPolicy, StopReason,
    ToolExecutor, ToolFailure, ToolFailureCategory, ToolOutcome, redact_event,
};
use sav::{
    CancellationToken, DirectModelTransport, FinishReason, LocalModelProvider, ModelRequest,
    ModelStreamEvent, ModelTransport, ModelTurn, ReasoningEffort, ToolDefinition, ToolIntent,
};

/// A scripted model that proposes one tool intent, then answers and stops.
struct ScriptedModel {
    calls: AtomicUsize,
}

impl ModelTransport for ScriptedModel {
    fn complete(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
    ) -> sav::Result<ModelTurn> {
        unreachable!("the loop streams turns")
    }

    fn stream(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
        _on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(ModelTurn {
                text: String::new(),
                reasoning: None,
                tool_intents: vec![ToolIntent {
                    id: "call-1".into(),
                    provider_id: None,
                    name: "probe".into(),
                    arguments: serde_json::json!({"path": 0}),
                }],
                finish_reason: FinishReason::ToolCalls,
                provider: LocalModelProvider::LlamaServer,
                model: "native".into(),
                response_id: None,
                provider_request_id: None,
                usage: None,
            }),
            _ => Ok(ModelTurn {
                text: "done".into(),
                reasoning: None,
                tool_intents: vec![],
                finish_reason: FinishReason::Stop,
                provider: LocalModelProvider::LlamaServer,
                model: "native".into(),
                response_id: None,
                provider_request_id: None,
                usage: None,
            }),
        }
    }

    fn deadline(&self) -> Duration {
        Duration::from_secs(30)
    }
}

/// A sink that records every event so a test can inspect the tool outcome.
#[derive(Default)]
struct CollectingSink {
    events: std::sync::Mutex<Vec<sav::runtime::Event>>,
}

impl sav::runtime::EventSink for CollectingSink {
    fn send(&self, event: sav::runtime::Event) -> sav::runtime::Result<()> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

/// A recording executor that consumes the host's typed grant before any effect.
/// It counts every effect it actually causes and records the canonical action
/// hash of each intent it dispatched, so a test can prove the runtime bound the
/// effect to the approved identity and never invented one.
#[derive(Default)]
struct GrantExecutor {
    granted: bool,
    effects: AtomicUsize,
    dispatched: std::sync::Mutex<Vec<sav::ActionRecord>>,
}

impl ToolExecutor for GrantExecutor {
    fn execute(
        &self,
        intent: &ToolIntent,
        _cancellation: &CancellationToken,
    ) -> sav::runtime::Result<ToolOutcome> {
        if !self.granted {
            return Err(sav::runtime::Error::ToolPolicy {
                tool: intent.name.clone(),
                reason: "the host did not grant this tool".into(),
            });
        }
        self.dispatched.lock().unwrap().push(sav::ActionRecord {
            action_hash: sav::compute_action_hash(&intent.name, &intent.arguments),
            tool_name: intent.name.clone(),
            observation_hash: None,
        });
        self.effects.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: b"ok".to_vec(),
            stderr: Vec::new(),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        })
    }
}

fn run_with_executor(
    executor: &GrantExecutor,
) -> (sav::runtime::RunSummary, Vec<sav::runtime::Event>) {
    let model = ScriptedModel {
        calls: AtomicUsize::new(0),
    };
    let mut session = AgentSession::new(
        ModelIdentity {
            id: "selected".into(),
            model: "native".into(),
        },
        ReasoningEffort::None,
        vec![ToolDefinition {
            name: "probe".into(),
            description: "Probe fixture".into(),
            parameters: serde_json::json!({"type": "object"}),
        }],
        "host instructions".into(),
    );
    session.push_user("run the probe");
    let mut context = ContextManager::new(128_000, 6);
    let cancellation = CancellationToken::new();
    let sink = CollectingSink::default();
    let summary = NativeRuntime::default()
        .run(
            &mut session,
            &model,
            executor,
            &sink,
            &cancellation,
            &mut context,
            None,
        )
        .expect("the scripted model and the executor never fail the loop");
    (summary, sink.events.lock().unwrap().clone())
}

/// A capability is usable only because the host explicitly policy-enabled it.
/// Advertised and tested tiers are evidence, not grants; an unknown capability
/// is the safe default and is never enabled.
#[test]
fn capability_grant_is_consumed_only_when_policy_enabled() {
    assert!(CapabilityState::PolicyEnabled.enabled());
    assert!(!CapabilityState::Advertised.enabled());
    assert!(!CapabilityState::Tested.enabled());

    let enabled = CapabilityDescriptor {
        name: "tools".into(),
        state: CapabilityState::PolicyEnabled,
    };
    assert!(enabled.is_enabled());

    let advertised = CapabilityDescriptor::advertised("tools");
    assert!(!advertised.is_enabled());

    // Promotion is monotonic: advertised -> tested -> policy-enabled, and
    // policy-enabled has no successor.
    assert_eq!(
        CapabilityState::Advertised.next(),
        Some(CapabilityState::Tested)
    );
    assert_eq!(
        CapabilityState::Tested.next(),
        Some(CapabilityState::PolicyEnabled)
    );
    assert_eq!(CapabilityState::PolicyEnabled.next(), None);
}

/// The runtime consumes the host's typed grant before any effect. When the host
/// has granted the tool, the effect runs and is bound to the canonical action
/// hash. When the host has not granted it, the runtime fails closed: no effect
/// is caused, the refusal is surfaced, and the tool result is reported failed.
#[test]
fn the_runtime_consumes_a_typed_host_grant_before_any_effect() {
    let executor = GrantExecutor {
        granted: true,
        ..GrantExecutor::default()
    };
    let (summary, _events) = run_with_executor(&executor);
    assert_eq!(executor.effects.load(Ordering::SeqCst), 1);
    assert_eq!(summary.tools_executed, 1);
    let dispatched = executor.dispatched.lock().unwrap();
    assert_eq!(dispatched.len(), 1);
    assert!(dispatched[0].action_hash.starts_with("sha256:"));
}

/// When the host has not granted the tool, the runtime refuses before any
/// effect: no effect is caused, the refusal is surfaced, and the tool result is
/// reported failed. The runtime never invents a grant.
#[test]
fn the_runtime_fails_closed_without_a_host_grant() {
    let executor = GrantExecutor {
        granted: false,
        ..GrantExecutor::default()
    };
    let (summary, events) = run_with_executor(&executor);
    assert_eq!(executor.effects.load(Ordering::SeqCst), 0);
    assert_eq!(summary.tools_executed, 0);
    let failure = summary.failure.as_deref().expect("the refusal is surfaced");
    assert!(
        failure.contains("not permitted"),
        "the policy refusal is surfaced: {failure}"
    );
    assert!(
        events
            .iter()
            .any(|event| { matches!(event, sav::runtime::Event::ToolResult { failed: true, .. }) }),
        "the refused tool result is reported as failed"
    );
}

/// Canonical arguments are bound to the approved identity. The action hash is
/// stable across key order (the model cannot reorder arguments to escape the
/// grant) and distinct for distinct arguments (a different action is a
/// different grant).
#[test]
fn canonical_arguments_are_bound_to_the_approved_identity() {
    let intent_a = ToolIntent {
        id: "call-a".into(),
        provider_id: None,
        name: "probe".into(),
        arguments: serde_json::json!({"path": 0, "depth": 1}),
    };
    let intent_b = ToolIntent {
        id: "call-b".into(),
        provider_id: None,
        name: "probe".into(),
        arguments: serde_json::json!({"depth": 1, "path": 0}),
    };
    let intent_c = ToolIntent {
        id: "call-c".into(),
        provider_id: None,
        name: "probe".into(),
        arguments: serde_json::json!({"path": 1, "depth": 1}),
    };

    let hash_a = sav::compute_action_hash(&intent_a.name, &intent_a.arguments);
    let hash_b = sav::compute_action_hash(&intent_b.name, &intent_b.arguments);
    let hash_c = sav::compute_action_hash(&intent_c.name, &intent_c.arguments);

    // Key order does not change the canonical identity.
    assert_eq!(hash_a, hash_b);
    // A different argument is a different identity.
    assert_ne!(hash_a, hash_c);
    // The hash is a stable sha256 digest.
    assert!(hash_a.starts_with("sha256:"));
    assert_eq!(hash_a.len(), "sha256:".len() + 64);
}

/// Canonical resources are bound to the approved mount plan. The mount-plan
/// identity is deterministic and changes when any destination, access, or
/// purpose changes. The model cannot redirect a grant to an arbitrary host
/// path or widen its access.
#[test]
fn canonical_resources_are_bound_to_the_approved_mount_plan() {
    let grants = [
        ("/usr".to_owned(), 0u8, 3u8),
        ("/workspace".to_owned(), 1, 1),
    ];
    let plan = sav::runtime::execution_backend::mount_plan_identity(&grants);
    assert!(plan.starts_with("sha256:"));
    assert_eq!(plan.len(), "sha256:".len() + 64);

    // The same plan is the same identity.
    let same = sav::runtime::execution_backend::mount_plan_identity(&grants);
    assert_eq!(plan, same);

    // A different destination is a different identity.
    let other_destination = [("/usr".to_owned(), 0u8, 3u8), ("/etc".to_owned(), 1, 1)];
    assert_ne!(
        plan,
        sav::runtime::execution_backend::mount_plan_identity(&other_destination)
    );

    // A different access is a different identity.
    let other_access = [
        ("/usr".to_owned(), 0u8, 3u8),
        ("/workspace".to_owned(), 0, 1),
    ];
    assert_ne!(
        plan,
        sav::runtime::execution_backend::mount_plan_identity(&other_access)
    );

    // A different purpose is a different identity.
    let other_purpose = [
        ("/usr".to_owned(), 0u8, 2u8),
        ("/workspace".to_owned(), 1, 1),
    ];
    assert_ne!(
        plan,
        sav::runtime::execution_backend::mount_plan_identity(&other_purpose)
    );
}

/// The provider network path is loopback-only. The direct transport accepts
/// only a bounded ASCII HTTP loopback endpoint and rejects every destination
/// that is not a numeric loopback address, including external IPs, hostnames,
/// and endpoints that embed credentials.
#[test]
fn the_provider_network_path_is_loopback_only() {
    let deadline = Duration::from_secs(30);
    let max_response_bytes = 8 * 1024 * 1024;

    // A numeric loopback endpoint is accepted.
    for endpoint in [
        "http://127.0.0.1:11434",
        "http://127.0.0.1:9931",
        "http://[::1]:11434",
    ] {
        assert!(
            DirectModelTransport::new(
                LocalModelProvider::LlamaServer,
                endpoint,
                deadline,
                max_response_bytes
            )
            .is_ok(),
            "loopback endpoint was rejected: {endpoint}"
        );
    }

    // Every non-loopback destination is rejected before any network I/O.
    for endpoint in [
        "http://8.8.8.8:11434",
        "http://localhost:11434",
        "http://example.com:11434",
        "http://token@127.0.0.1:11434",
        "https://127.0.0.1:11434",
        "http://127.0.0.1",
    ] {
        assert!(
            DirectModelTransport::new(
                LocalModelProvider::LlamaServer,
                endpoint,
                deadline,
                max_response_bytes
            )
            .is_err(),
            "non-loopback endpoint was accepted: {endpoint}"
        );
    }
}

/// Credentials are references, never inline secret material. The registry
/// rejects an external agent whose arguments carry an inline credential, but
/// accepts a reference-style argument that points at a credential file without
/// embedding the secret.
#[test]
fn credentials_are_references_never_inline() {
    // An inline credential in an external argument is rejected.
    for argument in [
        "--token=private",
        "Authorization: Bearer private",
        "--api-key",
        "--password",
    ] {
        let contents = format!(
            "schema_version = 1\n[profiles.agent.backend]\nkind = \"external-agent\"\nprovider = \"copilot\"\nexecutable = \"copilot\"\narguments = [\"{argument}\"]\n"
        );
        assert!(
            Registry::parse(Path::new("fixture"), &contents).is_err(),
            "an inline credential was accepted: {argument}"
        );
    }

    // A reference-style argument that points at a credential file, without
    // embedding the secret, is accepted.
    let reference = ExternalAgent {
        provider: ExternalProvider::Copilot,
        executable: "copilot".into(),
        arguments: vec!["--token-file".into(), "/run/secrets/token".into()],
        model: None,
    };
    let mut profile = sav::registry::Profile::new(Backend::ExternalAgent(reference));
    assert!(
        profile.validate().is_ok(),
        "a credential reference was rejected as inline material"
    );
}

/// Idempotency evidence is host-supplied, never self-certified. A successful
/// no-op output is not progress: three identical observations of a repeatable
/// action are evidence of stagnation. A host-validated state change clears the
/// recurrence history and permits the action to be observed again.
#[test]
fn idempotency_evidence_is_host_supplied_not_self_certified() {
    let intent = |index: usize| ToolIntent {
        id: format!("call-{index}"),
        provider_id: None,
        name: "probe".into(),
        arguments: serde_json::json!({"path": 0}),
    };
    let success = ToolOutcome {
        exited: true,
        status: Some(0),
        stdout: b"unchanged".to_vec(),
        stderr: Vec::new(),
        timed_out: false,
        output_limit_exceeded: false,
        cancelled: false,
    };

    let mut tracker = ToolLoopTracker::default();
    // The first two identical observations are not yet stagnation.
    assert!(
        tracker
            .observe(&intent(0), &success, true, ProgressEvidence::None)
            .is_none()
    );
    assert!(
        tracker
            .observe(&intent(1), &success, true, ProgressEvidence::None)
            .is_none()
    );
    // The third identical observation is evidence of stagnation.
    let report = tracker
        .observe(&intent(2), &success, true, ProgressEvidence::None)
        .expect("three identical observations are stagnation");
    assert_eq!(report.kind, ToolLoopKind::RepeatedObservation);

    // A host-validated state change clears the recurrence history.
    let mut tracker = ToolLoopTracker::default();
    tracker.observe(&intent(0), &success, true, ProgressEvidence::None);
    tracker.observe(&intent(1), &success, true, ProgressEvidence::None);
    let cleared = tracker.observe(&intent(2), &success, true, ProgressEvidence::StateChanged);
    assert!(
        cleared.is_none(),
        "a host-validated state change clears the recurrence history"
    );
    // After the state change, the same action is observed again without
    // triggering the breaker.
    assert!(
        tracker
            .observe(&intent(3), &success, true, ProgressEvidence::None)
            .is_none()
    );
}

/// Ambiguous outcomes are not progress. Three identical classified failures of
/// a repeatable action are evidence of stagnation, and the failure is
/// classified with effect certainty and a safe next action.
#[test]
fn ambiguous_failures_are_not_progress() {
    let intent = |index: usize| ToolIntent {
        id: format!("call-{index}"),
        provider_id: None,
        name: "probe".into(),
        arguments: serde_json::json!({"path": 0}),
    };
    let failure = ToolOutcome {
        exited: true,
        status: Some(1),
        stdout: Vec::new(),
        stderr: b"error".to_vec(),
        timed_out: false,
        output_limit_exceeded: false,
        cancelled: false,
    };

    let mut tracker = ToolLoopTracker::default();
    assert!(
        tracker
            .observe(&intent(0), &failure, true, ProgressEvidence::None)
            .is_none()
    );
    assert!(
        tracker
            .observe(&intent(1), &failure, true, ProgressEvidence::None)
            .is_none()
    );
    let report = tracker
        .observe(&intent(2), &failure, true, ProgressEvidence::None)
        .expect("three identical failures are stagnation");
    assert_eq!(report.kind, ToolLoopKind::RepeatedFailure);

    // A timeout is classified with effect certainty and a safe next action.
    let timed_out = ToolOutcome {
        exited: true,
        status: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        timed_out: true,
        output_limit_exceeded: false,
        cancelled: false,
    };
    let failure = ToolFailure::from_outcome(&timed_out).expect("a timeout is a classified failure");
    assert_eq!(failure.category, ToolFailureCategory::Timeout);
    assert_eq!(failure.effects, EffectCertainty::MayHaveEffects);
    assert_eq!(failure.recovery, RecoveryAction::ReviewLimits);
}

/// Generation retries are budgeted. The retry policy bounds the number of
/// attempts and the backoff delay grows exponentially up to a cap. The budget
/// is exhausted when the attempt count exceeds the maximum.
#[test]
fn generation_retries_are_budgeted() {
    let policy = RetryPolicy::new(3, Duration::from_secs(2), Duration::from_secs(30));
    assert_eq!(policy.max_attempts, 3);
    assert_eq!(policy.backoff_delay(2), Duration::from_secs(2));
    assert_eq!(policy.backoff_delay(3), Duration::from_secs(4));
    assert_eq!(policy.backoff_delay(4), Duration::from_secs(8));
    assert_eq!(policy.backoff_delay(5), Duration::from_secs(16));
    assert_eq!(policy.backoff_delay(6), Duration::from_secs(30));
    assert_eq!(policy.backoff_delay(100), Duration::from_secs(30));

    // A single-attempt policy has no retry.
    let single = RetryPolicy::new(1, Duration::from_secs(1), Duration::from_secs(5));
    assert_eq!(single.max_attempts, 1);
}

/// Opaque external-agent effects are not automatically retried. The supervisor
/// rejects a policy with a positive retry count before spawning, because an
/// unknown external effect must not be replayed. A zero-retry policy is
/// accepted.
#[test]
fn unsafe_retries_of_opaque_effects_are_rejected() {
    let command = sav::CommandSpec::new("/bin/true", Vec::<String>::new());
    let cancellation = CancellationToken::new();
    let mut observer = |_: sav::OutputStream, _: &[u8]| Ok(());

    // A positive retry count is rejected before spawning.
    for max_retries in [1u32, 3] {
        let policy = sav::SupervisionPolicy {
            idle_timeout: Duration::from_secs(2),
            attempt_timeout: None,
            detect_loops: false,
            max_retries,
            max_output_bytes: 64 * 1024,
        };
        assert!(
            sav::run_supervised_observed(&policy, &command, &cancellation, &mut observer).is_err(),
            "a positive retry count was accepted for an opaque effect"
        );
    }

    // A zero-retry policy is accepted.
    let policy = sav::SupervisionPolicy {
        idle_timeout: Duration::from_secs(2),
        attempt_timeout: None,
        detect_loops: false,
        max_retries: 0,
        max_output_bytes: 64 * 1024,
    };
    assert!(
        sav::run_supervised_observed(&policy, &command, &cancellation, &mut observer).is_ok(),
        "a zero-retry policy was rejected for an opaque effect"
    );
}

/// Runtime events are bounded and redacted before they reach a durable record.
/// Free-text payloads (reasoning, answer text, tool descriptions, failure
/// messages) are dropped; only the structural identifiers and boolean outcomes
/// remain.
#[test]
fn runtime_events_are_bounded_and_redacted() {
    let reasoning = sav::runtime::Event::Reasoning("hidden chain of thought".into());
    assert_eq!(redact_event(&reasoning), RedactedEvent::Note);

    let text = sav::runtime::Event::Text("answer text".into());
    assert_eq!(redact_event(&text), RedactedEvent::Note);

    let tool_result = sav::runtime::Event::ToolResult {
        description: "secret tool description".into(),
        name: "workspace.read".into(),
        failed: true,
    };
    assert_eq!(
        redact_event(&tool_result),
        RedactedEvent::ToolResult {
            name: "workspace.read".into(),
            failed: true,
        }
    );
    // The serialized redacted form must not contain the dropped text.
    let json = serde_json::to_string(&redact_event(&tool_result)).unwrap();
    assert!(!json.contains("secret"));

    let prompt_end = sav::runtime::Event::PromptEnd {
        exhausted: true,
        cancelled: false,
        reason: StopReason::Completed,
    };
    assert_eq!(
        redact_event(&prompt_end),
        RedactedEvent::PromptEnd {
            exhausted: true,
            cancelled: false,
        }
    );
}

/// A classified tool failure is bounded and carries effect certainty and a
/// safe next action. An explicit refusal is not executed; a host error is
/// uncertain. The failure message is bounded and printable.
#[test]
fn a_classified_tool_failure_is_bounded_and_actionable() {
    // An explicit policy refusal is not executed.
    let policy_error = sav::runtime::Error::ToolPolicy {
        tool: "probe".into(),
        reason: "not permitted".into(),
    };
    let failure = ToolFailure::from_error(&policy_error);
    assert_eq!(failure.category, ToolFailureCategory::Denied);
    assert_eq!(failure.effects, EffectCertainty::NotExecuted);
    assert_eq!(failure.recovery, RecoveryAction::ReviseRequest);

    // A host error is uncertain.
    let host_error = sav::runtime::Error::Host {
        reason: "execution failed".into(),
    };
    let failure = ToolFailure::from_error(&host_error);
    assert_eq!(failure.category, ToolFailureCategory::Unobserved);
    assert_eq!(failure.effects, EffectCertainty::MayHaveEffects);
    assert_eq!(failure.recovery, RecoveryAction::StopAndReconcile);

    // The failure message is bounded and printable.
    let long_message = "x".repeat(8192);
    let failure = ToolFailure::not_executed(ToolFailureCategory::InvalidRequest, &long_message);
    assert!(failure.message.len() <= 4096);
    assert!(failure.next_step.starts_with("Not executed"));
    assert_eq!(failure.recovery, RecoveryAction::ReviseRequest);
}

/// A successful no-op output is not progress. Three identical successful
/// observations of a repeatable action are evidence of stagnation, and the
/// run is bounded by the correction allowance, not by a turn cap.
#[test]
fn a_successful_noop_output_is_not_progress() {
    let intent = |index: usize| ToolIntent {
        id: format!("call-{index}"),
        provider_id: None,
        name: "probe".into(),
        arguments: serde_json::json!({"path": 0}),
    };
    let success = ToolOutcome {
        exited: true,
        status: Some(0),
        stdout: b"unchanged".to_vec(),
        stderr: Vec::new(),
        timed_out: false,
        output_limit_exceeded: false,
        cancelled: false,
    };

    let mut tracker = ToolLoopTracker::default();
    assert!(
        tracker
            .observe(&intent(0), &success, true, ProgressEvidence::None)
            .is_none()
    );
    assert!(
        tracker
            .observe(&intent(1), &success, true, ProgressEvidence::None)
            .is_none()
    );
    let report = tracker
        .observe(&intent(2), &success, true, ProgressEvidence::None)
        .expect("three identical no-op observations are stagnation");
    assert_eq!(report.kind, ToolLoopKind::RepeatedObservation);
    assert_eq!(report.copies, 3);
}
