# Task 6: Sub-agent listing

**Goal:** For one session, list its recent sub-agents with type, description, depth, state (running / done / failed / stopped), start/end time and context tokens, from `projects/<slug>/<sessionId>/subagents/`.

**Spec:** §3.3. **Deviation from the spec, decided here:** background agents return a `tool_result` immediately (`toolUseResult.isAsync = true`), so the parent's tool result cannot tell whether an agent is still running. State is read from the agent's own transcript instead:
- **done**: last assistant line has `message.stop_reason == "end_turn"` and nothing after it
- **failed**: an exhausted `api_error` is the latest event
- **running**: otherwise, if the file changed in the last 10 min
- **stopped**: otherwise (interrupted or abandoned)

Agents are listed if their transcript changed in the last 15 min. This replaces "kept until the parent's next turn".

Observed files (this machine), per agent `<id>`:
- `agent-<id>.meta.json`: `{"agentType":"fork","isFork":true,"description":"Relay Git Bash terminal detail","toolUseId":"toolu_01PRu8mVzKdnAoz61zhhpBfu","spawnDepth":1,"requestShape":"background","requestNonInteractive":false,"model":"inherit"}`
- `agent-<id>.jsonl`: first line `{"type":"fork-context-ref","agentId":"a43965c9922febcfc","parentSessionId":"…"}` (no timestamp), then normal `assistant`/`user` lines like the session transcript, each with `"timestamp"`.

**Files:**
- Create: `src/collectors/subagents.rs`
- Create: `tests/subagents_io.rs`
- Modify: `src/collectors/mod.rs` (add `pub mod subagents;`)

**Interfaces:**
- Consumes: `tail::{read_tail, read_head, complete_lines, TAIL_BYTES}`, `transcript::usage_input_total`, `timefmt::parse_iso8601`, `model::{Subagent, AgentState}`, `collectors::strip_bom`.
- Produces: `collectors::subagents::{AgentMeta, AgentTail, parse_meta(&str) -> Result<AgentMeta,String>, parse_agent_tail(&str, bool) -> AgentTail, first_timestamp_ms(&str) -> Option<i64>, classify(&AgentTail, mtime_ms: i64, now_ms: i64) -> AgentState, list_subagents(session_dir: &Path, now_ms: i64) -> Vec<Subagent>, LIST_WINDOW_MS, STALE_MS}`. `session_dir` is the transcript path without `.jsonl` (`transcript_path.with_extension("")`).

---

- [x] **Step 1: Write failing unit tests**

`src/collectors/subagents.rs`, tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const META: &str = r#"{"agentType":"fork","isFork":true,"description":"Relay Git Bash terminal detail","toolUseId":"toolu_01PR","spawnDepth":1,"requestShape":"background","model":"inherit"}"#;
    const REF: &str = r#"{"type":"fork-context-ref","agentId":"a4396","parentSessionId":"p"}"#;
    fn assistant(ts: &str, stop: &str) -> String {
        format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"model":"claude-sonnet-5","stop_reason":"{stop}","content":[],"usage":{{"input_tokens":2,"cache_read_input_tokens":41898,"cache_creation_input_tokens":0}}}}}}"#)
    }
    fn user(ts: &str) -> String {
        format!(r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"x","content":"ok"}}]}}}}"#)
    }

    #[test]
    fn parses_meta_and_defaults() {
        let m = parse_meta(META).unwrap();
        assert_eq!(m.agent_type, "fork");
        assert_eq!(m.description, "Relay Git Bash terminal detail");
        assert_eq!(m.tool_use_id.as_deref(), Some("toolu_01PR"));
        assert_eq!(m.spawn_depth, 1);
        let d = parse_meta("{}").unwrap();
        assert_eq!((d.agent_type.as_str(), d.spawn_depth), ("agent", 1));
        assert!(parse_meta("not json").is_err());
    }

    #[test]
    fn end_turn_means_finished_until_more_activity() {
        let a = assistant("2026-09-25T10:00:05Z", "tool_use");
        let b = assistant("2026-09-25T10:03:10Z", "end_turn");
        let t = parse_agent_tail(&format!("{REF}\n{a}\n{b}"), false);
        assert!(t.finished && !t.failed);
        assert_eq!(t.context_tokens, Some(41_900));
        assert_eq!(t.last_ts_ms, Some(crate::timefmt::parse_iso8601("2026-09-25T10:03:10Z").unwrap() * 1000));
        let more = parse_agent_tail(&format!("{a}\n{b}\n{}", user("2026-09-25T10:03:11Z")), false);
        assert!(!more.finished, "a later line means it continued");
    }

    #[test]
    fn exhausted_api_error_is_failure() {
        let e = r#"{"type":"system","subtype":"api_error","retryAttempt":10,"maxRetries":10}"#;
        let t = parse_agent_tail(&format!("{}\n{e}", assistant("2026-09-25T10:00:05Z", "tool_use")), false);
        assert!(t.failed);
    }

    #[test]
    fn first_timestamp_skips_lines_without_one() {
        let h = format!("{REF}\n{}\n{}", assistant("2026-09-25T10:00:05Z", "tool_use"), assistant("2026-09-25T10:00:09Z", "tool_use"));
        assert_eq!(first_timestamp_ms(&h), Some(crate::timefmt::parse_iso8601("2026-09-25T10:00:05Z").unwrap() * 1000));
        assert_eq!(first_timestamp_ms(REF), None);
    }

    #[test]
    fn classification() {
        let now = 10_000_000;
        let running = AgentTail::default();
        assert_eq!(classify(&running, now - 60_000, now), AgentState::Running);
        assert_eq!(classify(&running, now - STALE_MS - 1, now), AgentState::Stopped);
        let done = AgentTail { finished: true, ..Default::default() };
        assert_eq!(classify(&done, now - STALE_MS - 1, now), AgentState::Done);
        let failed = AgentTail { failed: true, finished: true, ..Default::default() };
        assert_eq!(classify(&failed, now, now), AgentState::Failed);
    }
}
```

Add `pub mod subagents;` to `src/collectors/mod.rs`.

- [x] **Step 2: Run to verify failure**

Run: `cargo test --lib collectors::subagents`
Expected: compile errors.

- [x] **Step 3: Implement**

Above the tests:

```rust
//! `projects/<slug>/<sessionId>/subagents/agent-<id>.{jsonl,meta.json}` (§3.3).

use super::strip_bom;
use super::tail::{complete_lines, read_head, read_tail, TAIL_BYTES};
use super::transcript::usage_input_total;
use crate::model::{AgentState, Subagent};
use crate::timefmt::parse_iso8601;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// Agents whose transcript changed within this window are listed.
pub const LIST_WINDOW_MS: i64 = 15 * 60_000;
/// An unfinished agent with no writes for this long is shown as stopped.
pub const STALE_MS: i64 = 10 * 60_000;

#[derive(Clone, Debug, PartialEq)]
pub struct AgentMeta {
    pub agent_type: String,
    pub description: String,
    pub tool_use_id: Option<String>,
    pub spawn_depth: u32,
}

impl Default for AgentMeta {
    fn default() -> Self {
        AgentMeta { agent_type: "agent".to_string(), description: String::new(), tool_use_id: None, spawn_depth: 1 }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMeta {
    agent_type: Option<String>,
    description: Option<String>,
    tool_use_id: Option<String>,
    spawn_depth: Option<u32>,
}

pub fn parse_meta(text: &str) -> Result<AgentMeta, String> {
    let r: RawMeta = serde_json::from_str(strip_bom(text)).map_err(|e| e.to_string())?;
    let d = AgentMeta::default();
    Ok(AgentMeta {
        agent_type: r.agent_type.filter(|s| !s.is_empty()).unwrap_or(d.agent_type),
        description: r.description.unwrap_or_default(),
        tool_use_id: r.tool_use_id,
        spawn_depth: r.spawn_depth.unwrap_or(1).max(1),
    })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentTail {
    pub finished: bool,
    pub failed: bool,
    pub last_ts_ms: Option<i64>,
    pub context_tokens: Option<u64>,
}

fn line_ts_ms(v: &Value) -> Option<i64> {
    v.get("timestamp").and_then(Value::as_str).and_then(parse_iso8601).map(|s| s * 1000)
}

pub fn parse_agent_tail(text: &str, truncated_start: bool) -> AgentTail {
    let mut t = AgentTail::default();
    for line in complete_lines(strip_bom(text), truncated_start) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if let Some(ts) = line_ts_ms(&v) {
            t.last_ts_ms = Some(ts);
        }
        match v.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                let msg = &v["message"];
                if let Some(c) = msg.get("usage").and_then(usage_input_total) {
                    t.context_tokens = Some(c);
                }
                t.finished = msg.get("stop_reason").and_then(Value::as_str) == Some("end_turn");
                t.failed = false;
            }
            Some("user") => t.finished = false,
            Some("system") if v.get("subtype").and_then(Value::as_str) == Some("api_error") => {
                let a = v.get("retryAttempt").and_then(Value::as_u64);
                let m = v.get("maxRetries").and_then(Value::as_u64);
                if matches!((a, m), (Some(a), Some(m)) if a >= m) {
                    t.failed = true;
                }
            }
            _ => {}
        }
    }
    t
}

pub fn first_timestamp_ms(head: &str) -> Option<i64> {
    complete_lines(strip_bom(head), false)
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find_map(|v| line_ts_ms(&v))
}

pub fn classify(t: &AgentTail, mtime_ms: i64, now_ms: i64) -> AgentState {
    if t.failed {
        AgentState::Failed
    } else if t.finished {
        AgentState::Done
    } else if now_ms - mtime_ms <= STALE_MS {
        AgentState::Running
    } else {
        AgentState::Stopped
    }
}

fn mtime_ms(p: &Path) -> Option<i64> {
    let d = std::fs::metadata(p).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(d.as_millis() as i64)
}

/// Running agents first, then newest first.
pub fn list_subagents(session_dir: &Path, now_ms: i64) -> Vec<Subagent> {
    let dir = session_dir.join("subagents");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out = Vec::new();
    for d in rd.flatten() {
        let name = d.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_prefix("agent-").and_then(|n| n.strip_suffix(".jsonl")) else { continue };
        let path = d.path();
        let Some(mtime) = mtime_ms(&path) else { continue };
        if now_ms - mtime > LIST_WINDOW_MS {
            continue;
        }
        let meta = std::fs::read_to_string(dir.join(format!("agent-{id}.meta.json")))
            .ok()
            .and_then(|t| parse_meta(&t).ok())
            .unwrap_or_default();
        let Ok((text, truncated)) = read_tail(&path, TAIL_BYTES) else { continue };
        let tail = parse_agent_tail(&text, truncated);
        let started_ms = read_head(&path, 16 * 1024).ok().and_then(|h| first_timestamp_ms(&h));
        let state = classify(&tail, mtime, now_ms);
        out.push(Subagent {
            agent_id: id.to_string(),
            agent_type: meta.agent_type,
            description: meta.description,
            depth: meta.spawn_depth,
            state,
            started_ms,
            ended_ms: if matches!(state, AgentState::Done | AgentState::Failed) { tail.last_ts_ms } else { None },
            context_tokens: tail.context_tokens,
        });
    }
    out.sort_by(|a, b| {
        (a.state != AgentState::Running)
            .cmp(&(b.state != AgentState::Running))
            .then(b.started_ms.cmp(&a.started_ms))
    });
    out
}
```

- [x] **Step 4: Run unit tests**

Run: `cargo test --lib collectors::subagents`
Expected: 5 passed.

- [x] **Step 5: Integration test with real files**

`tests/subagents_io.rs`:

```rust
mod common;

use claudehud::collectors::subagents::list_subagents;
use claudehud::model::AgentState;
use claudehud::timefmt::now_ms;
use std::time::{Duration, SystemTime};

fn assistant(ts: &str, stop: &str) -> String {
    format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"stop_reason":"{stop}","content":[],"usage":{{"input_tokens":1,"cache_read_input_tokens":9,"cache_creation_input_tokens":0}}}}}}"#)
}

#[test]
fn lists_recent_agents_running_first_and_skips_old_ones() {
    let t = common::TempDir::new("subagents");
    let sd = t.path().join("sess");
    t.write("sess/subagents/agent-aaa.meta.json", r#"{"agentType":"Explore","description":"Map endpoints","spawnDepth":1}"#);
    t.write("sess/subagents/agent-aaa.jsonl", &assistant("2026-09-25T10:00:00Z", "tool_use"));
    t.write("sess/subagents/agent-bbb.meta.json", r#"{"agentType":"general-purpose","description":"Write tests","spawnDepth":2}"#);
    t.write(
        "sess/subagents/agent-bbb.jsonl",
        &format!("{}\n{}", assistant("2026-09-25T10:01:00Z", "tool_use"), assistant("2026-09-25T10:02:00Z", "end_turn")),
    );
    let old = t.write("sess/subagents/agent-ccc.jsonl", &assistant("2026-09-25T08:00:00Z", "tool_use"));
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    t.write("sess/subagents/notes.txt", "ignored");

    let list = list_subagents(&sd, now_ms());
    let ids: Vec<&str> = list.iter().map(|a| a.agent_id.as_str()).collect();
    assert_eq!(ids, vec!["aaa", "bbb"], "running first; ccc is older than 15 min");
    assert_eq!(list[0].state, AgentState::Running);
    assert_eq!(list[0].agent_type, "Explore");
    assert_eq!(list[1].state, AgentState::Done);
    assert_eq!(list[1].depth, 2);
    assert_eq!(list[1].context_tokens, Some(10));
    assert!(list[1].ended_ms.is_some());
}

#[test]
fn missing_folder_means_no_agents() {
    let t = common::TempDir::new("subagents-none");
    assert!(list_subagents(&t.path().join("nope"), now_ms()).is_empty());
}
```

- [x] **Step 6: Run it**

Run: `cargo test --test subagents_io`
Expected: 2 passed.

- [x] **Step 7: Lint and commit**

```powershell
cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/collectors tests/subagents_io.rs
git commit -m "feat(subagents): list recent sub-agents with state and tokens"
```
