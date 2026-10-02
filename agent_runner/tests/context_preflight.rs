use agent_runner::{
    Error,
    context::{ContextManager, estimate_request, estimate_tokens},
};
use agent_runtime::{
    ModelMessage, ModelRequest, ReasoningEffort, ToolChoice, ToolDefinition, ToolIntent,
};
use serde_json::json;

fn request(messages: Vec<ModelMessage>) -> ModelRequest {
    ModelRequest {
        model: "local-model".into(),
        messages,
        tools: vec![],
        tool_choice: ToolChoice::Auto,
        reasoning_effort: None,
        output_schema: None,
        max_output_tokens: None,
    }
}

fn user(text: &str) -> ModelMessage {
    ModelMessage::User(text.into())
}

fn assistant(ids: &[&str], text: &str) -> ModelMessage {
    ModelMessage::Assistant {
        text: text.into(),
        tool_intents: ids
            .iter()
            .map(|id| ToolIntent {
                id: (*id).into(),
                provider_id: Some(format!("provider-{id}")),
                name: format!("tool-{id}"),
                arguments: json!({"path": "file.rs"}),
            })
            .collect(),
    }
}

fn result(id: &str, content: &str) -> ModelMessage {
    ModelMessage::ToolResult {
        call_id: id.into(),
        name: format!("tool-{id}"),
        content: content.into(),
    }
}

fn bounded_estimate(request: &ModelRequest, reserve: u32) -> usize {
    let mut request = request.clone();
    request.max_output_tokens = Some(reserve);
    estimate_request(&request).saturating_add(reserve as usize)
}

fn assert_budget_failure(mut request: ModelRequest, limit: usize, reserve: u32) {
    let original = request.clone();
    let mut manager = ContextManager::new(limit, 2);
    let error = manager.prepare(&mut request, reserve).unwrap_err();
    assert!(
        matches!(error, Error::ContextBudget { limit: actual, reserve: actual_reserve, .. }
            if actual == limit && actual_reserve == reserve as usize),
        "{error:?}"
    );
    assert_eq!(
        request, original,
        "failed preflight must not mutate the request"
    );
    assert!(manager.summary().is_empty());
}

#[test]
fn serialized_estimate_accounts_for_every_canonical_field_and_framing() {
    let mut request = request(vec![ModelMessage::System("system".into()), user("goal")]);
    request.tools.push(ToolDefinition {
        name: "read".into(),
        description: "description".repeat(80),
        parameters: json!({"type": "object", "properties": {"long": {"enum": ["x".repeat(400)]}}}),
    });
    request.model = "model-identity".repeat(50);
    request.reasoning_effort = Some(ReasoningEffort::High);
    request.output_schema = Some(json!({"type": "string", "description": "schema".repeat(50)}));
    request.max_output_tokens = Some(123);
    let serialized = serde_json::to_vec(&request).unwrap();
    let expected =
        serialized.len().div_ceil(4) + 8 + request.messages.len() * 4 + request.tools.len() * 8;
    assert_eq!(estimate_request(&request), expected);
    assert_eq!(
        estimate_request(&request),
        estimate_request(&request.clone())
    );
    assert!(estimate_request(&request) > estimate_request(&self::request(vec![user("goal")])));
}

#[test]
fn utf8_is_estimated_by_bytes_not_unicode_scalar_count() {
    assert_eq!(estimate_tokens("abcdefgh"), 2);
    assert_eq!(estimate_tokens("éééé"), 2);
    assert_eq!(estimate_tokens("🦀🦀🦀🦀"), 4);
    let ascii = request(vec![user("aaaa")]);
    let unicode = request(vec![user("🦀🦀🦀🦀")]);
    assert_eq!(estimate_request(&unicode) - estimate_request(&ascii), 3);
}

#[test]
fn first_request_accounts_for_huge_system_instructions() {
    assert_budget_failure(
        request(vec![
            ModelMessage::System("system".repeat(2_000)),
            user("goal"),
        ]),
        256,
        32,
    );
}

#[test]
fn first_request_accounts_for_complete_tool_schema_and_description() {
    for huge_description in [false, true] {
        let mut request = request(vec![user("goal")]);
        request.tools.push(ToolDefinition {
            name: "read".into(),
            description: if huge_description { "d".repeat(8_000) } else { "read".into() },
            parameters: json!({"type": "object", "description": if huge_description { "p".into() } else { "p".repeat(8_000) }}),
        });
        assert_budget_failure(request, 256, 32);
    }
}

#[test]
fn oversized_latest_prompt_is_irreducible() {
    assert_budget_failure(request(vec![user(&"goal".repeat(2_000))]), 256, 32);
}

#[test]
fn exact_limit_includes_reserve_and_output_bound_serialization() {
    let original = request(vec![user("small goal")]);
    let reserve = 32;
    let used = bounded_estimate(&original, reserve);
    let mut accepted = original.clone();
    let mut manager = ContextManager::new(used, 2);
    assert_eq!(manager.prepare(&mut accepted, reserve).unwrap(), None);
    assert_eq!(accepted.max_output_tokens, Some(reserve));
    assert_eq!(bounded_estimate(&accepted, reserve), used);
    assert_budget_failure(original, used - 1, reserve);
}

#[test]
fn invalid_reserves_and_windows_are_configuration_errors_without_mutation() {
    for (limit, reserve) in [(100, 0), (100, 100), (100, 101), (2_000_000, 1_048_577)] {
        let mut request = request(vec![user("goal")]);
        let original = request.clone();
        let mut manager = ContextManager::new(limit, 1);
        assert!(matches!(
            manager.prepare(&mut request, reserve),
            Err(Error::Config { .. })
        ));
        assert_eq!(request, original);
    }
    let mut request = request(vec![user("goal")]);
    let mut manager = ContextManager::with_bounds(100, 50, 0);
    assert!(matches!(
        manager.prepare(&mut request, 1),
        Err(Error::Config { .. })
    ));
}

#[test]
fn saturating_window_math_accepts_a_maximum_usize_limit() {
    let mut request = request(vec![user("goal")]);
    let mut manager = ContextManager::new(usize::MAX, 1);
    assert_eq!(manager.prepare(&mut request, 1_048_576).unwrap(), None);
    assert_eq!(request.max_output_tokens, Some(1_048_576));
}

#[test]
fn autonomous_single_goal_compacts_completed_turns_and_preserves_all_systems() {
    let goal = user("Implement the original goal");
    let systems = [
        ModelMessage::System("first instructions".into()),
        ModelMessage::System("second instructions".into()),
    ];
    let mut request = request(vec![systems[0].clone(), goal.clone()]);
    for index in 0..6 {
        let id = format!("call-{index}");
        request
            .messages
            .push(assistant(&[&id], &format!("work-{index}")));
        request
            .messages
            .push(result(&id, &"large result".repeat(100)));
        if index == 2 {
            request.messages.push(systems[1].clone());
        }
    }
    let newest = request.messages[request.messages.len() - 2..].to_vec();
    let mut manager = ContextManager::new(900, 4);
    let compaction = manager.prepare(&mut request, 64).unwrap().unwrap();
    assert!(
        compaction.compacted_turns >= 4,
        "must reduce the normal retention if needed"
    );
    assert!(request.messages.contains(&goal));
    assert_eq!(
        request
            .messages
            .iter()
            .filter(|message| matches!(message, ModelMessage::System(_)))
            .cloned()
            .collect::<Vec<_>>(),
        systems
    );
    assert_eq!(&request.messages[request.messages.len() - 2..], newest);
    assert!(bounded_estimate(&request, 64) <= manager.limit_tokens());
    assert!(request.messages.iter().any(|message| matches!(
        message, ModelMessage::User(text) if text.contains("lossy") && text.contains("non-authoritative")
    )));
}

#[test]
fn multi_call_groups_retain_every_result_in_its_original_order() {
    for ids in [vec!["a", "b"], vec!["a", "b", "c"]] {
        let mut messages = vec![
            user("goal"),
            assistant(&["old"], "old work"),
            result("old", &"x".repeat(8_000)),
        ];
        let newest_start = messages.len();
        messages.push(assistant(&ids, "new work"));
        for id in ids.iter().rev() {
            messages.push(result(id, "paired"));
        }
        let newest = messages[newest_start..].to_vec();
        let mut request = request(messages);
        let mut manager = ContextManager::new(800, 1);
        assert!(manager.prepare(&mut request, 32).unwrap().is_some());
        assert_eq!(
            &request.messages[request.messages.len() - newest.len()..],
            newest
        );
        assert!(request.messages.contains(&user("goal")));
        assert!(bounded_estimate(&request, 32) <= 800);
    }
}

#[test]
fn malformed_groups_fail_even_when_the_request_is_small() {
    let mut wrong_name = result("a", "bad");
    if let ModelMessage::ToolResult { name, .. } = &mut wrong_name {
        *name = "other-tool".into();
    }
    let histories = [
        vec![user("goal"), result("orphan", "bad")],
        vec![
            user("goal"),
            assistant(&["a", "a"], "duplicate"),
            result("a", "bad"),
        ],
        vec![user("goal"), assistant(&["a"], "missing")],
        vec![
            user("goal"),
            assistant(&["a", "b"], "missing one"),
            result("a", "ok"),
        ],
        vec![
            user("goal"),
            assistant(&["a"], "duplicate result"),
            result("a", "ok"),
            result("a", "bad"),
        ],
        vec![user("goal"), assistant(&["a"], "wrong name"), wrong_name],
        vec![
            user("goal"),
            assistant(&["a"], "interrupted"),
            user("new goal"),
            result("a", "late"),
        ],
        vec![
            user("goal"),
            assistant(&["a"], "interrupted"),
            assistant(&[], "next"),
        ],
    ];
    for messages in histories {
        let mut request = request(messages);
        let original = request.clone();
        let mut manager = ContextManager::new(10_000, 1);
        assert!(matches!(
            manager.prepare(&mut request, 32),
            Err(Error::InvalidModelTurn { .. })
        ));
        assert_eq!(request, original);
        assert!(manager.summary().is_empty());
    }
}

#[test]
fn turn_local_call_ids_can_be_reused_in_later_completed_turns() {
    let mut request = request(vec![
        user("goal"),
        assistant(&["a"], "first"),
        result("a", "first result"),
        assistant(&["a"], "second"),
        result("a", "second result"),
    ]);
    let mut manager = ContextManager::new(10_000, 2);
    assert_eq!(manager.prepare(&mut request, 32).unwrap(), None);
}

#[test]
fn newest_completed_group_cannot_be_discarded_to_fit() {
    assert_budget_failure(
        request(vec![
            user("goal"),
            assistant(&["a"], "older"),
            result("a", "small"),
            assistant(&["b"], "newest"),
            result("b", &"large".repeat(2_000)),
        ]),
        256,
        32,
    );
}

#[test]
fn normal_compaction_keeps_the_configured_last_completed_groups() {
    let mut request = request(vec![user("goal")]);
    for id in ["a", "b", "c", "d"] {
        request.messages.push(assistant(&[id], "work"));
        request.messages.push(result(id, "result"));
    }
    let retained = request.messages[3..].to_vec();
    let mut manager = ContextManager::with_bounds(10_000, 100, 3);
    let compaction = manager.prepare(&mut request, 32).unwrap().unwrap();
    assert_eq!(compaction.compacted_turns, 1);
    assert_eq!(
        &request.messages[request.messages.len() - retained.len()..],
        retained
    );
}

#[test]
fn repeated_compaction_preserves_prior_summary_without_duplication() {
    let mut request = request(vec![
        user("goal"),
        assistant(&["a"], "HISTORY_ALPHA"),
        result("a", &"x".repeat(2_000)),
        assistant(&["b"], "HISTORY_BETA"),
        result("b", "b result"),
    ]);
    let mut manager = ContextManager::with_bounds(2_000, 100, 1);
    manager.prepare(&mut request, 32).unwrap().unwrap();
    assert_eq!(manager.summary().matches("HISTORY_ALPHA").count(), 1);
    request.messages.push(assistant(&["c"], "current work"));
    request.messages.push(result("c", "current result"));
    manager.prepare(&mut request, 32).unwrap().unwrap();
    assert_eq!(manager.summary().matches("HISTORY_ALPHA").count(), 1);
    assert_eq!(manager.summary().matches("HISTORY_BETA").count(), 1);
    let snapshot = request.clone();
    assert_eq!(manager.prepare(&mut request, 32).unwrap(), None);
    assert_eq!(request, snapshot);
    assert_eq!(
        request
            .messages
            .iter()
            .filter(|message| matches!(
                message, ModelMessage::User(text) if text.contains("non-authoritative")
            ))
            .count(),
        1
    );
}

#[test]
fn old_summary_is_not_mistaken_for_the_active_user_goal() {
    let mut request = request(vec![
        user("old goal"),
        assistant(&["a"], "old work"),
        result("a", &"x".repeat(3_000)),
        assistant(&["b"], "recent work"),
        result("b", "result"),
    ]);
    let mut manager = ContextManager::with_bounds(1_000, 100, 1);
    manager.prepare(&mut request, 32).unwrap();
    let prior_summary = request
        .messages
        .iter()
        .find(|message| {
            matches!(
                message, ModelMessage::User(text) if text.contains("non-authoritative")
            )
        })
        .unwrap()
        .clone();
    // A summary is not a goal even if it follows the actual goal in the list.
    request.messages = vec![user("new active goal"), prior_summary];
    request.messages.push(assistant(&["c"], "next"));
    request.messages.push(result("c", "next result"));
    manager.prepare(&mut request, 32).unwrap();
    assert!(request.messages.contains(&user("new active goal")));
    assert_eq!(manager.summary().matches("old work").count(), 1);
    assert!(
        !manager.summary().contains("non-authoritative"),
        "do not summarize the summary envelope"
    );
}

#[test]
fn summary_is_bounded_by_the_real_remaining_window_even_when_tiny() {
    let goal = user("goal");
    let newest = assistant(&[], "newest");
    let minimum = request(vec![goal.clone(), newest.clone()]);
    let reserve = 16;
    let limit = bounded_estimate(&minimum, reserve) + 4;
    let mut request = request(vec![
        goal.clone(),
        assistant(&["a"], "old history"),
        result("a", &"old".repeat(2_000)),
        newest.clone(),
    ]);
    let mut manager = ContextManager::new(limit, 6);
    manager.prepare(&mut request, reserve).unwrap().unwrap();
    assert!(bounded_estimate(&request, reserve) <= limit);
    assert!(request.messages.contains(&goal));
    assert!(request.messages.contains(&newest));
    assert!(manager.summary().chars().count() < 4_000);
}

#[test]
fn an_error_after_a_success_does_not_change_the_rolling_summary() {
    let mut request = request(vec![
        user("goal"),
        assistant(&["a"], "remember this"),
        result("a", &"x".repeat(4_000)),
        assistant(&["b"], "latest"),
        result("b", "ok"),
    ]);
    let mut manager = ContextManager::new(700, 1);
    manager.prepare(&mut request, 32).unwrap();
    let summary = manager.summary().to_owned();
    request.messages.push(assistant(&["missing"], "malformed"));
    let original = request.clone();
    assert!(matches!(
        manager.prepare(&mut request, 32),
        Err(Error::InvalidModelTurn { .. })
    ));
    assert_eq!(request, original);
    assert_eq!(manager.summary(), summary);
    request.messages.pop();
    request.messages.push(assistant(&["oversized"], "latest"));
    request
        .messages
        .push(result("oversized", &"large".repeat(4_000)));
    let original = request.clone();
    assert!(matches!(
        manager.prepare(&mut request, 32),
        Err(Error::ContextBudget { .. })
    ));
    assert_eq!(request, original);
    assert_eq!(manager.summary(), summary);
}
