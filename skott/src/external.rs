//! Explicit opaque-agent execution; never a native model or protected broker.
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sav::observation::RunJournal;
use sav::registry::{Backend, ResolvedProfile};
use sav::{CancellationToken, CapturedExecutionReport, SupervisionPolicy};
use serde_json::json;

use crate::{Error, Result};

static NEXT_RUN: AtomicU64 = AtomicU64::new(1);

/// Host-supplied inputs, independent of any provider's internal permission flags.
pub struct ExternalRequest<'a> {
    pub profile: &'a ResolvedProfile,
    pub prompt: &'a str,
    pub working_directory: &'a Path,
    pub log_directory: &'a Path,
    pub wall_time: Duration,
    pub effort: Option<sav::ReasoningEffort>,
    pub acknowledged_host: bool,
}

/// An owner-held external process. Observers do not own or drop this lifetime.
pub struct ExternalRun {
    journal: PathBuf,
    cancellation: CancellationToken,
    worker: Option<JoinHandle<sav::Result<CapturedExecutionReport>>>,
}

impl ExternalRun {
    pub fn journal_path(&self) -> &Path {
        &self.journal
    }
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// Waits for the one supervised process; does not replay uncertain effects.
    pub fn wait(mut self) -> Result<CapturedExecutionReport> {
        self.join()
    }

    fn join(&mut self) -> Result<CapturedExecutionReport> {
        let worker = self.worker.take().ok_or_else(|| Error::Control {
            reason: "external run has already been joined".into(),
        })?;
        worker
            .join()
            .map_err(|_| Error::Recording {
                reason: "external run worker panicked; journal may be incomplete".into(),
            })?
            .map_err(|source| Error::ExternalAgent { source })
    }
}

impl Drop for ExternalRun {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.cancellation.cancel();
            if let Err(error) = self.join() {
                tracing::debug!(%error,"external owner shutdown");
            }
        }
    }
}

/// Starts an explicitly acknowledged host command with mandatory private logs.
/// This low-level host API does not supply a sandbox or native tool authority.
pub fn start(request: ExternalRequest<'_>) -> Result<ExternalRun> {
    if !request.acknowledged_host {
        return Err(Error::Config {
            path: None,
            reason: "external-agent execution requires explicit HOST UNCONFINED acknowledgement"
                .into(),
        });
    }
    if !request.profile.profile.enabled
        || request.prompt.trim().is_empty()
        || request.prompt.len() > 64 * 1024
    {
        return Err(Error::Config {
            path: None,
            reason: "external profile must be enabled and prompt nonblank and at most 64 KiB"
                .into(),
        });
    }
    let Backend::ExternalAgent(backend) = &request.profile.profile.backend else {
        return Err(Error::Config {
            path: None,
            reason: "selected profile is not an external-agent backend".into(),
        });
    };
    let workdir = crate::config::resolve_working_directory(request.working_directory)?;
    let command = backend.command_spec(request.prompt, &[], &workdir, request.effort)?;
    let policy = SupervisionPolicy {
        idle_timeout: request.wall_time.min(Duration::from_secs(3600)),
        attempt_timeout: Some(request.wall_time),
        detect_loops: false,
        max_retries: 0,
        max_output_bytes: 4 * 1024 * 1024,
    };
    policy.validate()?;
    let (logs, _directory) =
        crate::session_log::private_directory(request.log_directory).map_err(|source| {
            crate::error::io_error(
                "open private external-agent logs",
                Some(&request.log_directory.display().to_string()),
                source,
            )
        })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Recording {
            reason: "system clock precedes the Unix epoch".into(),
        })?
        .as_nanos();
    let id = format!(
        "external-{}-{stamp}-{}",
        std::process::id(),
        NEXT_RUN.fetch_add(1, Ordering::Relaxed)
    );
    let path = logs.join(format!("{id}.events.jsonl"));
    let mut journal = RunJournal::create(
        &path,
        &id,
        json!({
            "host":"skott","kind":"external-agent","profile":request.profile.name,"source":request.profile.source,
            "digest":request.profile.digest,"provider":backend.provider,"model":backend.model,
            "execution_scope":"host_unconfined","canonical_evidence":false,"steering":"unsupported",
        }),
    )?;
    journal.record(
        &json!({"type":"external_dispatch","data":{"opaque":true,"automatic_retries":false}}),
    )?;
    journal.checkpoint()?;
    let cancellation = CancellationToken::new();
    let token = cancellation.clone();
    let worker = thread::Builder::new()
        .name("external-agent".into())
        .spawn(move || {
            let result =
                sav::run_supervised_observed(&policy, &command, &token, &mut |stream, bytes| {
                    journal.record(&json!({"type":"external_output","data":{
                        "stream":if stream == sav::OutputStream::Stdout {"stdout"} else {"stderr"},
                        "text":String::from_utf8_lossy(bytes),"raw_hex":hex::encode(bytes)
                    }}))
                });
            let disposition = match &result {
                Ok(_) => "completed",
                Err(sav::Error::Cancelled) => "cancelled",
                Err(sav::Error::Observation { .. }) => "recording_failed",
                Err(_) => "failed",
            };
            journal.finish(disposition)?;
            result
        })
        .map_err(|source| crate::error::io_error("start external-agent worker", None, source))?;
    Ok(ExternalRun {
        journal: path,
        cancellation,
        worker: Some(worker),
    })
}

/// Terminal frontend for a separately selected opaque agent.
pub fn run_profile(
    profile: &ResolvedProfile,
    overrides: &crate::tui::Overrides,
    working_directory: &Path,
    headless: bool,
) -> Result<std::process::ExitCode> {
    if headless || !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(Error::NotInteractive{reason:"external-agent host execution requires an interactive terminal; headless/protected execution is not supported".into()});
    }
    if overrides.no_logs
        || overrides.host_turns.is_some()
        || overrides.context_limit.is_some()
        || overrides.response_reserve.is_some()
        || overrides.profile.is_some()
        || overrides.limits.max_tokens != crate::RunLimits::default().max_tokens
    {
        return Err(Error::Config{path:None,reason:"external agents require durable logs and do not support native turn, context, tool-profile or token-limit overrides".into()});
    }
    let prompt = overrides.prompt.as_deref().ok_or_else(|| Error::Config {
        path: None,
        reason: "external-agent execution requires an initial prompt argument".into(),
    })?;
    let cwd = overrides.cwd.as_deref().unwrap_or(working_directory);
    let logs = overrides
        .log_dir
        .clone()
        .unwrap_or_else(|| cwd.join(crate::session_log::DEFAULT_LOG_DIR));
    let run = start(ExternalRequest {
        profile,
        prompt,
        working_directory: cwd,
        log_directory: &logs,
        wall_time: overrides.limits.wall_time,
        effort: overrides.effort,
        acknowledged_host: overrides.allow_host_execution,
    })?;
    eprintln!(
        "HOST UNCONFINED external agent; internal tools and token accounting belong to the selected CLI."
    );
    eprintln!(
        "live run journal: {}",
        crate::error::terminal_text(&run.journal_path().display().to_string())
    );
    crate::inspection::inspect_owned(run.journal_path(), run.cancellation())?;
    if run
        .worker
        .as_ref()
        .is_some_and(|worker| !worker.is_finished())
    {
        eprintln!("viewer detached; the run owner is still waiting. Ctrl+C requests cancellation.");
    }
    run.wait()?;
    Ok(std::process::ExitCode::SUCCESS)
}
