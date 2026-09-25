# Task 5: Transcript tail reader

**Goal:** Read only the last 64 KB of a session transcript and extract model, last-turn input tokens, compaction, exhausted API errors and a pending `AskUserQuestion`/`ExitPlanMode` call. Locate the transcript from `cwd` + `sessionId`.

**Spec:** §3.2 and the transcript fallback in §3.1.

**Read `docs/spike-results.md` first.** The Task 2 live spike confirmed that plan-mode approval
(`ExitPlanMode`) *never* writes `status: "waiting"` to the registry file, even when the prompt sits
open for several seconds — this was checked twice against a real Claude Code 2.1.282 session, not a
timing fluke. That makes this module's `ExitPlanMode` detection the **only** signal for plan-mode
waiting, not a defensive fallback behind the registry. Add a specific test with a transcript fixture
whose last line is a pending `ExitPlanMode` tool call (no matching `tool_result` yet), and be sure the
detection logic here doesn't assume the registry will ever corroborate it.

Real line shapes (observed on this machine; long values elided). Each line is one JSON object:

```json
{"type":"assistant","timestamp":"2026-09-22T01:38:25.464Z","message":{"model":"claude-opus-5","content":[{"type":"tool_use","id":"toolu_01PR","name":"Agent","input":{"description":"Relay detail","subagent_type":"fork","prompt":"…"}}],"stop_reason":"tool_use","usage":{"input_tokens":2,"cache_creation_input_tokens":305,"cache_read_input_tokens":61323,"output_tokens":491}}}
{"type":"user","timestamp":"…","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01PR","content":[{"type":"text","text":"…"}]}]},"toolUseResult":{"isAsync":true,"status":"…","agentId":"a43965c9922febcfc"}}
{"type":"user","message":{"role":"user","content":"please fix the bug"}}
{"type":"system","subtype":"api_error","retryAttempt":10,"maxRetries":10,"error":{"rateLimits":null}}
{"type":"system","subtype":"compact_boundary"}
```

Other `type` values seen and to be ignored: `mode`, `permission-mode`, `attachment`, `last-prompt`, `ai-title`, `file-history-snapshot`, `cost-state`, `queue-operation`; other `system` subtypes: `turn_duration`, `stop_hook_summary`, `away_summary`, `bridge_status`, `local_command`.

**Files:**
- Create: `src/collectors/tail.rs`, `src/collectors/transcript.rs`
- Create: `tests/transcript_io.rs`
- Modify: `src/collectors/mod.rs` (add `pub mod tail; pub mod transcript;`)

**Interfaces:**
- Consumes: `model::TranscriptFacts`, `collectors::strip_bom`.
- Produces:
  - `collectors::tail::{TAIL_BYTES: u64 = 65_536, read_tail(&Path, u64) -> io::Result<(String, bool /*truncated_start*/)>, read_head(&Path, u64) -> io::Result<String>, complete_lines(&str, bool) -> impl Iterator<Item=&str>}`
  - `collectors::transcript::{slug_for_cwd(&str) -> String, find_transcript(projects_dir: &Path, cwd: &str, session_id: &str) -> Option<PathBuf>, parse_tail(text: &str, truncated_start: bool) -> TranscriptFacts, usage_input_total(&serde_json::Value) -> Option<u64>}`

---

- [ ] **Step 1: Write failing unit tests for `tail::complete_lines` and `parse_tail`**

`src/collectors/tail.rs`, tests only for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_partial_first_line_only_when_truncated() {
        let t = "partial}\n{\"a\":1}\r\n\n{\"b\":2}";
        assert_eq!(complete_lines(t, true).collect::<Vec<_>>(), vec!["{\"a\":1}", "{\"b\":2}"]);
        assert_eq!(complete_lines(t, false).count(), 3);
    }
}
```

`src/collectors/transcript.rs`, tests only for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const ASSISTANT: &str = r#"{"type":"assistant","message":{"model":"claude-opus-5","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":2,"cache_creation_input_tokens":305,"cache_read_input_tokens":132300,"output_tokens":10}}}"#;

    fn ask(id: &str, name: &str) -> String {
        format!(r#"{{"type":"assistant","message":{{"model":"claude-opus-5","content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{{}}}}]}}}}"#)
    }
    fn result(id: &str) -> String {
        format!(r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"ok"}}]}}}}"#)
    }

    #[test]
    fn slug_matches_claude_code() {
        assert_eq!(slug_for_cwd(r"C:\Projects\Personal\ClaudeHUD"), "C--Projects-Personal-ClaudeHUD");
        assert_eq!(slug_for_cwd("/home/me/app"), "-home-me-app");
    }

    #[test]
    fn model_and_input_tokens_from_last_assistant_line() {
        let f = parse_tail(ASSISTANT, false);
        assert_eq!(f.model.as_deref(), Some("claude-opus-5"));
        assert_eq!(f.last_turn_input_tokens, Some(132_607));
        assert!(!f.compacted && !f.api_error_exhausted && f.pending_user_tool.is_none());
    }

    #[test]
    fn synthetic_model_is_ignored() {
        let synth = ASSISTANT.replace("claude-opus-5", "<synthetic>");
        let f = parse_tail(&format!("{ASSISTANT}\n{synth}"), false);
        assert_eq!(f.model.as_deref(), Some("claude-opus-5"));
    }

    #[test]
    fn pending_question_until_answered() {
        let t = format!("{ASSISTANT}\n{}", ask("t1", "AskUserQuestion"));
        assert_eq!(parse_tail(&t, false).pending_user_tool.as_deref(), Some("AskUserQuestion"));
        let t2 = format!("{t}\n{}", result("t1"));
        assert_eq!(parse_tail(&t2, false).pending_user_tool, None);
        let plan = ask("t2", "ExitPlanMode");
        assert_eq!(parse_tail(&plan, false).pending_user_tool.as_deref(), Some("ExitPlanMode"));
    }

    #[test]
    fn other_pending_tools_are_not_questions() {
        let t = ask("t3", "Bash");
        assert_eq!(parse_tail(&t, false).pending_user_tool, None);
    }

    #[test]
    fn new_user_prompt_clears_pending() {
        let t = format!("{}\n{}", ask("t1", "AskUserQuestion"), r#"{"type":"user","message":{"role":"user","content":"never mind"}}"#);
        assert_eq!(parse_tail(&t, false).pending_user_tool, None);
    }

    #[test]
    fn api_error_only_when_exhausted_or_rate_limited() {
        let low = r#"{"type":"system","subtype":"api_error","retryAttempt":2,"maxRetries":10,"error":{"rateLimits":null}}"#;
        let high = r#"{"type":"system","subtype":"api_error","retryAttempt":10,"maxRetries":10,"error":{}}"#;
        let rl = r#"{"type":"system","subtype":"api_error","retryAttempt":1,"maxRetries":10,"error":{"rateLimits":{"x":1}}}"#;
        assert!(!parse_tail(low, false).api_error_exhausted);
        assert!(parse_tail(high, false).api_error_exhausted);
        assert!(parse_tail(rl, false).api_error_exhausted);
        let cleared = format!("{high}\n{ASSISTANT}");
        assert!(!parse_tail(&cleared, false).api_error_exhausted, "a later assistant line clears it");
    }

    #[test]
    fn compact_boundary_marks_compacted() {
        let t = format!("{ASSISTANT}\n{}\n{ASSISTANT}", r#"{"type":"system","subtype":"compact_boundary"}"#);
        assert!(parse_tail(&t, false).compacted);
    }

    #[test]
    fn torn_last_line_and_partial_first_line_are_dropped() {
        let t = format!("tail of an older line\"}}\n{ASSISTANT}\n{{\"type\":\"assistant\",\"message\":{{\"model\":\"claude-hai");
        let f = parse_tail(&t, true);
        assert_eq!(f.model.as_deref(), Some("claude-opus-5"));
    }

    #[test]
    fn crlf_lines_parse() {
        let t = format!("{ASSISTANT}\r\n{}\r\n", ask("t9", "AskUserQuestion"));
        assert_eq!(parse_tail(&t, false).pending_user_tool.as_deref(), Some("AskUserQuestion"));
    }

    #[test]
    fn unknown_records_are_ignored() {
        let t = format!("{}\n{ASSISTANT}\n{}", r#"{"type":"ai-title","title":"x"}"#, r#"{"type":"system","subtype":"turn_duration","ms":5}"#);
        assert_eq!(parse_tail(&t, false).last_turn_input_tokens, Some(132_607));
    }
}
```

Add `pub mod tail;` and `pub mod transcript;` to `src/collectors/mod.rs`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib collectors::`
Expected: compile errors.

- [ ] **Step 3: Implement `tail.rs`**

Above its tests:

```rust
//! Bounded reads of append-only JSONL files that another process is writing.
//! `File::open` on Windows shares read/write/delete, so Claude Code is never blocked.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub const TAIL_BYTES: u64 = 65_536;

/// The last `max` bytes. `truncated_start` is true when the read began mid-file,
/// so the first line is probably partial.
pub fn read_tail(path: &Path, max: u64) -> io::Result<(String, bool)> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(max);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.take(max).read_to_end(&mut buf)?;
    Ok((String::from_utf8_lossy(&buf).into_owned(), start > 0))
}

/// The first `max` bytes (the last line may be partial).
pub fn read_head(path: &Path, max: u64) -> io::Result<String> {
    let f = File::open(path)?;
    let mut buf = Vec::new();
    f.take(max).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Non-empty lines without `\r`. Drops the first line when `truncated_start`.
/// A torn final line is still yielded; callers skip lines that fail to parse.
pub fn complete_lines(text: &str, truncated_start: bool) -> impl Iterator<Item = &str> {
    let mut it = text.split('\n');
    if truncated_start {
        it.next();
    }
    it.map(|l| l.trim_end_matches('\r')).filter(|l| !l.trim().is_empty())
}
```

- [ ] **Step 4: Implement `transcript.rs`**

Above its tests:

```rust
//! `~/.claude/projects/<slug>/<sessionId>.jsonl` (§3.2).

use super::strip_bom;
use super::tail::complete_lines;
use crate::model::TranscriptFacts;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const USER_TOOLS: [&str; 2] = ["AskUserQuestion", "ExitPlanMode"];

/// cwd with `:` `\` `/` replaced by `-`, case preserved.
pub fn slug_for_cwd(cwd: &str) -> String {
    cwd.chars().map(|c| if matches!(c, ':' | '\\' | '/') { '-' } else { c }).collect()
}

/// Tries `<projects>/<slug>/<id>.jsonl`, then every project folder (the drive
/// letter's case differs between entrypoints, and other characters may be escaped).
pub fn find_transcript(projects_dir: &Path, cwd: &str, session_id: &str) -> Option<PathBuf> {
    let file = format!("{session_id}.jsonl");
    let direct = projects_dir.join(slug_for_cwd(cwd)).join(&file);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(projects_dir)
        .ok()?
        .flatten()
        .map(|d| d.path().join(&file))
        .find(|p| p.is_file())
}

/// input + cache_read + cache_creation tokens of a `message.usage` object.
pub fn usage_input_total(usage: &Value) -> Option<u64> {
    let get = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let total = get("input_tokens") + get("cache_read_input_tokens") + get("cache_creation_input_tokens");
    (total > 0).then_some(total)
}

pub fn parse_tail(text: &str, truncated_start: bool) -> TranscriptFacts {
    let mut facts = TranscriptFacts::default();
    // (tool_use_id, tool name) in order of appearance
    let mut calls: Vec<(String, String)> = Vec::new();
    let mut resolved: HashSet<String> = HashSet::new();

    for line in complete_lines(strip_bom(text), truncated_start) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        match v.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                let msg = &v["message"];
                if let Some(m) = msg.get("model").and_then(Value::as_str) {
                    if !m.starts_with('<') {
                        facts.model = Some(m.to_string());
                    }
                }
                if let Some(t) = msg.get("usage").and_then(usage_input_total) {
                    facts.last_turn_input_tokens = Some(t);
                }
                facts.api_error_exhausted = false;
                for c in msg.get("content").and_then(Value::as_array).into_iter().flatten() {
                    if c.get("type").and_then(Value::as_str) == Some("tool_use") {
                        if let (Some(id), Some(name)) = (c.get("id").and_then(Value::as_str), c.get("name").and_then(Value::as_str)) {
                            calls.push((id.to_string(), name.to_string()));
                        }
                    }
                }
            }
            Some("user") => match v["message"].get("content") {
                Some(Value::String(_)) => calls.clear(), // a fresh prompt: nothing is pending any more
                Some(Value::Array(blocks)) => {
                    for b in blocks {
                        if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                            if let Some(id) = b.get("tool_use_id").and_then(Value::as_str) {
                                resolved.insert(id.to_string());
                            }
                        } else if b.get("type").and_then(Value::as_str) == Some("text") {
                            calls.clear();
                        }
                    }
                }
                _ => {}
            },
            Some("system") => match v.get("subtype").and_then(Value::as_str) {
                Some("api_error") => {
                    let attempt = v.get("retryAttempt").and_then(Value::as_u64);
                    let max = v.get("maxRetries").and_then(Value::as_u64);
                    let rate_limited = v
                        .get("error")
                        .and_then(|e| e.get("rateLimits"))
                        .is_some_and(|r| !r.is_null());
                    let exhausted = matches!((attempt, max), (Some(a), Some(m)) if a >= m);
                    if rate_limited || exhausted {
                        facts.api_error_exhausted = true;
                    }
                }
                Some("compact_boundary") => facts.compacted = true,
                _ => {}
            },
            _ => {}
        }
    }

    facts.pending_user_tool = calls
        .iter()
        .rev()
        .find(|(id, name)| !resolved.contains(id) && USER_TOOLS.contains(&name.as_str()))
        .map(|(_, name)| name.clone());
    facts
}
```

- [ ] **Step 5: Run unit tests**

Run: `cargo test --lib collectors::`
Expected: 12 passed (1 tail + 11 transcript).

- [ ] **Step 6: Write IO integration tests**

`tests/transcript_io.rs`:

```rust
mod common;

use claudehud::collectors::tail::{read_tail, TAIL_BYTES};
use claudehud::collectors::transcript::{find_transcript, parse_tail};

const LINE: &str = r#"{"type":"assistant","message":{"model":"claude-opus-5","content":[],"usage":{"input_tokens":1,"cache_read_input_tokens":99,"cache_creation_input_tokens":0}}}"#;

#[test]
fn reads_only_the_tail_of_a_large_file() {
    let t = common::TempDir::new("tail-large");
    let filler = format!("{{\"type\":\"attachment\",\"pad\":\"{}\"}}\n", "x".repeat(1000));
    let mut body = filler.repeat(200); // ~200 KB
    body.push_str(LINE);
    body.push('\n');
    let p = t.write("big.jsonl", &body);
    let (text, truncated) = read_tail(&p, TAIL_BYTES).unwrap();
    assert!(truncated);
    assert!(text.len() as u64 <= TAIL_BYTES);
    assert_eq!(parse_tail(&text, truncated).last_turn_input_tokens, Some(100));
}

#[test]
fn small_file_is_not_truncated() {
    let t = common::TempDir::new("tail-small");
    let p = t.write("s.jsonl", LINE);
    let (text, truncated) = read_tail(&p, TAIL_BYTES).unwrap();
    assert!(!truncated);
    assert_eq!(parse_tail(&text, truncated).model.as_deref(), Some("claude-opus-5"));
}

#[test]
fn finds_transcript_by_slug_then_by_scan() {
    let t = common::TempDir::new("find");
    let projects = t.path().join("projects");
    let direct = t.write("projects/C--work-app/s1.jsonl", LINE);
    assert_eq!(find_transcript(&projects, r"C:\work\app", "s1"), Some(direct));
    // stored under a lower-case drive slug: found by scanning
    let other = t.write("projects/c--other-thing/s2.jsonl", LINE);
    assert_eq!(find_transcript(&projects, r"D:\elsewhere", "s2"), Some(other));
    assert_eq!(find_transcript(&projects, r"C:\work\app", "missing"), None);
}
```

- [ ] **Step 7: Run it**

Run: `cargo test --test transcript_io`
Expected: 3 passed.

- [ ] **Step 8: Lint and commit**

Run: `cargo clippy --all-targets -- -D warnings` and `cargo fmt`.

```powershell
git add src/collectors tests/transcript_io.rs
git commit -m "feat(transcript): bounded tail read and transcript facts"
```
