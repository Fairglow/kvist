use sav::{ModelRequest, ModelUsage};

use crate::Result;

/// Known provider usage and conservative reservations are distinct.
/// A failed/interrupted attempt, or an unavailable/zero usage report, consumes
/// its full preflight estimate plus output reservation instead of becoming free.
#[derive(Clone)]
pub(super) struct TokenBudget {
    limit: Option<u64>,
    charged: u64,
    pub(super) uncertain_attempts: u32,
}

fn exhausted() -> crate::KvistError {
    sav::runtime::Error::RunBudget {
        reason: "cumulative token limit exhausted (including conservative unknown-attempt charges)"
            .into(),
    }
    .into()
}

impl TokenBudget {
    pub(super) fn new(limit: Option<usize>) -> Self {
        Self {
            limit: limit.map(|value| value as u64),
            charged: 0,
            uncertain_attempts: 0,
        }
    }

    pub(super) fn reserve(&mut self, request: &mut ModelRequest) -> Result<u64> {
        let Some(limit) = self.limit else {
            self.uncertain_attempts = self.uncertain_attempts.saturating_add(1);
            return Ok(0);
        };
        let remaining = limit.checked_sub(self.charged).ok_or_else(exhausted)?;
        request.max_output_tokens = Some(request.max_output_tokens.unwrap_or(1024));
        let input = sav::runtime::context::estimate_request(request) as u64;
        let available = remaining
            .checked_sub(input)
            .filter(|value| *value > 0)
            .ok_or_else(exhausted)?;
        request.max_output_tokens =
            Some(u64::from(request.max_output_tokens.unwrap_or(1024)).min(available) as u32);
        let charge = (sav::runtime::context::estimate_request(request) as u64)
            .saturating_add(u64::from(request.max_output_tokens.unwrap_or(1024)));
        if charge > remaining {
            return Err(exhausted());
        }

        self.charged = self.charged.saturating_add(charge);
        self.uncertain_attempts = self.uncertain_attempts.saturating_add(1);
        Ok(charge)
    }

    pub(super) fn preflight(&self, request: &ModelRequest) -> Result<()> {
        self.clone().reserve(&mut request.clone()).map(|_| ())
    }

    pub(super) fn finish(&mut self, reservation: u64, usage: Option<ModelUsage>) {
        if let Some(usage) = usage {
            let total = usage
                .input_tokens
                .saturating_add(usage.output_tokens)
                .max(usage.total_tokens);
            if total > 0 {
                if self.limit.is_some() {
                    self.charged = self
                        .charged
                        .saturating_sub(reservation)
                        .saturating_add(total);
                }
                self.uncertain_attempts = self.uncertain_attempts.saturating_sub(1);
            }
        }
    }

    pub(super) fn check(&self) -> Result<()> {
        if self.limit.is_some_and(|limit| self.charged > limit) {
            Err(exhausted())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_and_zero_usage_do_not_replenish_the_budget() {
        for usage in [
            None,
            Some(ModelUsage {
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
            }),
        ] {
            let mut request = ModelRequest {
                model: "fixture".into(),
                messages: vec![sav::ModelMessage::User("task".into())],
                tools: vec![],
                tool_choice: sav::ToolChoice::None,
                reasoning_effort: None,
                output_schema: None,
                max_output_tokens: Some(256),
            };
            let mut budget = TokenBudget::new(Some(1000));
            let reserved = budget.reserve(&mut request).expect("reserve");
            budget.finish(reserved, usage);
            assert_eq!(budget.charged, reserved);
            assert_eq!(budget.uncertain_attempts, 1);
        }
    }

    #[test]
    fn remaining_budget_caps_output_and_known_usage_is_cumulative() {
        let mut request = ModelRequest {
            model: "fixture".into(),
            messages: vec![sav::ModelMessage::User("task".into())],
            tools: vec![],
            tool_choice: sav::ToolChoice::None,
            reasoning_effort: None,
            output_schema: None,
            max_output_tokens: Some(256),
        };
        let mut budget = TokenBudget::new(Some(1000));
        let first = budget.reserve(&mut request).expect("first");
        budget.finish(
            first,
            Some(ModelUsage {
                input_tokens: 700,
                output_tokens: 100,
                total_tokens: 800,
            }),
        );
        let second = budget.reserve(&mut request).expect("remaining request");
        assert!(request.max_output_tokens.expect("bounded output") < 256);
        assert!(budget.charged <= 1000);
        budget.finish(
            second,
            Some(ModelUsage {
                input_tokens: 100,
                output_tokens: 100,
                total_tokens: 200,
            }),
        );
        assert_eq!(budget.charged, 1000);
        assert!(budget.reserve(&mut request).is_err());
    }

    #[test]
    fn failed_attempt_reservations_survive_retries_and_steering() {
        let mut request = ModelRequest {
            model: "fixture".into(),
            messages: vec![sav::ModelMessage::User("task".into())],
            tools: vec![],
            tool_choice: sav::ToolChoice::None,
            reasoning_effort: None,
            output_schema: None,
            max_output_tokens: Some(256),
        };
        let input = sav::runtime::context::estimate_request(&request) as u64;
        let mut budget = TokenBudget::new(Some((input + 256) as usize));
        budget.reserve(&mut request).expect("one attempt");
        // No completion/usage is available for a retry or interrupted attempt.
        assert!(budget.preflight(&request).is_err());
        assert_eq!(budget.uncertain_attempts, 1);
    }
}
