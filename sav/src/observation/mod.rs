//! Bounded live diagnostics and separately authorized local controls.
//!
//! Neither interface grants tool authority or produces canonical evidence.

mod control;
mod journal;

pub use control::{ControlAction, ControlReply, ControlRequest, ControlServer, send_control};
pub use journal::{JournalFollower, JournalRecord, RunJournal};

/// Binds explicit private process controls to one runtime's bounded mailbox.
///
/// Guidance cannot start work. The returned listener must remain owned by the
/// host, and its pathname must not be placed in agent context or journals.
pub fn bind_runtime_controls(
    controls: &crate::runtime::ControlMailbox,
    cancellation: &crate::CancellationToken,
) -> crate::Result<ControlServer> {
    let controls = controls.clone();
    let cancellation = cancellation.clone();
    ControlServer::bind(move |request| match request.action {
        ControlAction::Cancel => match controls.cancel_active(&cancellation) {
            Ok(true) => ControlReply::cancellation_requested(request.id),
            Ok(false) => ControlReply::rejected(request.id, "no active run"),
            Err(_) => ControlReply::rejected(request.id, "runtime control is unavailable"),
        },
        ControlAction::Guide { text } => match controls.steer(text) {
            Ok(id) => {
                let mut reply = ControlReply::queued(request.id);
                reply.message = Some(format!("guidance #{} accepted; not yet delivered", id.0));
                reply
            }
            Err(error) => ControlReply::rejected(request.id, error.to_string()),
        },
    })
}

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
