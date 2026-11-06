use sav::{ModelRequest, ModelUsage};

use crate::Result;

/// Known usage is a lower bound; complete provenance is unavailable permanently
/// once any generation attempt returns without trustworthy usage.
#[derive(serde::Serialize)]
pub(super) struct Accounting {
    pub(super) returned_attempts: u64,
    pub(super) known_attempts: u64,
    pub(super) uncertain_attempts: u32,
    pub(super) known_usage: Option<ModelUsage>,
    pub(super) cumulative_usage: Option<ModelUsage>,
    pub(super) provenance_complete: bool,
    pub(super) charged_tokens: Option<u64>,
}

/// Known provider usage and conservative reservations are distinct.
/// A failed/interrupted attempt, or an unavailable/zero usage report, consumes
/// its full preflight estimate plus output reservation instead of becoming free.
#[derive(Clone)]
pub(super) struct TokenBudget {
    limit: Option<u64>,
    charged: u64,
    known: Option<ModelUsage>,
    returned_attempts: u64,
    known_attempts: u64,
    provenance_lost: bool,
    pub(super) uncertain_attempts: u32,
}

fn exhausted() -> crate::KvistError {
    sav::runtime::Error::RunBudget {
        resource: sav::runtime::BudgetResource::ModelTokens,
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
            known: None,
            returned_attempts: 0,
            known_attempts: 0,
            provenance_lost: false,
            uncertain_attempts: 0,
        }
    }

    pub(super) fn reserve(&mut self, request: &mut ModelRequest) -> Result<u64> {
        let remaining = self.limit.map_or(Ok(u64::MAX), |limit| {
            limit.checked_sub(self.charged).ok_or_else(exhausted)
        })?;
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
        self.returned_attempts = self.returned_attempts.saturating_add(1);
        if let Some(usage) = usage {
            let total = usage
                .input_tokens
                .saturating_add(usage.output_tokens)
                .max(usage.total_tokens);
            let known = self.known.get_or_insert(ModelUsage {
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
            });
            known.input_tokens = known.input_tokens.saturating_add(usage.input_tokens);
            known.output_tokens = known.output_tokens.saturating_add(usage.output_tokens);
            known.total_tokens = known.total_tokens.saturating_add(total);
            if total > 0 {
                self.known_attempts = self.known_attempts.saturating_add(1);
                self.charged = self
                    .charged
                    .saturating_add(total.saturating_sub(reservation));
                self.uncertain_attempts = self.uncertain_attempts.saturating_sub(1);
            } else {
                self.provenance_lost = true;
            }
        } else {
            self.provenance_lost = true;
        }
    }

    pub(super) fn failed(&mut self) {
        self.returned_attempts = self.returned_attempts.saturating_add(1);
        self.provenance_lost = true;
    }

    pub(super) fn accounting(&self) -> Accounting {
        let provenance_complete = !self.provenance_lost && self.uncertain_attempts == 0;
        Accounting {
            returned_attempts: self.returned_attempts,
            known_attempts: self.known_attempts,
            uncertain_attempts: self.uncertain_attempts,
            known_usage: self.known,
            cumulative_usage: if provenance_complete {
                self.known
            } else {
                None
            },
            provenance_complete,
            charged_tokens: Some(self.charged),
        }
    }

    pub(super) fn known_usage(&self) -> Option<ModelUsage> {
        self.known
    }

    pub(super) fn charge(&self) -> u64 {
        self.charged
    }

    pub(super) fn remaining(&self) -> Option<u64> {
        self.limit.map(|limit| limit.saturating_sub(self.charged))
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
    fn phase4_known_usage_below_reservation_never_refunds_model_charge() {
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
        let reservation = budget.reserve(&mut request).expect("reserve");
        budget.finish(
            reservation,
            Some(ModelUsage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
            }),
        );
        assert_eq!(budget.charged, reservation);
        assert_eq!(
            budget.known_usage().map(|usage| usage.total_tokens),
            Some(2)
        );
    }

    #[test]
    fn phase4_unlimited_policy_still_records_conservative_unknown_debit() {
        let mut request = ModelRequest {
            model: "fixture".into(),
            messages: vec![sav::ModelMessage::User("task".into())],
            tools: vec![],
            tool_choice: sav::ToolChoice::None,
            reasoning_effort: None,
            output_schema: None,
            max_output_tokens: Some(256),
        };
        let mut budget = TokenBudget::new(None);
        let reservation = budget.reserve(&mut request).expect("reserve");
        budget.failed();
        let accounting = budget.accounting();
        assert!(reservation > 256);
        assert_eq!(accounting.charged_tokens, Some(reservation));
        assert!(!accounting.provenance_complete);
    }

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
