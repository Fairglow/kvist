#![cfg(target_os = "linux")]

#[path = "dogfood_boundary/accepted_commit.rs"]
mod accepted_commit;
#[path = "dogfood_boundary/cargo_acquisition.rs"]
mod cargo_acquisition;
#[path = "dogfood_boundary/layout.rs"]
mod layout;
#[path = "dogfood_boundary/protocol.rs"]
mod protocol;
#[path = "dogfood_boundary/support.rs"]
mod support;
#[path = "dogfood_boundary/workflow.rs"]
mod workflow;
