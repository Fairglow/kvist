use sav::runtime::{AgentSession, ControlMailbox, GuidanceNotice, GuidanceState, ToolOutcome};
use sav::{CancellationToken, ToolIntent};
use serde_json::json;

use super::{EffectRecord, LiveRun, redact_bounded};
use crate::Result;

/// Shared control mechanics; task approval and the broker remain outside this adapter.
pub(super) struct RunControls<'a> {
    pub(super) mailbox: &'a ControlMailbox,
    pub(super) cancellation: &'a CancellationToken,
    pub(super) live: &'a LiveRun,
    pub(super) redactions: &'a [String],
}

impl RunControls<'_> {
    fn notice(&self, id: sav::runtime::GuidanceId, state: GuidanceState) -> Result<()> {
        self.live.record(
            &json!({"type":"control","data":GuidanceNotice{id,state}}),
            true,
        )?;
        Ok(())
    }

    pub(super) fn deliver(&self, session: &mut AgentSession) -> Result<bool> {
        let mut any_delivered = false;
        for id in self.mailbox.withdrawals()? {
            self.notice(id, GuidanceState::Withdrawn)?;
        }
        for guidance in self.mailbox.claim()? {
            self.notice(guidance.id, GuidanceState::Accepted)?;
            self.notice(guidance.id, GuidanceState::Claimed)?;
            if self.cancellation.is_cancelled() || self.mailbox.is_shutdown() {
                self.notice(guidance.id, GuidanceState::Dropped)?;
                continue;
            }
            self.live.record(
                &json!({"type":"guidance","data":{
                    "id":guidance.id,"text":redact_bounded(&guidance.text,self.redactions,8192)
                }}),
                true,
            )?;
            let delivered = self.mailbox.deliver_if_open(|| {
                if !self.cancellation.is_cancelled() {
                    session.push_user(format!(
                        "Supplemental operator guidance; original task and approved scope remain unchanged:\n{}",guidance.text
                    ));
                }
            })? && !self.cancellation.is_cancelled();
            any_delivered |= delivered;
            self.notice(
                guidance.id,
                if delivered {
                    GuidanceState::Delivered
                } else {
                    GuidanceState::Dropped
                },
            )?;
        }
        Ok(any_delivered)
    }

    pub(super) fn skip_calls<'a>(
        &self,
        intents: impl Iterator<Item = &'a ToolIntent>,
        effects: &[EffectRecord],
        session: &mut AgentSession,
    ) -> Result<()> {
        for intent in
            intents.filter(|intent| !effects.iter().any(|effect| effect.call_id == intent.id))
        {
            let content = "not executed: proposed action superseded by guidance before dispatch";
            self.live.record(
                &json!({"type":"tool_result","data":{
                    "call_id":intent.id,"name":intent.name,"not_executed":true,"reason":content
                }}),
                true,
            )?;
            session.record_tool_result(
                &intent.id,
                &intent.name,
                &ToolOutcome::rejected_with(content),
            );
        }
        Ok(())
    }

    pub(super) fn close(&self) -> Result<()> {
        let withdrawals = self.mailbox.withdrawals()?;
        let dropped = self.mailbox.end_run()?;
        for id in withdrawals {
            self.notice(id, GuidanceState::Withdrawn)?;
        }
        for id in dropped {
            self.notice(id, GuidanceState::Dropped)?;
        }
        Ok(())
    }
}
