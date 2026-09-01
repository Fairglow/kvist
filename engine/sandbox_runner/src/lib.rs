#![forbid(unsafe_code)]

#[cfg(not(target_os = "linux"))]
compile_error!("kvist-sandbox-runner currently supports Linux only");

/// Honest status exposed by the buildable pre-protocol scaffold.
pub const IMPLEMENTATION_STATUS: &str =
    "kvist-sandbox-runner is a scaffold; sandbox enforcement is not implemented";
