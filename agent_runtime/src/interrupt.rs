//! Process-level interrupt registry shared by the engine and the runtime.
//!
//! A single `sigaction` handler for SIGINT and SIGTERM records an atomic
//! interrupt flag and forwards the signal to the currently active child
//! process group. Supervision loops (the engine sandbox supervisor and the
//! host supervisor) register their process group while a child runs and poll
//! the flag between supervision steps.
//!
//! The handler only performs async-signal-safe operations: atomic stores and
//! a `killpg` call. It never allocates, locks, or formats.

use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

/// Set when SIGINT or SIGTERM arrives while the process is running.
static INTERRUPT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Process group of the currently supervised child; zero means none.
static ACTIVE_PROCESS_GROUP: AtomicI64 = AtomicI64::new(0);

/// Installs the handler exactly once per process.
static HANDLER_INSTALLED: Once = Once::new();

/// Signal handler body. Must stay async-signal-safe.
extern "C" fn signal_handler(_signal: i32) {
    INTERRUPT_REQUESTED.store(true, Ordering::SeqCst);
    let pgid = ACTIVE_PROCESS_GROUP.load(Ordering::SeqCst);
    if pgid > 0
        && let Ok(pgid) = i32::try_from(pgid)
    {
        let _ = killpg(Pid::from_raw(pgid), Signal::SIGINT);
    }
}

/// Installs the shared SIGINT/SIGTERM handler, idempotently.
///
/// # Safety contract
///
/// This is the one `unsafe` block in the crate, and it is required because
/// installing a signal handler cannot be expressed safely in Rust. The
/// invariant that makes it sound:
///
/// - `signal_handler` performs only async-signal-safe operations (atomic
///   stores and `killpg`); it never allocates, locks, formats, or calls
///   into non-async-signal-safe runtime state.
/// - Installation happens exactly once per process (`Once`), so concurrent
///   installs cannot race, and the handler outlives the process.
/// - `SA_RESTART` is set so in-flight system calls resume instead of
///   failing with `EINTR`; supervision loops observe the interrupt through
///   [`take_interrupted`] on their next bounded poll.
pub fn install_handler() {
    HANDLER_INSTALLED.call_once(|| {
        use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, sigaction};
        let action = SigAction::new(
            SigHandler::Handler(signal_handler),
            SaFlags::SA_RESTART,
            SigSet::empty(),
        );
        // SAFETY: see the safety contract on this function.
        if let Err(error) = unsafe { sigaction(Signal::SIGINT, &action) } {
            tracing::warn!(
                ?error,
                "could not install SIGINT handler; interrupts fall back to default behavior"
            );
        }
        // SAFETY: see the safety contract on this function.
        if let Err(error) = unsafe { sigaction(Signal::SIGTERM, &action) } {
            tracing::warn!(
                ?error,
                "could not install SIGTERM handler; interrupts fall back to default behavior"
            );
        }
    });
}

/// Returns and clears the interrupt flag.
pub fn take_interrupted() -> bool {
    INTERRUPT_REQUESTED.swap(false, Ordering::SeqCst)
}

/// Registers the active child process group for signal forwarding.
pub fn set_active_process_group(pgid: i32) {
    ACTIVE_PROCESS_GROUP.store(i64::from(pgid), Ordering::SeqCst);
}

/// Clears the active child process group registration.
pub fn clear_active_process_group() {
    ACTIVE_PROCESS_GROUP.store(0, Ordering::SeqCst);
}

/// Clears the registration only when it still holds `pgid`, so a dropped
/// scope cannot clobber a newer registration.
pub fn clear_active_process_group_if(pgid: i32) -> bool {
    ACTIVE_PROCESS_GROUP
        .compare_exchange(i64::from(pgid), 0, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn install_handler_is_idempotent_and_flag_round_trips() {
        install_handler();
        install_handler();
        assert!(!take_interrupted());
        INTERRUPT_REQUESTED.store(true, Ordering::SeqCst);
        assert!(take_interrupted());
        assert!(!take_interrupted());
    }

    #[test]
    fn handler_forwards_to_the_active_process_group() {
        use std::os::unix::process::CommandExt;

        // Spawn a child in its own process group so the handler can signal it.
        let mut command = Command::new("sleep");
        command.arg("60").process_group(0);
        let mut child = command.spawn().expect("spawn sleep");
        let pgid = i32::try_from(child.id()).expect("child id fits");
        set_active_process_group(pgid);

        signal_handler(2);

        assert!(take_interrupted());
        // The child must observe the forwarded SIGINT and exit.
        let status = child.wait().expect("wait for child");
        clear_active_process_group();
        assert!(!status.success());
    }
}
