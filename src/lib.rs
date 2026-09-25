//! ClaudeHUD: a Claude Code status light for the Windows desktop.
//!
//! Everything outside `platform` is plain Rust with no Windows dependency, so it
//! can be unit-tested on any machine. Modules are added task by task.
pub mod collectors;
pub mod fixture;
pub mod format;
pub mod icon;
pub mod latch;
pub mod model;
pub mod state;
pub mod timefmt;
pub mod tooltip;
