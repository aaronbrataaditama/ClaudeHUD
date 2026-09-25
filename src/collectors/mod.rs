//! Readers for Claude Code's files and Anthropic's endpoints. Parsers are pure;
//! functions that touch the filesystem are named `scan_*`, `read_*`, `list_*` or `find_*`.

pub mod registry;
pub mod status;

/// Windows tools often write a UTF-8 BOM; serde_json rejects it.
pub fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}
