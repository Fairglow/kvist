//! Bounded, lock-linearized submission and guidance handoff.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use super::{Error, Result};
use serde::Serialize;

const CAPACITY: usize = 32;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_QUEUED_BYTES: usize = 1024 * 1024;

/// Run-owner-local monotonically increasing control identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GuidanceId(pub u64);

/// Observable guidance state; only accepted messages can be edited/withdrawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuidanceState {
    Accepted,
    Claimed,
    Delivered,
    Withdrawn,
    Dropped,
}

/// Correlated runtime observation, not an authorization decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GuidanceNotice {
    pub id: GuidanceId,
    pub state: GuidanceState,
}

/// The classification chosen atomically against the worker's run state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionKind {
    Task,
    Guidance,
}

/// Accepted submission receipt. Acceptance is not model-context delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Submission {
    pub id: GuidanceId,
    pub kind: SubmissionKind,
}

/// Claimed guidance, no longer editable or withdrawable.
pub struct Guidance {
    pub id: GuidanceId,
    pub text: String,
}

#[derive(Default)]
struct State {
    sequence: u64,
    active: bool,
    owned: bool,
    closed: bool,
    tasks: VecDeque<Guidance>,
    guidance: VecDeque<Guidance>,
    withdrawn: VecDeque<GuidanceId>,
    dropped: VecDeque<GuidanceId>,
    bytes: usize,
}

/// Cloneable private in-process control capability.
///
/// At most 32 messages and 1 MiB of text are pending. Guidance is <=64 KiB;
/// idle task submissions may consume the complete 1-MiB text bound.
/// A terminal run never promotes undelivered guidance into replacement tasks.
#[derive(Clone, Default)]
pub struct ControlMailbox(Arc<Mutex<State>>);

fn rejected(reason: &str) -> Error {
    Error::Control {
        reason: reason.into(),
    }
}

fn validate(text: &str, max_bytes: usize) -> Result<()> {
    if text.trim().is_empty() || text.len() > max_bytes || text.contains('\0') {
        return Err(rejected(
            "message must be nonblank, NUL-free and within the task/guidance byte limit",
        ));
    }
    Ok(())
}

impl ControlMailbox {
    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.0
            .lock()
            .map_err(|_| rejected("control state is unavailable"))
    }

    /// Submits a task only when idle; otherwise adds guidance to the active task.
    pub fn submit(&self, text: impl Into<String>) -> Result<Submission> {
        self.enqueue(text.into(), false)
    }

    /// Adds guidance, refusing an idle or closed runtime.
    pub fn steer(&self, text: impl Into<String>) -> Result<GuidanceId> {
        self.enqueue(text.into(), true)
            .map(|submission| submission.id)
    }

    fn enqueue(&self, text: String, guidance_only: bool) -> Result<Submission> {
        validate(&text, MAX_QUEUED_BYTES)?;
        let mut state = self.lock()?;
        if state.closed {
            return Err(rejected("runtime is shut down"));
        }
        if guidance_only && !state.active {
            return Err(rejected("no active task"));
        }
        if state.active {
            validate(&text, MAX_MESSAGE_BYTES)?;
        }
        if state.guidance.len() + state.tasks.len() >= CAPACITY
            || state.bytes.saturating_add(text.len()) > MAX_QUEUED_BYTES
        {
            return Err(rejected("pending control queue is full"));
        }
        state.sequence = state
            .sequence
            .checked_add(1)
            .ok_or_else(|| rejected("control identities exhausted"))?;
        let id = GuidanceId(state.sequence);
        state.bytes += text.len();
        let kind = if state.active {
            SubmissionKind::Guidance
        } else {
            SubmissionKind::Task
        };
        let queue = if state.active {
            &mut state.guidance
        } else {
            &mut state.tasks
        };
        queue.push_back(Guidance { id, text });
        Ok(Submission { id, kind })
    }

    /// Replaces an accepted, unclaimed guidance message without changing order.
    pub fn edit(&self, id: GuidanceId, text: impl Into<String>) -> Result<()> {
        let text = text.into();
        validate(&text, MAX_MESSAGE_BYTES)?;
        let mut state = self.lock()?;
        if state.closed {
            return Err(rejected("runtime is shut down"));
        }
        let index = state
            .guidance
            .iter()
            .position(|message| message.id == id)
            .ok_or_else(|| rejected("guidance is no longer pending"))?;
        let bytes = state.bytes - state.guidance[index].text.len() + text.len();
        if bytes > MAX_QUEUED_BYTES {
            return Err(rejected("pending control bytes exceeded"));
        }
        state.guidance[index].text = text;
        state.bytes = bytes;
        Ok(())
    }

    /// Withdraws guidance before claim. Claimed messages cannot be withdrawn.
    pub fn withdraw(&self, id: GuidanceId) -> Result<()> {
        let mut state = self.lock()?;
        if state.closed {
            return Err(rejected("runtime is shut down"));
        }
        let index = state
            .guidance
            .iter()
            .position(|message| message.id == id)
            .ok_or_else(|| rejected("guidance is no longer pending"))?;
        if state.withdrawn.len() >= CAPACITY {
            return Err(rejected("pending withdrawal notice queue is full"));
        }
        if let Some(message) = state.guidance.remove(index) {
            state.bytes -= message.text.len();
            state.withdrawn.push_back(id);
        }
        Ok(())
    }

    /// Arms a caller-owned run. Worker task claiming also arms the mailbox.
    pub fn begin_run(&self) -> Result<()> {
        let mut state = self.lock()?;
        if state.closed {
            return Err(rejected("runtime is shut down"));
        }
        if state.owned {
            return Err(rejected("a runtime already owns this task"));
        }
        state.active = true;
        state.owned = true;
        Ok(())
    }

    /// Atomically claims the next idle task and marks execution active.
    pub fn next_task(&self) -> Result<Option<String>> {
        let mut state = self.lock()?;
        if state.closed || state.active {
            return Ok(None);
        }
        Ok(state.tasks.pop_front().map(|message| {
            state.bytes -= message.text.len();
            state.active = true;
            message.text
        }))
    }

    /// True while guidance is pending; used only to interrupt provider operations.
    pub fn has_pending(&self) -> bool {
        self.lock()
            .is_ok_and(|state| !state.closed && !state.guidance.is_empty())
    }

    /// Whether the owner has claimed an active task.
    pub fn is_active(&self) -> bool {
        self.lock().is_ok_and(|state| state.active && !state.closed)
    }

    /// Linearizes the dispatch boundary against guidance/shutdown. A true
    /// result is not authorization; the host executor still decides authority.
    /// Guidance accepted after this point belongs to an already-started effect.
    pub fn dispatch_ready(&self) -> Result<bool> {
        let state = self.lock()?;
        Ok(state.active && !state.closed && state.guidance.is_empty())
    }

    /// Cancels only the active owned prompt, atomically against completion.
    pub fn cancel_active(&self, cancellation: &crate::CancellationToken) -> Result<bool> {
        let state = self.lock()?;
        if state.active && !state.closed {
            cancellation.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Runs a short in-memory context mutation atomically against shutdown.
    /// The callback must not block, perform effects, or call this mailbox.
    pub fn deliver_if_open(&self, deliver: impl FnOnce()) -> Result<bool> {
        let state = self.lock()?;
        if state.closed {
            return Ok(false);
        }
        deliver();
        Ok(true)
    }

    /// Permanent shutdown discards queued tasks and prevents further delivery.
    pub fn shutdown(&self) {
        if let Ok(mut state) = self.lock() {
            state.closed = true;
            state.tasks.clear();
            let ids: Vec<_> = state.guidance.drain(..).map(|message| message.id).collect();
            state.dropped.extend(ids);
            state.withdrawn.clear();
            state.bytes = 0;
        }
    }

    /// Whether shutdown was requested.
    pub fn is_shutdown(&self) -> bool {
        self.lock().map_or(true, |state| state.closed)
    }

    /// Linearization point between a final answer and a simultaneous submission.
    pub fn finish_if_idle(&self) -> Result<bool> {
        let mut state = self.lock()?;
        if !state.closed && !state.guidance.is_empty() {
            return Ok(false);
        }
        state.active = false;
        Ok(true)
    }

    /// Atomically claims a bounded ordered batch; the owner must record its
    /// acceptance, interruption and final delivery/drop disposition.
    pub fn claim(&self) -> Result<Vec<Guidance>> {
        let mut state = self.lock()?;
        if state.closed {
            return Ok(vec![]);
        }
        let messages: Vec<_> = state.guidance.drain(..).collect();
        for message in &messages {
            state.bytes -= message.text.len();
        }
        Ok(messages)
    }

    /// Drains bounded withdrawal notices for the current owner to record.
    pub fn withdrawals(&self) -> Result<Vec<GuidanceId>> {
        Ok(self.lock()?.withdrawn.drain(..).collect())
    }

    /// Ends ownership and returns undelivered guidance IDs, never queued tasks.
    /// Unconsumed withdrawal notices are discarded, not leaked into a later run.
    pub fn end_run(&self) -> Result<Vec<GuidanceId>> {
        let mut state = self.lock()?;
        state.active = false;
        state.owned = false;
        state.withdrawn.clear();
        let messages: Vec<_> = state.guidance.drain(..).collect();
        for message in &messages {
            state.bytes -= message.text.len();
        }
        let mut dropped: Vec<_> = state.dropped.drain(..).collect();
        dropped.extend(messages.into_iter().map(|message| message.id));
        Ok(dropped)
    }
}
