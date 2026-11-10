//! Skott integration for live transcript display.
//!
//! This module handles launching and attaching to Skott sessions for real-time
//! transcript viewing while Kvist focuses on orchestration.
//!
//! Skott reports metrics back to Kvist via stdout in a structured format that
//! Kvist can parse. This creates a clean separation: Skott handles display and
//! metrics, Kvist handles orchestration.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

/// Represents a Skott session that can be attached to or launched.
pub struct SkottSession {
    pub session_id: String,
    pub task_id: String,
    pub attached: bool,
    pub running: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkottMetrics {
    pub turns: usize,
    pub tool_calls: usize,
    pub tokens_input: u64,
    pub tokens_output: u64,
    pub elapsed_secs: f64,
    pub compactions: usize,
}

impl SkottSession {
    /// Launch a new Skott session for the given task.
    /// Skott will report metrics back to Kvist via stdout.
    pub fn launch(task_id: &str, model: Option<&str>) -> Result<Self, crate::KvistError> {
        let session_id = format!("kvist-{}", uuid::Uuid::new_v4().simple());
        
        // Launch skott with metrics output enabled
        let mut cmd = Command::new("skott");
        cmd.arg("--task-id")
            .arg(task_id)
            .arg("--session-id")
            .arg(&session_id)
            .arg("--metrics-output")
            .arg("stdout"); // Skott reports metrics to stdout for Kvist to parse
        
        if let Some(model) = model {
            cmd.arg("--model").arg(model);
        }
        
        // Start skott with stdout captured for metrics
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());
        
        cmd.spawn().map_err(|e| crate::KvistError::SkottLaunchFailed {
            reason: e.to_string(),
        })?;
        
        Ok(Self {
            session_id,
            task_id: task_id.to_owned(),
            attached: false,
            running: Arc::new(AtomicBool::new(true)),
        })
    }
    
    /// Attach to an existing Skott session.
    pub fn attach(session_id: &str) -> Result<Self, crate::KvistError> {
        let mut cmd = Command::new("skott");
        cmd.arg("--attach")
            .arg(session_id)
            .arg("--metrics-output")
            .arg("stdout");
        
        cmd.spawn().map_err(|e| crate::KvistError::SkottAttachFailed {
            reason: e.to_string(),
        })?;
        
        Ok(Self {
            session_id: session_id.to_owned(),
            task_id: String::new(),
            attached: true,
            running: Arc::new(AtomicBool::new(true)),
        })
    }
    
    /// Stop the Skott session.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}
