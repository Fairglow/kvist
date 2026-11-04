//! Full-screen Markdown workspace. The line-oriented host remains independent.

mod editor;
mod process;
mod render;
pub(crate) mod storage;
mod workspace;

pub(crate) use workspace::{available, run};
