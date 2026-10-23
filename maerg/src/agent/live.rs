use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use sav::observation::RunJournal;
use sav::{CancellationToken, ModelRequest, ModelStreamEvent, ModelTransport, ModelTurn};
use serde_json::json;

use crate::{KvistError, Result};

use super::{StreamRedactor, redact_bounded};

/// Diagnostic recording is required independently of whether an observer exists.
pub(super) struct LiveRun {
    journal: Mutex<RunJournal>,
}

impl LiveRun {
    pub(super) fn create(path: &Path, run_id: &str, task: &str, profile: &str) -> Result<Self> {
        let journal = RunJournal::create(
            path,
            run_id,
            json!({"host":"kvist","task_id":task,"profile":profile,"canonical_evidence":false,
                "execution_scope":"protected_task"}),
        )?;
        eprintln!(
            "live run journal: {}",
            path.display().to_string().escape_debug()
        );
        Ok(Self {
            journal: Mutex::new(journal),
        })
    }

    pub(super) fn record(&self, event: &serde_json::Value, checkpoint: bool) -> sav::Result<()> {
        let mut journal = self.journal.lock().map_err(|_| sav::Error::Observation {
            reason: "required live journal lock is poisoned".into(),
        })?;
        journal.record(event)?;
        if checkpoint {
            journal.checkpoint()?;
        }
        Ok(())
    }

    pub(super) fn finish(&self, disposition: &str) -> Result<()> {
        self.journal
            .lock()
            .map_err(|_| {
                KvistError::Sav(sav::Error::Observation {
                    reason: "required live journal lock is poisoned".into(),
                })
            })?
            .finish(disposition)?;
        Ok(())
    }
}

pub(super) struct ObservedTransport<'a, T> {
    pub(super) transport: T,
    pub(super) live: &'a LiveRun,
    pub(super) redactions: &'a [String],
}

impl<T: ModelTransport> ObservedTransport<'_, T> {
    fn request(&self, request: &ModelRequest) -> sav::Result<()> {
        self.live.record(&json!({"type":"model_request","data":{
            "model":request.model,"message_count":request.messages.len(),"tool_count":request.tools.len()
        }}), false)
    }

    fn result(&self, turn: &sav::Result<ModelTurn>) -> sav::Result<()> {
        match turn {
            Ok(turn) => self.live.record(
                &json!({"type":"model_result","data":{
                    "model":turn.model,"text":redact_bounded(&turn.text,self.redactions,8192),
                    "text_truncated":turn.text.len()>8192,"finish_reason":turn.finish_reason,
                    "tool_count":turn.tool_intents.len(),"usage":turn.usage
                }}),
                false,
            ),
            Err(error) => self.live.record(
                &json!({"type":"model_failure","data":{
                    "message":redact_bounded(&error.to_string(),self.redactions,1024)
                }}),
                false,
            ),
        }
    }
}

impl<T: ModelTransport> ModelTransport for ObservedTransport<'_, T> {
    fn complete(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
    ) -> sav::Result<ModelTurn> {
        self.request(request)?;
        let result = self.transport.complete(request, cancellation);
        self.result(&result)?;
        result
    }

    fn stream(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        self.request(request)?;
        let mut redactor = StreamRedactor::new(self.redactions);
        let mut record_error = None;
        let mut relay = |event: ModelStreamEvent| {
            if let ModelStreamEvent::TextDelta(text) = &event {
                let text = redactor.absorb(text);
                if !text.is_empty()
                    && let Err(error) = self.live.record(&json!({"type":"text","data":text}), false)
                {
                    record_error = Some(error);
                    return Err(sav::Error::ModelTransportCancelled);
                }
            }
            on_event(event)
        };
        let result = self.transport.stream(request, cancellation, &mut relay);
        if let Some(error) = record_error {
            return Err(error);
        }
        let tail = redactor.finish();
        if !tail.is_empty() {
            self.live
                .record(&json!({"type":"text","data":tail}), false)?;
        }
        self.result(&result)?;
        result
    }

    fn deadline(&self) -> Duration {
        self.transport.deadline()
    }
}
