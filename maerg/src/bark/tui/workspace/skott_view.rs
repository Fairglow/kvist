use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::KeyEvent;
use sav::runtime::{GuidanceId, GuidanceNotice, GuidanceState, ProgressTelemetry, TokenAccounting};
use serde_json::Value;
use skott::session::Event;
use skott::tui::app::{App, KeyAction};

use super::{ActivePane, Workspace};
use crate::workspace_feed::Reader;

pub(super) fn idle_app() -> App {
    let mut app = App::new(
        &[],
        None,
        sav::ReasoningEffort::Medium,
        None,
        80,
        24,
        skott::tui::Theme::dark(),
    );
    app.execution_scope = skott::session_log::ExecutionScope::SandboxedWorkspace;
    app
}

pub(super) struct TaskView {
    pub app: App,
    pub shown: bool,
    pub feed: Option<Reader>,
    control: Option<PathBuf>,
    control_id: u64,
    stats: Option<TaskStats>,
}

struct TaskStats {
    id: String,
    started: Instant,
    turns: u64,
    attempts: u64,
    tools: u64,
    usage: sav::ModelUsage,
    missing_usage: bool,
    context: usize,
    limit: usize,
    capacity_source: &'static str,
    compactions: u64,
    rollovers: u64,
    timed_output: u64,
    decode_secs: f64,
    current_decode_secs: f64,
    streamed: bool,
    explicit_turns: bool,
}

impl TaskStats {
    fn new(id: String) -> Self {
        Self {
            id,
            started: Instant::now(),
            turns: 0,
            attempts: 0,
            tools: 0,
            usage: sav::ModelUsage {
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
            },
            missing_usage: false,
            context: 0,
            limit: 0,
            capacity_source: "unavailable",
            compactions: 0,
            rollovers: 0,
            timed_output: 0,
            decode_secs: 0.0,
            current_decode_secs: 0.0,
            streamed: false,
            explicit_turns: false,
        }
    }

    fn counts(&self) -> String {
        let tokens = if self.attempts == 0 || self.missing_usage {
            "tokens unavailable".into()
        } else {
            format!(
                "{} in / {} out",
                self.usage.input_tokens, self.usage.output_tokens
            )
        };
        format!(
            "{} attempts | {} tools | {tokens}",
            self.attempts, self.tools
        )
    }
}

impl Default for TaskView {
    fn default() -> Self {
        let mut app = idle_app();
        app.status = "waiting for a supervised native task".into();
        app.model_discovery_pending = true;
        Self {
            app,
            shown: false,
            feed: None,
            control: None,
            control_id: 0,
            stats: None,
        }
    }
}

impl TaskView {
    pub(super) fn turn_line(&self) -> Option<String> {
        let s = self.stats.as_ref()?;
        Some(format!(
            " Turn {} | {} | {:.1}s",
            s.turns,
            s.counts(),
            s.started.elapsed().as_secs_f64()
        ))
    }

    pub(super) fn guide(&mut self, text: &str) -> io::Result<()> {
        let path = self.control.as_deref().ok_or_else(|| {
            io::Error::other("no active native task accepts steering at this stage")
        })?;
        self.control_id = self.control_id.saturating_add(1);
        let reply = sav::observation::send_control(
            path,
            &sav::observation::ControlRequest::guide(self.control_id, text),
            Duration::from_millis(250),
        )
        .map_err(io::Error::other)?;
        if reply.status != "queued" {
            return Err(io::Error::other(
                reply
                    .message
                    .unwrap_or_else(|| "guidance was rejected".into()),
            ));
        }
        self.app
            .push_event(Event::Note("Guidance queued; delivery is pending.".into()));
        Ok(())
    }

    pub(super) fn finish(&mut self, elapsed: Option<f64>) -> Option<String> {
        self.control = None;
        let stats = self.stats.take()?;
        self.app
            .push_event(Event::Note("Task run ended. F7: free-form Skott.".into()));
        self.app.running = false;
        self.app.running_since = None;
        self.app.status = "task ended (F7: free-form)".into();
        let elapsed = elapsed.unwrap_or_else(|| stats.started.elapsed().as_secs_f64());
        Some(format!(
            "\n\n**Task statistics — {}:** {} turns | {} | {:.1}s\n\n",
            crate::presentation::literal(&stats.id),
            stats.turns,
            stats.counts(),
            elapsed
        ))
    }

    pub(super) fn observe(&mut self, event: Value) -> Option<String> {
        let kind = event["type"].as_str()?;
        let data = &event["data"];
        match kind {
            "workspace_task_start" => {
                let previous = self.finish(None);
                let id = data.as_str()?.to_owned();
                self.stats = Some(TaskStats::new(id.clone()));
                self.app
                    .push_event(Event::Note(format!("Kvist task: {id}")));
                self.app.status = "waiting for model".into();
                return previous;
            }
            "workspace_task_end" => return self.finish(data["elapsed"].as_f64()),
            "workspace_control" => {
                self.control = data.as_str().map(PathBuf::from);
                self.app.push_event(Event::Note(
                    "The prompt sends guidance to the supervised runner.".into(),
                ));
                return None;
            }
            "workspace_end" => {
                self.control = None;
                let status = data.as_str().unwrap_or("ended");
                self.app
                    .push_event(Event::Note(format!("Model run: {status}")));
                self.app.status = format!("model {status}; Kvist is finishing the task");
                self.app.running = false;
                self.app.running_since = None;
                if let Some(stats) = &mut self.stats {
                    stats.explicit_turns = false;
                }
                return None;
            }
            _ => {}
        }
        let s = self.stats.as_mut()?;
        match kind {
            "workspace_turn" => {
                s.turns = s.turns.saturating_add(1);
                s.explicit_turns = true;
            }
            "workspace_context" => {
                s.limit = data["limit"].as_u64().unwrap_or(0) as usize;
                s.capacity_source = match data["source"].as_str() {
                    Some("provider") => "provider",
                    Some("configured") => "configured",
                    Some("local") => "local",
                    _ => "unavailable",
                };
            }
            "model_request" => {
                s.attempts = s.attempts.saturating_add(1);
                if !s.explicit_turns {
                    s.turns = s.turns.saturating_add(1);
                }
                s.context = data["estimated_input_tokens"].as_u64().unwrap_or(0) as usize;
                s.streamed = false;
                s.current_decode_secs = 0.0;
                let model = data["model"].as_str().unwrap_or("model").to_owned();
                self.app.model = Some(model.clone());
                self.app.push_event(Event::TurnStart { model });
                self.progress(None, None, 0);
            }
            "workspace_decode" => {
                s.current_decode_secs = data["seconds"].as_f64().unwrap_or(0.0);
                self.progress(None, None, data["output"].as_u64().unwrap_or(0));
            }
            "text" | "reasoning" => {
                let text = data.as_str()?.to_owned();
                if kind == "text" {
                    s.streamed = true;
                    self.app.push_event(Event::Text(text));
                } else {
                    self.app.push_event(Event::Reasoning(text));
                }
            }
            "model_result" => {
                if !s.streamed
                    && let Some(text) = data["text"].as_str()
                    && !text.is_empty()
                {
                    self.app.push_event(Event::Text(text.to_owned()));
                }
                let usage = serde_json::from_value::<sav::ModelUsage>(data["usage"].clone()).ok();
                let timings =
                    serde_json::from_value::<sav::ModelTimings>(data["timings"].clone()).ok();
                if let Some(usage) = usage {
                    s.usage.input_tokens = s.usage.input_tokens.saturating_add(usage.input_tokens);
                    s.usage.output_tokens =
                        s.usage.output_tokens.saturating_add(usage.output_tokens);
                    s.usage.total_tokens = s.usage.total_tokens.saturating_add(usage.total_tokens);
                    s.context = usize::try_from(usage.total_tokens).unwrap_or(usize::MAX);
                } else {
                    s.missing_usage = true;
                }
                self.progress(usage, timings, 0);
            }
            "model_failure" => {
                s.missing_usage = true;
                let message = data["message"].as_str().unwrap_or("model request failed");
                self.app.push_event(Event::Note(message.to_owned()));
                self.progress(None, None, 0);
            }
            "tool_dispatch" | "tool_result" => {
                let name = data["name"].as_str().unwrap_or("tool").to_owned();
                if kind == "tool_dispatch" {
                    s.tools = s.tools.saturating_add(1);
                    self.app.push_event(Event::ToolCall {
                        description: name.clone(),
                        name,
                    });
                } else {
                    self.app.push_event(Event::ToolResult {
                        description: name.clone(),
                        name,
                        failed: data["applied"].as_bool() == Some(false),
                    });
                }
            }
            "context_compaction" => s.compactions = s.compactions.saturating_add(1),
            "context_rollover" => s.rollovers = s.rollovers.saturating_add(1),
            "control" => {
                let id = GuidanceId(data["id"].as_u64()?);
                let state = match data["state"].as_str()? {
                    "accepted" => GuidanceState::Accepted,
                    "claimed" => GuidanceState::Claimed,
                    "delivered" => GuidanceState::Delivered,
                    "withdrawn" => GuidanceState::Withdrawn,
                    "dropped" => GuidanceState::Dropped,
                    _ => return None,
                };
                self.app
                    .push_event(Event::Control(GuidanceNotice { id, state }));
            }
            _ => {}
        }
        None
    }

    fn progress(
        &mut self,
        usage: Option<sav::ModelUsage>,
        timings: Option<sav::ModelTimings>,
        streaming_output: u64,
    ) {
        let Some(s) = &mut self.stats else { return };
        let timings = timings.unwrap_or_default();
        let provider_seconds = timings
            .generation_seconds
            .filter(|time| time.is_finite() && *time > 0.0);
        let seconds = provider_seconds.unwrap_or(s.current_decode_secs);
        let output = usage.map_or(streaming_output, |usage| usage.output_tokens);
        if usage.is_some() && seconds > 0.0 {
            s.timed_output = s.timed_output.saturating_add(output);
            s.decode_secs += seconds;
        }
        let rate = if seconds > 0.0 {
            output as f64 / seconds
        } else {
            0.0
        };
        let live_seconds = if streaming_output > 0 { seconds } else { 0.0 };
        let total_seconds = s.decode_secs + live_seconds;
        let context = s.context.saturating_add(streaming_output as usize);
        self.app.push_event(Event::Progress {
            token_accounting: if streaming_output > 0 {
                TokenAccounting::Estimated
            } else if s.missing_usage || s.usage.total_tokens == 0 {
                TokenAccounting::Unavailable
            } else {
                TokenAccounting::Provider
            },
            input_tokens: s.usage.input_tokens,
            output_tokens: s.usage.output_tokens.saturating_add(streaming_output),
            total_tokens: s.usage.total_tokens.saturating_add(streaming_output),
            context_tokens: context,
            context_limit: s.limit,
            context_utilization: if s.limit > 0 {
                context as f64 / s.limit as f64
            } else {
                0.0
            },
            compaction_progress: 0.0,
            compactions: s.compactions,
            elapsed_secs: s.started.elapsed().as_secs_f64(),
            generation_tokens_per_sec: rate,
            average_tokens_per_sec: if total_seconds > 0.0 {
                s.timed_output.saturating_add(streaming_output) as f64 / total_seconds
            } else {
                0.0
            },
            telemetry: ProgressTelemetry {
                context_observed: usage.is_some(),
                capacity_source: s.capacity_source,
                rollovers: s.rollovers,
                generation_provider: usage.is_some() && provider_seconds.is_some(),
                prompt_seconds: timings.prompt_seconds,
                cached_input_tokens: timings.cached_input_tokens,
            },
        });
    }
}

impl Workspace {
    fn free_app(&mut self) -> &mut App {
        self.skott
            .as_mut()
            .map_or(&mut self.free_skott, |session| &mut session.app)
    }

    pub(super) fn skott_app(&mut self) -> &mut App {
        if self.task_skott.shown {
            &mut self.task_skott.app
        } else {
            self.free_app()
        }
    }

    pub(super) fn prepare_skott_view(&mut self) {
        if self.task_skott.shown || !self.free_app().models.is_empty() || self.skott.is_some() {
            return;
        }
        match self.skott_config() {
            Ok(config) => {
                let model = config
                    .default_model()
                    .or_else(|| (config.models.len() == 1).then(|| &config.models[0]))
                    .map(|model| model.id.clone());
                let models: Vec<_> = config.models.iter().map(|model| model.id.clone()).collect();
                let themes_dir = skott::tui::theme_files::themes_dir_for_config(
                    &self.project.join("skott.toml"),
                );
                let app = self.free_app();
                let draft = app.editor.lines().join("\n");
                *app = App::new(
                    &models,
                    model.as_deref(),
                    config.default_thinking_effort,
                    None,
                    app.width,
                    app.height,
                    config.theme,
                );
                app.editor.insert_str(draft);
                app.execution_scope = skott::session_log::ExecutionScope::SandboxedWorkspace;
                app.set_theme_catalog(themes_dir);
                app.set_log_dir(config.working_directory.join(skott::DEFAULT_LOG_DIR));
                app.status = "ready".into();
            }
            Err(error) => {
                let app = self.free_app();
                app.model_discovery_pending = true;
                app.status = "Skott setup required".into();
                app.push_event(Event::Note(error.to_string()));
            }
        }
    }

    fn skott_config(&self) -> io::Result<skott::Config> {
        let config_path = self.project.join("skott.toml");
        skott::Config::load_for_workspace(&config_path, &self.project).map_err(|error| {
            io::Error::other(format!(
                "Skott configuration {}: {}; create one with `skott --init-config` or `skott --setup`",
                config_path.display(),
                error.describe()
            ))
        })
    }

    pub(super) fn start_agent_session(&mut self, prompt: &str) -> io::Result<()> {
        self.task_skott.shown = false;
        self.prepare_skott_view();
        self.send_free_prompt(prompt, true)?;
        self.select_pane(ActivePane::Skott);
        Ok(())
    }

    fn send_free_prompt(&mut self, prompt: &str, echo: bool) -> io::Result<()> {
        if self.running.is_some() {
            return Err(io::Error::other(
                "wait for the running Kvist command before starting free-form Skott work",
            ));
        }
        let model = self.free_app().model.clone();
        let effort = self.free_app().effort;
        let needs_worker = self.skott.as_ref().is_none_or(|session| {
            !session.is_running()
                || session.model_id() != model.as_deref()
                || session.effort() != effort
        });
        if needs_worker {
            if self.skott.as_ref().is_some_and(|session| session.is_busy()) {
                return Err(io::Error::other(
                    "wait for the active Skott turn or restore its model and effort to steer it",
                ));
            }
            let config = self.skott_config()?;
            let selected = match model.as_deref() {
                Some(id) => config.models.iter().find(|model| model.id == id),
                None => config
                    .default_model()
                    .or_else(|| (config.models.len() == 1).then(|| &config.models[0])),
            }
            .cloned()
            .ok_or_else(|| {
                io::Error::other(
                    "no matching model in the project's `skott.toml`; choose a model with Tab",
                )
            })?;
            let mut session = crate::skott_embedded::EmbeddedSkott::start(
                config,
                &selected,
                effort,
                None,
                skott::RunLimits::default(),
                None,
            )
            .map_err(io::Error::other)?;
            let app = self.free_app();
            app.model = Some(selected.id);
            std::mem::swap(app, &mut session.app);
            self.skott = Some(session);
        }
        let session = self
            .skott
            .as_mut()
            .ok_or_else(|| io::Error::other("Skott worker failed to start"))?;
        session
            .send_prompt(prompt.to_owned(), echo)
            .map_err(io::Error::other)
    }

    pub(super) fn skott_key(&mut self, key: KeyEvent) -> io::Result<()> {
        let action = self.skott_app().on_key(key);
        self.skott_app()
            .emit_pending_osc_52()
            .map_err(io::Error::other)?;
        match action {
            KeyAction::Submit => {
                let Some(text) = self.skott_app().take_pending_prompt() else {
                    return Ok(());
                };
                let result = if self.task_skott.shown {
                    if self.running.is_some() {
                        self.task_skott.guide(&text)
                    } else {
                        Err(io::Error::other(
                            "this task has ended; press F7 for free-form Skott",
                        ))
                    }
                } else {
                    self.send_free_prompt(&text, false)
                };
                if let Err(error) = result {
                    let app = self.skott_app();
                    app.status = "input not sent".into();
                    app.editor.insert_str(&text);
                    app.push_event(Event::Note(error.to_string()));
                }
            }
            KeyAction::Cancel if self.task_skott.shown => {
                if let Some(run) = &mut self.running {
                    run.cancel()?;
                }
            }
            KeyAction::Cancel => {
                if let Some(skott) = &mut self.skott {
                    skott.cancel();
                }
            }
            KeyAction::NewSession if self.task_skott.shown => {
                self.task_skott.app.push_event(Event::Note("This pane belongs to Kvist’s task runner; F7 returns to free-form Skott after it stops.".into()));
            }
            KeyAction::NewSession => {
                let (model, effort) = (self.free_app().model.clone(), self.free_app().effort);
                self.skott.take();
                self.free_skott = idle_app();
                self.prepare_skott_view();
                if model.is_some() {
                    self.free_skott.model = model;
                    self.free_skott.effort = effort;
                }
            }
            KeyAction::Quit => {
                self.skott_app().should_quit = false;
                self.select_pane(ActivePane::Kvist);
            }
            KeyAction::Idle => {}
        }
        Ok(())
    }

    pub(super) fn pump_task_view(&mut self) -> io::Result<bool> {
        let Some(feed) = &mut self.task_skott.feed else {
            return Ok(false);
        };
        match feed.drain() {
            Ok((events, more)) => {
                for event in events {
                    if let Some(stats) = self.task_skott.observe(event) {
                        self.append(&stats)?;
                    }
                }

                Ok(more)
            }
            Err(error) => {
                self.task_skott.feed.take();
                self.record_error(&format!("Skott presentation feed: {error}"))?;
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn task_prompt_sends_multiline_guidance_to_the_private_control() {
        use sav::observation::{ControlAction, ControlReply, ControlServer};

        let project = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::new(project.path()).unwrap();
        workspace.start(vec!["--help".into()]).unwrap();
        let (sent, received) = std::sync::mpsc::channel();
        let server = ControlServer::bind(move |request| match request.action {
            ControlAction::Guide { text } => {
                sent.send(text).unwrap();
                ControlReply::queued(request.id)
            }
            ControlAction::Cancel => ControlReply::rejected(request.id, "unexpected cancel"),
        })
        .unwrap();
        workspace.task_skott.shown = true;
        workspace.task_skott.control = Some(server.path().to_owned());
        workspace.task_skott.app.model = Some("fixture".into());
        workspace.select_pane(ActivePane::Skott);
        workspace.paste("Use option B.\nKeep the tests.");
        workspace
            .skott_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(
            received.recv_timeout(Duration::from_secs(1)).unwrap(),
            "Use option B.\nKeep the tests."
        );
        assert!(!workspace.source.contains("Use option B."));
    }

    #[test]
    fn free_form_prompt_editor_reuses_the_conversation_after_a_view_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::new(dir.path()).unwrap();
        workspace.skott = Some(crate::skott_embedded::tests::start_stubbed().0);
        workspace.select_pane(ActivePane::Skott);
        let submit = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
        for text in ["first question\nsecond line", "follow-up question"] {
            workspace.paste(text);
            workspace.skott_key(submit).unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while workspace.skott.as_ref().unwrap().is_busy() {
                assert!(Instant::now() < deadline);
                workspace.pump().unwrap();
                std::thread::sleep(Duration::from_millis(5));
            }
            workspace.select_pane(ActivePane::Kvist);
            workspace.select_pane(ActivePane::Skott);
        }
        let app = workspace.skott_app();
        assert_eq!(
            app.history,
            ["first question\nsecond line", "follow-up question"]
        );
        assert_eq!(app.stats.unwrap().total_tokens, 30);
        assert!(!workspace.source.contains("follow-up question"));
    }

    #[test]
    fn free_form_prompt_steers_an_active_turn_across_layout_changes() {
        let project = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::new(project.path()).unwrap();
        let (session, requests) = crate::skott_embedded::tests::start_steerable_stub();
        workspace.skott = Some(session);
        workspace.select_pane(ActivePane::Skott);
        let submit = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
        workspace.paste("Write an explanation");
        workspace.skott_key(submit).unwrap();
        let first = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            first
                .messages
                .contains(&sav::ModelMessage::User("Write an explanation".into()))
        );

        workspace.select_pane(ActivePane::Kvist);
        assert!(workspace.start(vec!["status".into()]).is_err());
        workspace.select_pane(ActivePane::Skott);
        workspace.paste("Keep it brief");
        workspace.skott_key(submit).unwrap();
        let next = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            next.messages
                .contains(&sav::ModelMessage::User("Write an explanation".into()))
        );
        assert!(
            next.messages
                .contains(&sav::ModelMessage::User("Keep it brief".into()))
        );
    }
}
