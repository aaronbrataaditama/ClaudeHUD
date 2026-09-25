//! ClaudeHUD: a Claude Code status light for the Windows desktop.
//!
//! Everything outside `platform` is plain Rust with no Windows dependency, so it
//! can be unit-tested on any machine. Modules are added task by task.
pub mod collectors;
pub mod format;
pub mod model;
pub mod timefmt;
