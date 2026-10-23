//! Bounded live diagnostics and separately authorized local controls.
//!
//! Neither interface grants tool authority or produces canonical evidence.

mod control;
mod journal;

pub use control::{ControlAction, ControlReply, ControlRequest, ControlServer, send_control};
pub use journal::{JournalFollower, JournalRecord, RunJournal};

fn invalid(reason: impl Into<String>) -> crate::Error {
    crate::Error::Observation {
        reason: reason.into(),
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}
