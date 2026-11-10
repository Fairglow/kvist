//! Embedded Skott integration for Kvist.
//!
//! This module embeds Skott's functionality directly into Kvist, providing:
//! - A split-pane TUI interface with keyboard navigation
//! - Bidirectional communication between Kvist and Skott
//! - Real-time metrics display and processing
//! - Prompt/guidance channel from Kvist to Skott

use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use ratatui::backend::Backend;
use ratatui::Terminal;
use crossterm::event::{KeyEvent, KeyModifiers, KeyCode};

use skott::{
    config::Config,
    session::{AgentSession, RunSummary, RunLimits},
    start_session,
};

/// Represents the embedded Skott instance within Kvist.
pub struct EmbeddedSkott {
    /// The Skott configuration
    config: Config,
    /// Session state
    session: Arc<Mutex<Option<AgentSession>>>,
    /// Metrics collected from Skott
    metrics: Arc<Mutex<SkottMetrics>>,
    /// Communication channels
    prompt_channel: mpsc::Sender<String>,
    response_channel: mpsc::Receiver<String>,
    /// TUI state
    terminal: Option<Terminal<Box<dyn Backend<Error = std::io::Error>>>>,
    /// Active pane (0 = Kvist, 1 = Skott)
    active_pane: u8,
}

/// Metrics collected from Skott execution
#[derive(Debug, Clone, Default)]
pub struct SkottMetrics {
    pub turns: usize,
    pub tool_calls: usize,
    pub tokens_input: u64,
    pub tokens_output: u64,
    pub elapsed_secs: f64,
    pub compactions: usize,
    pub avg_tokens_per_sec: f64,
    pub last_update: Option<Instant>,
}

impl EmbeddedSkott {
    /// Create a new embedded Skott instance
    pub fn new(config: Config) -> Result<Self, crate::KvistError> {
        let (prompt_tx, prompt_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        
        Ok(Self {
            config,
            session: Arc::new(Mutex::new(None)),
            metrics: Arc::new(Mutex::new(SkottMetrics::default())),
            prompt_channel: prompt_tx,
            response_channel: response_rx,
            terminal: None,
            active_pane: 0,
        })
    }
    
    /// Start the embedded Skott session
    pub fn start_session(&mut self) -> Result<(), crate::KvistError> {
        // Initialize Skott session with embedded configuration
        let session = start_session(
            &self.config,
            RunLimits::default(),
        )?;
        
        *self.session.lock().unwrap() = Some(session);
        Ok(())
    }
    
    /// Send a prompt/guidance to Skott
    pub fn send_prompt(&self, prompt: String) -> Result<(), crate::KvistError> {
        self.prompt_channel.send(prompt).map_err(|e| {
            crate::KvistError::SkottCommunicationFailed {
                reason: e.to_string(),
            }
        })
    }
    
    /// Receive response from Skott
    pub fn receive_response(&self) -> Result<String, crate::KvistError> {
        self.response_channel.recv().map_err(|e| {
            crate::KvistError::SkottCommunicationFailed {
                reason: e.to_string(),
            }
        })
    }
    
    /// Update metrics from Skott
    pub fn update_metrics(&self, summary: &RunSummary) {
        let mut metrics = self.metrics.lock().unwrap();
        metrics.turns = summary.turns;
        metrics.tool_calls = summary.tool_calls;
        metrics.tokens_input = summary.tokens_input;
        metrics.tokens_output = summary.tokens_output;
        metrics.elapsed_secs = summary.elapsed_secs;
        metrics.compactions = summary.compactions;
        metrics.avg_tokens_per_sec = if summary.elapsed_secs > 0.0 {
            (summary.tokens_input + summary.tokens_output) as f64 / summary.elapsed_secs
        } else {
            0.0
        };
        metrics.last_update = Some(Instant::now());
    }
    
    /// Toggle between Kvist and Skott panes
    pub fn toggle_pane(&mut self) {
        self.active_pane = if self.active_pane == 0 { 1 } else { 0 };
    }
    
    /// Handle keyboard events for pane navigation
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Tab => {
                self.toggle_pane();
                true
            },
            KeyCode::LeftCtrl | KeyCode::RightCtrl => {
                // Ctrl+arrow for detailed navigation
                if key.modifiers == KeyModifiers::CONTROL {
                    return true;
                }
                false
            },
            _ => false,
        }
    }
    
    /// Get current metrics for display
    pub fn get_metrics(&self) -> SkottMetrics {
        self.metrics.lock().unwrap().clone()
    }
}
