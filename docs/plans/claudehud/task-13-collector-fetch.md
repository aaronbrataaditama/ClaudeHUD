# Task 13: Collector, HTTP fetch logic, poll schedule

**Goal:** One `Collector` that the UI thread ticks every second (registry scan → liveness → torn-file reuse → crash latch → cached transcript tails → sub-agents) and that absorbs results from the worker thread (usage, status). It produces the `Snapshot` everything else consumes. Also the platform-free HTTP logic (`fetch_usage`, `fetch_status`) and the poll `Schedule` with rate-limit backoff, all tested with fake HTTP and fake process probes.

**Spec:** §3.1 (liveness, torn files are a Review Focus item), §3.2 (read tails only when changed), §3.3 (sub-agents only while the panel is visible or the transcript changed), §3.5 (re-read credentials every poll, 300 s, rate limit = backoff and no colour, token-expired copy), §3.6, §7.

**Files:**
- Create: `src/collect.rs`
- Create: `tests/collect.rs`
- Modify: `src/lib.rs` (add `pub mod collect;`)

**Interfaces:**
- Consumes: `collectors::{registry::{scan_dir, liveness, Liveness, ProcessProbe, RegistryEntry, RegistryScan}, transcript::{find_transcript, parse_tail}, tail::{read_tail, TAIL_BYTES}, subagents::list_subagents, credentials::parse_credentials, usage::parse_usage, status::parse_status}`, `latch::CrashLatch`, `model::*`.
- Produces (`collect::…`):
  - consts `USAGE_HOST = "api.anthropic.com"`, `USAGE_PATH = "/api/oauth/usage"`, `STATUS_HOST = "status.claude.com"`, `STATUS_PATH = "/api/v2/summary.json"`, `OAUTH_BETA = "oauth-2025-04-20"`, `USER_AGENT = "ClaudeHUD/0.1 (Claude Code status light)"`, `TORN_REUSE_TICKS: u32 = 5`
  - `HttpResponse { status: u16, body: String, retry_after_s: Option<u64> }`
  - `trait HttpGet { fn get(&self, host: &str, path: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, String>; }` (HTTPS GET; implemented with WinHTTP in Task 15)
  - `UsageError { NoCredentials, TokenExpired, RateLimited { retry_after_s: Option<u64> }, Http(u16), Network(String), Parse(String) }` with `health_text() -> String`
  - `UsageOutcome { plan: Option<String>, result: Result<Usage, UsageError> }`
  - `fetch_usage(claude_dir: &Path, http: &dyn HttpGet, now_ms: i64) -> UsageOutcome`
  - `fetch_status(http: &dyn HttpGet, now_ms: i64) -> Result<ServiceStatus, String>`
  - `Schedule` with `new(now_ms)`, `usage_due(now_ms) -> bool`, `status_due(now_ms) -> bool`, `after_usage(now_ms, &Result<Usage,UsageError>, base_s: u64)`, `after_status(now_ms, ok: bool, base_s: u64)`, `refresh_now(now_ms)`, `next_wake_ms() -> i64`, `usage_backoff_s() -> u64`
  - `Collector` with `new(claude_dir: PathBuf)`, `tick(&mut self, now_ms, &dyn ProcessProbe, want_subagents: bool)`, `apply_usage(&mut self, UsageOutcome, now_ms)`, `apply_status(&mut self, Result<ServiceStatus,String>)`, `acknowledge(&mut self)`, `usage_age_ms(&self, now_ms) -> Option<i64>`, `snapshot(&self, now_ms, warn_percent: u8) -> Snapshot`

---

- [ ] **Step 1: Write the failing integration tests**

`tests/collect.rs`:

```rust
mod common;

use claudehud::collect::*;
use claudehud::collectors::registry::ProcessProbe;
use claudehud::model::*;
use std::cell::RefCell;
use std::collections::HashMap;

// ---------- fakes ----------

#[derive(Default)]
struct Probe(RefCell<HashMap<u32, u64>>);
impl Probe {
    fn set(&self, pid: u32, start: Option<u64>) {
        match start {
            Some(s) => self.0.borrow_mut().insert(pid, s),
            None => self.0.borrow_mut().remove(&pid),
        };
    }
}
impl ProcessProbe for Probe {
    fn creation_filetime(&self, pid: u32) -> Option<u64> {
        self.0.borrow().get(&pid).copied()
    }
    fn pid_domain(&self) -> String {
        "win32:test-pc".into()
    }
}

#[derive(Default)]
struct Http {
    responses: RefCell<Vec<Result<HttpResponse, String>>>,
    calls: RefCell<Vec<(String, String, Vec<(String, String)>)>>,
}
impl Http {
    fn push(&self, status: u16, body: &str, retry_after_s: Option<u64>) {
        self.responses.borrow_mut().push(Ok(HttpResponse { status, body: body.into(), retry_after_s }));
    }
}
impl HttpGet for Http {
    fn get(&self, host: &str, path: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, String> {
        self.calls.borrow_mut().push((host.into(), path.into(), headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()));
        let mut r = self.responses.borrow_mut();
        if r.is_empty() {
            Err("no response queued".into())
        } else {
            r.remove(0)
        }
    }
}

const NOW: i64 = 1_790_330_000_000;
const USAGE_OK: &str = r#"{"five_hour":{"utilization":61,"resets_at":"2026-09-25T14:32:00Z"},"seven_day":{"utilization":88,"resets_at":"2026-09-28T09:00:00Z"}}"#;

fn registry(pid: u32, status: &str) -> String {
    format!(r#"{{"pid":{pid},"sessionId":"sess-{pid}","cwd":"C:\\work\\app{pid}","procStart":"{}","pidDomain":"win32:test-pc","name":"app{pid}","status":"{status}","startedAt":{NOW},"statusUpdatedAt":{NOW}}}"#, 5000 + pid as u64)
}
fn creds(expires_ms: i64) -> String {
    format!(r#"{{"claudeAiOauth":{{"accessToken":"tok-123","expiresAt":{expires_ms},"subscriptionType":"team","rateLimitTier":"default_claude_max_5x"}}}}"#)
}

// ---------- collector ----------

#[test]
fn live_session_gets_transcript_and_subagents() {
    let t = common::TempDir::new("collect-live");
    t.write("sessions/7.json", &registry(7, "busy"));
    t.write("projects/C--work-app7/sess-7.jsonl", r#"{"type":"assistant","message":{"model":"claude-opus-5","content":[],"usage":{"input_tokens":1,"cache_read_input_tokens":999,"cache_creation_input_tokens":0}}}"#);
    t.write("projects/C--work-app7/sess-7/subagents/agent-a1.meta.json", r#"{"agentType":"Explore","description":"Look around"}"#);
    t.write("projects/C--work-app7/sess-7/subagents/agent-a1.jsonl", r#"{"type":"assistant","timestamp":"2026-09-25T10:00:00Z","message":{"stop_reason":"tool_use","content":[]}}"#);
    let probe = Probe::default();
    probe.set(7, Some(5007));
    let mut c = Collector::new(t.path().to_path_buf());
    c.tick(claudehud::timefmt::now_ms(), &probe, true);
    let s = c.snapshot(NOW, 85);
    assert_eq!(s.registry, Health::Healthy);
    assert_eq!(s.sessions.len(), 1);
    let sess = &s.sessions[0];
    assert_eq!((sess.name.as_str(), sess.status), ("app7", SessionStatus::Busy));
    assert_eq!(sess.transcript.as_ref().unwrap().last_turn_input_tokens, Some(1000));
    assert_eq!(sess.subagents.len(), 1);
    assert_eq!(sess.subagents[0].agent_type, "Explore");
}

#[test]
fn torn_registry_file_reuses_last_good_entry() {
    let t = common::TempDir::new("collect-torn");
    t.write("sessions/7.json", &registry(7, "busy"));
    let probe = Probe::default();
    probe.set(7, Some(5007));
    let mut c = Collector::new(t.path().to_path_buf());
    c.tick(NOW, &probe, false);
    assert_eq!(c.snapshot(NOW, 85).sessions.len(), 1);
    t.write("sessions/7.json", r#"{"pid":7,"sessionId":"se"#); // caught mid-rewrite
    for i in 0..TORN_REUSE_TICKS {
        c.tick(NOW, &probe, false);
        assert_eq!(c.snapshot(NOW, 85).sessions.len(), 1, "tick {i}: reuse last good parse");
    }
    c.tick(NOW, &probe, false);
    assert_eq!(c.snapshot(NOW, 85).sessions.len(), 0, "gives up after {TORN_REUSE_TICKS} ticks");
    t.write("sessions/7.json", &registry(7, "idle"));
    c.tick(NOW, &probe, false);
    assert_eq!(c.snapshot(NOW, 85).sessions[0].status, SessionStatus::Idle);
}

#[test]
fn crash_mid_turn_latches_until_acknowledged() {
    let t = common::TempDir::new("collect-crash");
    t.write("sessions/7.json", &registry(7, "busy"));
    let probe = Probe::default();
    probe.set(7, Some(5007));
    let mut c = Collector::new(t.path().to_path_buf());
    c.tick(NOW, &probe, false);
    probe.set(7, None); // killed; the registry file stays behind
    c.tick(NOW, &probe, false);
    let s = c.snapshot(NOW, 85);
    assert!(s.sessions.is_empty());
    assert_eq!(s.crashed.len(), 1);
    c.acknowledge();
    c.tick(NOW, &probe, false);
    assert!(c.snapshot(NOW, 85).crashed.is_empty());
}

#[test]
fn stale_file_at_startup_is_silent() {
    let t = common::TempDir::new("collect-stale");
    t.write("sessions/7.json", &registry(7, "busy"));
    let mut c = Collector::new(t.path().to_path_buf());
    c.tick(NOW, &Probe::default(), false);
    let s = c.snapshot(NOW, 85);
    assert!(s.sessions.is_empty() && s.crashed.is_empty());
}

#[test]
fn registry_health() {
    let t = common::TempDir::new("collect-health");
    let mut c = Collector::new(t.path().to_path_buf());
    c.tick(NOW, &Probe::default(), false);
    assert_eq!(c.snapshot(NOW, 85).registry, Health::Healthy, "no sessions dir yet is fine");
    t.write("sessions", "i am a file, not a directory");
    c.tick(NOW, &Probe::default(), false);
    assert!(matches!(c.snapshot(NOW, 85).registry, Health::Unknown(_)));
}

// ---------- fetch_usage ----------

#[test]
fn usage_request_carries_token_and_beta_header() {
    let t = common::TempDir::new("usage-ok");
    t.write(".credentials.json", &creds(NOW + 3_600_000));
    let http = Http::default();
    http.push(200, USAGE_OK, None);
    let o = fetch_usage(t.path(), &http, NOW);
    assert_eq!(o.plan.as_deref(), Some("Team · Max 5x"));
    assert_eq!(o.result.as_ref().unwrap().limits.len(), 2);
    let calls = http.calls.borrow();
    let (host, path, headers) = &calls[0];
    assert_eq!((host.as_str(), path.as_str()), (USAGE_HOST, USAGE_PATH));
    assert!(headers.contains(&("Authorization".into(), "Bearer tok-123".into())));
    assert!(headers.contains(&("anthropic-beta".into(), OAUTH_BETA.into())));
}

#[test]
fn locally_expired_or_missing_token_makes_no_request() {
    let t = common::TempDir::new("usage-expired");
    let http = Http::default();
    assert_eq!(fetch_usage(t.path(), &http, NOW).result.unwrap_err(), UsageError::NoCredentials);
    t.write(".credentials.json", &creds(NOW - 1));
    assert_eq!(fetch_usage(t.path(), &http, NOW).result.unwrap_err(), UsageError::TokenExpired);
    assert!(http.calls.borrow().is_empty());
}

#[test]
fn http_401_maps_to_token_expired_and_keeps_stale_value() {
    let t = common::TempDir::new("usage-401");
    t.write(".credentials.json", &creds(NOW + 3_600_000));
    let http = Http::default();
    http.push(200, USAGE_OK, None);
    http.push(401, "unauthorized", None);
    let mut c = Collector::new(t.path().to_path_buf());
    c.apply_usage(fetch_usage(t.path(), &http, NOW), NOW);
    c.apply_usage(fetch_usage(t.path(), &http, NOW), NOW + 1);
    let s = c.snapshot(NOW, 85);
    assert_eq!(s.usage.health, Health::Degraded("Token expired — open Claude Code to refresh".into()));
    assert_eq!(s.usage.value.as_ref().unwrap().limits.len(), 2, "stale values kept for display");
    assert_eq!(s.plan.as_deref(), Some("Team · Max 5x"));
    assert_eq!(c.usage_age_ms(NOW + 10_000), Some(10_000), "age counts from the last success");
}

#[test]
fn status_codes_and_bad_bodies() {
    let t = common::TempDir::new("usage-codes");
    t.write(".credentials.json", &creds(NOW + 3_600_000));
    let http = Http::default();
    http.push(429, "", Some(900));
    http.push(500, "", None);
    http.push(200, "<html>", None);
    assert_eq!(fetch_usage(t.path(), &http, NOW).result.unwrap_err(), UsageError::RateLimited { retry_after_s: Some(900) });
    assert_eq!(fetch_usage(t.path(), &http, NOW).result.unwrap_err(), UsageError::Http(500));
    assert!(matches!(fetch_usage(t.path(), &http, NOW).result.unwrap_err(), UsageError::Parse(_)));
    assert!(matches!(fetch_usage(t.path(), &http, NOW).result.unwrap_err(), UsageError::Network(_)));
    assert_eq!(UsageError::RateLimited { retry_after_s: None }.health_text(), "usage unavailable · rate limited");
}

// ---------- fetch_status ----------

#[test]
fn status_fetch() {
    let http = Http::default();
    http.push(200, r#"{"status":{"description":"All Systems Operational"},"components":[{"name":"Claude Code","status":"operational"}]}"#, None);
    http.push(503, "", None);
    let s = fetch_status(&http, NOW).unwrap();
    assert_eq!(s.components.len(), 1);
    assert_eq!(s.checked_at_ms, NOW);
    assert!(fetch_status(&http, NOW).is_err());
    assert_eq!(http.calls.borrow()[0].0, STATUS_HOST);
}

// ---------- schedule ----------

#[test]
fn schedule_backs_off_on_rate_limit_and_resets_on_success() {
    let mut s = Schedule::new(NOW);
    assert!(s.usage_due(NOW) && s.status_due(NOW));
    let rl: Result<Usage, UsageError> = Err(UsageError::RateLimited { retry_after_s: None });
    s.after_usage(NOW, &rl, 300);
    assert_eq!(s.usage_backoff_s(), 600);
    assert!(!s.usage_due(NOW + 599_000) && s.usage_due(NOW + 600_000));
    s.after_usage(NOW, &rl, 300);
    assert_eq!(s.usage_backoff_s(), 1200);
    s.refresh_now(NOW);
    assert!(!s.usage_due(NOW), "refresh_now never cuts a rate-limit backoff short");
    let hinted: Result<Usage, UsageError> = Err(UsageError::RateLimited { retry_after_s: Some(3000) });
    s.after_usage(NOW, &hinted, 300);
    assert!(!s.usage_due(NOW + 2_999_000), "Retry-After longer than backoff wins");
    s.after_usage(NOW, &Ok(Usage::default()), 300);
    assert_eq!(s.usage_backoff_s(), 0);
    assert!(s.usage_due(NOW + 300_000) && !s.usage_due(NOW + 299_000));
    s.refresh_now(NOW);
    assert!(s.usage_due(NOW) && s.status_due(NOW));
    s.after_status(NOW, true, 300);
    assert_eq!(s.next_wake_ms(), NOW, "usage still due now");
}

#[test]
fn schedule_does_not_storm_on_expiry_or_network_errors() {
    let mut s = Schedule::new(NOW);
    s.after_usage(NOW, &Err(UsageError::TokenExpired), 300);
    assert!(!s.usage_due(NOW + 299_000));
    s.after_usage(NOW, &Err(UsageError::Network("offline".into())), 300);
    assert!(!s.usage_due(NOW + 59_000) && s.usage_due(NOW + 60_000));
}
```

Add `pub mod collect;` to `src/lib.rs` and create an empty `src/collect.rs`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test collect`
Expected: compile errors (nothing defined in `collect`).

- [ ] **Step 3: Implement `src/collect.rs`**

```rust
//! Glue between the collectors and the UI. The UI thread owns one `Collector`
//! and ticks it every second; the worker thread runs `fetch_usage` /
//! `fetch_status` on the `Schedule` and hands the results back. No Win32 here:
//! the process probe and HTTP client come in as traits.

use crate::collectors::credentials::parse_credentials;
use crate::collectors::registry::{liveness, scan_dir, Liveness, ProcessProbe, RegistryEntry, RegistryScan};
use crate::collectors::status::parse_status;
use crate::collectors::subagents::list_subagents;
use crate::collectors::tail::{read_tail, TAIL_BYTES};
use crate::collectors::transcript::{find_transcript, parse_tail};
use crate::collectors::usage::parse_usage;
use crate::latch::CrashLatch;
use crate::model::{Collected, Health, ServiceStatus, Session, Snapshot, Subagent, TranscriptFacts, Usage};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const USAGE_HOST: &str = "api.anthropic.com";
pub const USAGE_PATH: &str = "/api/oauth/usage";
pub const STATUS_HOST: &str = "status.claude.com";
pub const STATUS_PATH: &str = "/api/v2/summary.json";
pub const OAUTH_BETA: &str = "oauth-2025-04-20";
pub const USER_AGENT: &str = "ClaudeHUD/0.1 (Claude Code status light)";
/// How many ticks a registry file that fails to parse keeps its last good value.
pub const TORN_REUSE_TICKS: u32 = 5;

// ---------------------------------------------------------------- HTTP

pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub retry_after_s: Option<u64>,
}

/// HTTPS GET. Header values may contain the OAuth token: implementations must
/// never log them.
pub trait HttpGet {
    fn get(&self, host: &str, path: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, String>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum UsageError {
    NoCredentials,
    TokenExpired,
    RateLimited { retry_after_s: Option<u64> },
    Http(u16),
    Network(String),
    Parse(String),
}

impl UsageError {
    /// Copy for the panel footer and tooltip (§3.5).
    pub fn health_text(&self) -> String {
        match self {
            UsageError::NoCredentials => "Not signed in to Claude Code".to_string(),
            UsageError::TokenExpired => "Token expired — open Claude Code to refresh".to_string(),
            UsageError::RateLimited { .. } => "usage unavailable · rate limited".to_string(),
            UsageError::Http(code) => format!("usage unavailable · HTTP {code}"),
            UsageError::Network(_) => "usage unavailable · offline".to_string(),
            UsageError::Parse(_) => "usage unavailable · unexpected response".to_string(),
        }
    }
}

pub struct UsageOutcome {
    pub plan: Option<String>,
    pub result: Result<Usage, UsageError>,
}

/// Re-reads `.credentials.json` every call (Claude Code refreshes it).
pub fn fetch_usage(claude_dir: &Path, http: &dyn HttpGet, now_ms: i64) -> UsageOutcome {
    let fail = |plan, e| UsageOutcome { plan, result: Err(e) };
    let Ok(text) = std::fs::read_to_string(claude_dir.join(".credentials.json")) else {
        return fail(None, UsageError::NoCredentials);
    };
    let creds = match parse_credentials(&text) {
        Ok(c) => c,
        Err(e) => return fail(None, UsageError::Parse(e)),
    };
    let plan = creds.plan_label();
    let Some(token) = creds.token.as_ref() else { return fail(plan, UsageError::NoCredentials) };
    if creds.is_expired(now_ms) {
        return fail(plan, UsageError::TokenExpired);
    }
    let auth = format!("Bearer {}", token.expose());
    let headers = [("Authorization", auth.as_str()), ("anthropic-beta", OAUTH_BETA), ("Accept", "application/json")];
    let result = match http.get(USAGE_HOST, USAGE_PATH, &headers) {
        Err(e) => Err(UsageError::Network(e)),
        Ok(r) if r.status == 401 || r.status == 403 => Err(UsageError::TokenExpired),
        Ok(r) if r.status == 429 => Err(UsageError::RateLimited { retry_after_s: r.retry_after_s }),
        Ok(r) if !(200..300).contains(&r.status) => Err(UsageError::Http(r.status)),
        Ok(r) => parse_usage(&r.body).map_err(UsageError::Parse),
    };
    UsageOutcome { plan, result }
}

pub fn fetch_status(http: &dyn HttpGet, now_ms: i64) -> Result<ServiceStatus, String> {
    match http.get(STATUS_HOST, STATUS_PATH, &[("Accept", "application/json")]) {
        Err(_) => Err("status page unreachable".to_string()),
        Ok(r) if r.status != 200 => Err(format!("status page HTTP {}", r.status)),
        Ok(r) => parse_status(&r.body, now_ms),
    }
}

// ---------------------------------------------------------------- schedule

const NETWORK_RETRY_S: u64 = 60;
const BACKOFF_MIN_S: u64 = 600;
const BACKOFF_MAX_S: u64 = 3_600;

#[derive(Clone, Debug, PartialEq)]
pub struct Schedule {
    usage_due_ms: i64,
    status_due_ms: i64,
    usage_backoff_s: u64,
}

impl Schedule {
    pub fn new(now_ms: i64) -> Schedule {
        Schedule { usage_due_ms: now_ms, status_due_ms: now_ms, usage_backoff_s: 0 }
    }
    pub fn usage_due(&self, now_ms: i64) -> bool {
        now_ms >= self.usage_due_ms
    }
    pub fn status_due(&self, now_ms: i64) -> bool {
        now_ms >= self.status_due_ms
    }
    pub fn usage_backoff_s(&self) -> u64 {
        self.usage_backoff_s
    }
    pub fn next_wake_ms(&self) -> i64 {
        self.usage_due_ms.min(self.status_due_ms)
    }

    pub fn after_usage(&mut self, now_ms: i64, result: &Result<Usage, UsageError>, base_s: u64) {
        let delay_s = match result {
            Err(UsageError::RateLimited { retry_after_s }) => {
                self.usage_backoff_s = (self.usage_backoff_s * 2).clamp(BACKOFF_MIN_S, BACKOFF_MAX_S);
                retry_after_s.map_or(self.usage_backoff_s, |r| r.max(self.usage_backoff_s))
            }
            Err(UsageError::Network(_)) => NETWORK_RETRY_S,
            Err(_) => base_s,
            Ok(_) => {
                self.usage_backoff_s = 0;
                base_s
            }
        };
        self.usage_due_ms = now_ms + delay_s as i64 * 1000;
    }

    pub fn after_status(&mut self, now_ms: i64, ok: bool, base_s: u64) {
        let delay_s = if ok { base_s } else { base_s.min(NETWORK_RETRY_S * 2) };
        self.status_due_ms = now_ms + delay_s as i64 * 1000;
    }

    /// Resume from sleep, unlock, panel opened with old data. Never shortens a
    /// rate-limit backoff: that would make the rate limiting worse.
    pub fn refresh_now(&mut self, now_ms: i64) {
        if self.usage_backoff_s == 0 {
            self.usage_due_ms = self.usage_due_ms.min(now_ms);
        }
        self.status_due_ms = self.status_due_ms.min(now_ms);
    }
}

// ---------------------------------------------------------------- collector

struct CachedTranscript {
    path: PathBuf,
    len: u64,
    mtime_ms: i64,
    facts: TranscriptFacts,
}

pub struct Collector {
    claude_dir: PathBuf,
    latch: CrashLatch,
    /// file name → (last good parse, consecutive unreadable ticks)
    last_good: HashMap<String, (RegistryEntry, u32)>,
    transcripts: HashMap<String, CachedTranscript>,
    subagents: HashMap<String, Vec<Subagent>>,
    sessions: Vec<Session>,
    registry: Health,
    usage: Collected<Usage>,
    usage_ok_ms: Option<i64>,
    status: Collected<ServiceStatus>,
    plan: Option<String>,
}

fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Collector {
    pub fn new(claude_dir: PathBuf) -> Collector {
        Collector {
            claude_dir,
            latch: CrashLatch::new(),
            last_good: HashMap::new(),
            transcripts: HashMap::new(),
            subagents: HashMap::new(),
            sessions: Vec::new(),
            registry: Health::Healthy,
            usage: Collected::default(),
            usage_ok_ms: None,
            status: Collected::default(),
            plan: None,
        }
    }

    pub fn tick(&mut self, now_ms: i64, probe: &dyn ProcessProbe, want_subagents: bool) {
        let scan = match scan_dir(&self.claude_dir.join("sessions"), probe) {
            Ok(s) => s,
            Err(e) => {
                self.registry = Health::Unknown(e);
                return;
            }
        };
        self.registry = Health::Healthy;
        let scan = self.patch_torn(scan, probe);
        self.latch.update(&scan.live, &scan.dead);

        let projects = self.claude_dir.join("projects");
        let mut sessions = Vec::with_capacity(scan.live.len());
        for e in &scan.live {
            let changed = self.refresh_transcript(&projects, e);
            if want_subagents || changed {
                if let Some(c) = self.transcripts.get(&e.session_id) {
                    let dir = c.path.with_extension("");
                    self.subagents.insert(e.session_id.clone(), list_subagents(&dir, now_ms));
                }
            }
            let mut s = e.to_session();
            s.transcript = self.transcripts.get(&e.session_id).map(|c| c.facts.clone());
            s.subagents = self.subagents.get(&e.session_id).cloned().unwrap_or_default();
            sessions.push(s);
        }
        let live: HashSet<String> = scan.live.iter().map(|e| e.session_id.clone()).collect();
        self.transcripts.retain(|k, _| live.contains(k));
        self.subagents.retain(|k, _| live.contains(k));
        self.sessions = sessions;
    }

    /// Unreadable files reuse their last good parse for up to TORN_REUSE_TICKS ticks.
    fn patch_torn(&mut self, mut scan: RegistryScan, probe: &dyn ProcessProbe) -> RegistryScan {
        for e in scan.live.iter().chain(scan.dead.iter()) {
            self.last_good.insert(e.file_name.clone(), (e.clone(), 0));
        }
        let unreadable = std::mem::take(&mut scan.unreadable);
        for name in unreadable {
            let reuse = match self.last_good.get_mut(&name) {
                Some((e, streak)) if *streak < TORN_REUSE_TICKS => {
                    *streak += 1;
                    Some(e.clone())
                }
                _ => None,
            };
            match reuse {
                Some(e) => match liveness(&e, probe) {
                    Liveness::Alive => scan.live.push(e),
                    Liveness::Dead => scan.dead.push(e),
                    Liveness::Foreign => {}
                },
                None => scan.unreadable.push(name),
            }
        }
        let present: HashSet<String> = scan
            .live
            .iter()
            .chain(scan.dead.iter())
            .map(|e| e.file_name.clone())
            .chain(scan.unreadable.iter().cloned())
            .collect();
        self.last_good.retain(|k, _| present.contains(k));
        scan
    }

    /// Re-reads the transcript tail only if its size or mtime changed. Returns true if it did.
    fn refresh_transcript(&mut self, projects: &Path, e: &RegistryEntry) -> bool {
        let path = match self.transcripts.get(&e.session_id) {
            Some(c) if c.path.is_file() => c.path.clone(),
            _ => match find_transcript(projects, &e.cwd, &e.session_id) {
                Some(p) => p,
                None => return false,
            },
        };
        let Ok(meta) = std::fs::metadata(&path) else { return false };
        let (len, mtime) = (meta.len(), mtime_ms(&meta));
        if let Some(c) = self.transcripts.get(&e.session_id) {
            if c.path == path && c.len == len && c.mtime_ms == mtime {
                return false;
            }
        }
        let Ok((text, truncated)) = read_tail(&path, TAIL_BYTES) else { return false };
        let facts = parse_tail(&text, truncated);
        self.transcripts.insert(e.session_id.clone(), CachedTranscript { path, len, mtime_ms: mtime, facts });
        true
    }

    pub fn apply_usage(&mut self, o: UsageOutcome, now_ms: i64) {
        if o.plan.is_some() {
            self.plan = o.plan;
        }
        match o.result {
            Ok(u) => {
                self.usage = Collected::healthy(u);
                self.usage_ok_ms = Some(now_ms);
            }
            Err(e) => self.usage.health = Health::Degraded(e.health_text()),
        }
    }

    pub fn apply_status(&mut self, r: Result<ServiceStatus, String>) {
        match r {
            Ok(s) => self.status = Collected::healthy(s),
            Err(e) => self.status.health = Health::Degraded(e),
        }
    }

    pub fn acknowledge(&mut self) {
        self.latch.acknowledge();
    }

    pub fn usage_age_ms(&self, now_ms: i64) -> Option<i64> {
        self.usage_ok_ms.map(|t| now_ms - t)
    }

    pub fn snapshot(&self, now_ms: i64, warn_percent: u8) -> Snapshot {
        Snapshot {
            now_ms,
            registry: self.registry.clone(),
            sessions: self.sessions.clone(),
            crashed: self.latch.crashed().to_vec(),
            plan: self.plan.clone(),
            usage: self.usage.clone(),
            status: self.status.clone(),
            warn_percent,
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test collect`
Expected: 12 passed.

Two tests are easy to get subtly wrong:
- `crash_mid_turn_latches_until_acknowledged`: the dead file is still on disk, so every later tick reports it as `dead`. The latch must not re-latch it after `acknowledge()` (Task 9 removes it from `seen_alive`).
- `torn_registry_file_reuses_last_good_entry`: the streak resets when the file parses again.

- [ ] **Step 5: Full suite, lint, commit**

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/lib.rs src/collect.rs tests/collect.rs
git commit -m "feat(collect): collector tick, usage/status fetch, poll schedule with backoff"
```
