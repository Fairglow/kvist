use std::{
    cell::Cell,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use sav::{
    CancellationToken, Error, ModelRequest, ModelStreamEvent, ModelTransport, ModelTurn,
    loop_detection::{RepetitionReport, StreamLoopDetector},
};
use serde_json::json;

use super::{LiveRun, TurnBudget};

/// Run-local, monotonic evidence that this provider operation saw guidance,
/// even if it was withdrawn before the authoring owner resumes.
#[derive(Clone, Copy)]
pub(super) struct Controls<'a> {
    pub(super) mailbox: &'a sav::runtime::ControlMailbox,
    pub(super) interrupted: &'a AtomicBool,
}

#[derive(Clone, Copy)]
pub(super) struct Inspection<'a> {
    pub(super) cancellation: &'a CancellationToken,
    pub(super) controls: Option<Controls<'a>>,
    pub(super) budget: &'a TurnBudget,
}

impl Inspection<'_> {
    pub(super) fn check(&self) -> sav::Result<()> {
        if self.cancellation.is_cancelled()
            || self
                .controls
                .is_some_and(|controls| controls.mailbox.is_shutdown())
        {
            return Err(Error::ModelTransportCancelled);
        }
        if let Some(controls) = self.controls
            && controls.mailbox.has_pending()
        {
            controls.interrupted.store(true, Ordering::Relaxed);
            return Err(Error::ModelTransportCancelled);
        }
        if self.budget.remaining().is_zero() {
            return Err(Error::ModelTransportTimedOut);
        }
        Ok(())
    }

    fn inspect(
        &self,
        inspect: impl FnOnce(&mut dyn FnMut() -> bool) -> Option<RepetitionReport>,
    ) -> sav::Result<Option<RepetitionReport>> {
        self.check()?;
        let mut stopped = None;
        let report = inspect(&mut || {
            if stopped.is_some() {
                return false;
            }
            match self.check() {
                Ok(()) => true,
                Err(error) => {
                    stopped = Some(error);
                    false
                }
            }
        });
        if let Some(error) = stopped {
            return Err(error);
        }
        self.check()?;
        Ok(report)
    }
}

fn record(report: RepetitionReport, live: Option<&LiveRun>) -> sav::Result<()> {
    if let Some(live) = live {
        live.record(
            &json!({"type":"generation_repetition","data":{
                "report":report,"proposals_executed":false,"provisional_output_discarded":true
            }}),
            true,
        )?;
    }
    Ok(())
}

/// Complete/unary output is checked before length retries or authorization.
pub(super) fn finish(
    turn: &ModelTurn,
    live: Option<&LiveRun>,
    inspection: Inspection<'_>,
) -> sav::Result<()> {
    if let Some(report) = inspection
        .inspect(|keep_going| StreamLoopDetector::default().finish_while(turn, keep_going))?
    {
        record(report, live)?;
        inspection.check()?;
        return Err(Error::RepetitiveGeneration { report });
    }
    Ok(())
}

/// One fresh detector per provider attempt; only the provider child is cancelled.
pub(super) struct DetectedTransport<'a, T> {
    pub(super) transport: &'a T,
    pub(super) live: Option<&'a LiveRun>,
    pub(super) inspection: Inspection<'a>,
    /// The counted owner reconciles completed turns before applying this
    /// rejection. The handoff belongs to exactly one synchronous attempt.
    pub(super) rejection: &'a Cell<Option<Error>>,
}

impl<T: ModelTransport> ModelTransport for DetectedTransport<'_, T> {
    fn complete(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
    ) -> sav::Result<ModelTurn> {
        self.transport.complete(request, cancellation)
    }

    fn stream(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        let mut detector = StreamLoopDetector::default();
        let mut repetition = None;
        let mut recording_error = None;
        let mut inspection_error = None;
        let mut relay = |event: ModelStreamEvent| {
            if repetition.is_some() || recording_error.is_some() || inspection_error.is_some() {
                return Err(Error::ModelTransportCancelled);
            }
            let report = match self
                .inspection
                .inspect(|keep_going| detector.observe_while(&event, keep_going))
            {
                Ok(report) => report,
                Err(error) => {
                    inspection_error = Some(error);
                    return Err(Error::ModelTransportCancelled);
                }
            };
            if let Some(report) = report {
                if let Err(error) = record(report, self.live) {
                    recording_error = Some(error);
                } else if let Err(error) = self.inspection.check() {
                    inspection_error = Some(error);
                    return Err(Error::ModelTransportCancelled);
                } else {
                    repetition = Some(report);
                }
                cancellation.cancel();
                return Err(Error::ModelTransportCancelled);
            }
            on_event(event)
        };
        let result = self.transport.stream(request, cancellation, &mut relay);
        let rejection = if let Some(error) = recording_error {
            Some(error)
        } else if matches!(
            &result,
            Ok(_) | Err(Error::Cancelled | Error::ModelTransportCancelled)
        ) {
            inspection_error
                .or_else(|| repetition.map(|report| Error::RepetitiveGeneration { report }))
        } else {
            None
        };
        match (rejection, result) {
            (Some(error), Ok(turn)) => {
                self.rejection.set(Some(error));
                Ok(turn)
            }
            (Some(error), Err(_)) => Err(error),
            (None, result) => result,
        }
    }

    fn deadline(&self) -> Duration {
        self.transport.deadline()
    }
}
