use sav::{CancellationToken, ModelRequest, registry::NativeModel, runtime::ContextManager};
use serde_json::json;

use super::{LiveRun, TurnBudget, is_retryable_transport_error, redact_bounded};
use crate::{KvistError, Result};
use std::time::Duration;

pub(super) fn model_context(
    native: Option<&NativeModel>,
    initial_request: &ModelRequest,
    budget: &TurnBudget,
    cancellation: &CancellationToken,
    live: &LiveRun,
    redactions: &[String],
) -> Result<(ContextManager, u32)> {
    budget.model_deadline(cancellation)?;
    let mut known_limit = native.and_then(|model| model.context_limit);
    let mut source = if known_limit.is_some() {
        "configuration"
    } else if native.is_none() {
        "default"
    } else {
        "provisional"
    };
    if let Some(native) = native.filter(|_| known_limit.is_none()) {
        let deadline = Duration::from_secs(1).min(budget.remaining() / 10);
        let transport = native.transport_bounded(deadline, 1024 * 1024)?;
        live.record(
            &json!({"type":"capacity_probe","data":{
                "model":native.model,"endpoint":native.base_url,
                "deadline_ms":deadline.as_millis(),"optional":true
            }}),
            true,
        )?;
        eprintln!(
            "Checking optional serving-capacity metadata (at most 1s); inference does not require it."
        );
        let unavailable = match transport.context_limit(&native.model, cancellation) {
            Ok(Some(limit)) => {
                known_limit = Some(limit);
                source = "provider";
                None
            }
            Ok(None) => Some("selected-model serving capacity was not advertised".to_owned()),
            Err(error)
                if is_retryable_transport_error(&error)
                    || matches!(
                        error,
                        sav::Error::ModelProviderStatus {
                            status: 404 | 405 | 501
                        }
                    ) =>
            {
                Some(error.to_string())
            }
            Err(error) => {
                return Err(KvistError::AgentModelRequestFailed {
                    reason: format!(
                        "optional capacity discovery for model `{}` at {} failed; invalid metadata, authorization and cancellation errors cannot select a provisional budget",
                        native.model, native.base_url,
                    ),
                    source: error,
                });
            }
        };
        // A metadata timeout is optional; cancellation or an expired task is not.
        budget.model_deadline(cancellation)?;
        if let Some(reason) = unavailable {
            live.record(
                &json!({"type":"capacity_unavailable","data":{
                    "reason":redact_bounded(&reason,redactions,1024),
                    "action":"continue with a provisional local working budget"
                }}),
                true,
            )?;
            eprintln!(
                "{}",
                redact_bounded(
                    &format!(
                        "Warning: serving capacity is unknown ({reason}); continuing with a provisional local working budget, not an asserted server limit. Configure context_limit to pin the actual capacity."
                    ),
                    redactions,
                    2048,
                )
            );
        }
    }

    let reserve = native
        .and_then(|model| model.response_reserve)
        .unwrap_or_else(|| {
            if native.is_none() {
                1024
            } else {
                known_limit.map_or(8192, |limit| (limit / 4).clamp(1, 8192) as u32)
            }
        });
    let mut initial_request = initial_request.clone();
    initial_request.max_output_tokens = Some(reserve);
    let estimated_input = sav::runtime::context::estimate_request(&initial_request);
    let limit = match known_limit {
        Some(limit) => limit,
        None if native.is_none() => sav::runtime::DEFAULT_CONTEXT_TOKENS,
        None => provisional_limit(estimated_input, reserve)?,
    };
    live.record(
        &json!({"type":"model_budget","data":{
            "context_limit":limit,"response_reserve":reserve,"source":source,
            "serving_capacity_known":known_limit.is_some(),
            "estimated_initial_input_tokens":estimated_input,
            "remaining_ms":budget.remaining().as_millis()
        }}),
        true,
    )?;
    Ok((ContextManager::new(limit, 4), reserve))
}

fn provisional_limit(estimated_input: usize, reserve: u32) -> Result<usize> {
    let required = estimated_input
        .saturating_add(reserve as usize)
        .saturating_add(8192);
    if required > 1_048_576 {
        return Err(sav::runtime::Error::Config {
            path: None,
            reason: "initial request, output reserve and 8192-token history headroom exceed the provisional working-budget ceiling of 1048576 tokens; reduce selected intent or configure the actual serving context_limit".into(),
        }
        .into());
    }
    Ok(required.max(32768))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sav::{LocalModelProvider, ModelMessage, ToolChoice};

    fn request(text: String) -> ModelRequest {
        ModelRequest {
            model: "fixture".into(),
            messages: vec![ModelMessage::User(text)],
            tools: vec![],
            tool_choice: ToolChoice::None,
            reasoning_effort: None,
            output_schema: None,
            max_output_tokens: None,
        }
    }

    #[test]
    fn cancellation_expiry_and_recording_failure_cannot_select_provisional_operation() {
        let directory = tempfile::tempdir().unwrap();
        let live = LiveRun::create(
            &directory.path().join("events.jsonl"),
            "fixture",
            "fixture",
            "fixture",
            &[],
        )
        .unwrap();
        let native = NativeModel::new(
            LocalModelProvider::LlamaServer,
            "http://127.0.0.1:1".into(),
            "fixture".into(),
        );
        for cancelled in [false, true] {
            let token = CancellationToken::new();
            if cancelled {
                token.cancel();
            }
            let error = model_context(
                Some(&native),
                &request("task".into()),
                &TurnBudget::new(Duration::ZERO),
                &token,
                &live,
                &[],
            )
            .unwrap_err();
            assert!(if cancelled {
                matches!(error, KvistError::Sav(sav::Error::ModelTransportCancelled))
            } else {
                matches!(error, KvistError::Sav(sav::Error::ModelTransportTimedOut))
            });
        }
        live.finish("failed").unwrap();
        let error = model_context(
            Some(&native),
            &request("task".into()),
            &TurnBudget::new(Duration::from_secs(5)),
            &CancellationToken::new(),
            &live,
            &[],
        )
        .unwrap_err();
        assert!(matches!(
            error,
            KvistError::Sav(sav::Error::Observation { .. })
        ));
    }

    #[test]
    fn provisional_budget_hard_ceiling_never_truncates_irreducible_input() {
        let directory = tempfile::tempdir().unwrap();
        let live = LiveRun::create(
            &directory.path().join("events.jsonl"),
            "fixture",
            "fixture",
            "fixture",
            &[],
        )
        .unwrap();
        let initial = request("x".repeat(4 * 1_048_576));
        let native = NativeModel::new(
            LocalModelProvider::LlamaServer,
            "http://127.0.0.1:1".into(),
            "fixture".into(),
        );
        let before = initial.clone();
        let error = model_context(
            Some(&native),
            &initial,
            &TurnBudget::new(Duration::from_secs(5)),
            &CancellationToken::new(),
            &live,
            &[],
        )
        .unwrap_err();
        assert!(error.to_string().contains("history headroom"));
        assert_eq!(initial, before);
    }

    #[test]
    fn provisional_budget_preserves_headroom_at_the_exact_ceiling() {
        assert_eq!(provisional_limit(100, 8192).unwrap(), 32768);
        assert_eq!(
            provisional_limit(1_048_576 - 16384, 8192).unwrap(),
            1_048_576
        );
        assert!(provisional_limit(1_048_576 - 16384 + 1, 8192).is_err());
        assert!(provisional_limit(usize::MAX, 8192).is_err());
    }

    #[test]
    fn explicit_endpoint_context_and_reserve_defaults_are_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let live = LiveRun::create(
            &directory.path().join("events.jsonl"),
            "fixture",
            "fixture",
            "fixture",
            &[],
        )
        .unwrap();
        let (context, reserve) = model_context(
            None,
            &request("task".into()),
            &TurnBudget::new(Duration::from_secs(5)),
            &CancellationToken::new(),
            &live,
            &[],
        )
        .unwrap();
        assert_eq!(context.limit_tokens(), 8192);
        assert_eq!(reserve, 1024);
    }
}
