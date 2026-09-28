//! Glue between the collectors and the UI. The UI thread owns one `Collector`
//! and ticks it every second; the worker thread runs `fetch_usage` /
//! `fetch_status` on the `Schedule` and hands the results back. No Win32 here:
//! the process probe and HTTP client come in as traits.

use crate::collectors::credentials::parse_credentials;
use crate::collectors::registry::{
    liveness, scan_dir, Liveness, ProcessProbe, RegistryEntry, RegistryScan,
};
use crate::collectors::status::parse_status;
use crate::collectors::subagents::list_subagents;
use crate::collectors::tail::{read_tail, TAIL_BYTES};
use crate::collectors::transcript::{find_transcript, parse_tail};
use crate::collectors::usage::parse_usage;
use crate::latch::CrashLatch;
use crate::model::{
    Collected, Health, ServiceStatus, Session, Snapshot, Subagent, TranscriptFacts, Usage,
};
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
    fn get(&self, host: &str, path: &str, headers: &[(&str, &str)])
        -> Result<HttpResponse, String>;
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
    let fail = |plan, e| UsageOutcome {
        plan,
        result: Err(e),
    };
    let Ok(text) = std::fs::read_to_string(claude_dir.join(".credentials.json")) else {
        return fail(None, UsageError::NoCredentials);
    };
    let creds = match parse_credentials(&text) {
        Ok(c) => c,
        Err(e) => return fail(None, UsageError::Parse(e)),
    };
    let plan = creds.plan_label();
    let Some(token) = creds.token.as_ref() else {
        return fail(plan, UsageError::NoCredentials);
    };
    if creds.is_expired(now_ms) {
        return fail(plan, UsageError::TokenExpired);
    }
    let auth = format!("Bearer {}", token.expose());
    let headers = [
        ("Authorization", auth.as_str()),
        ("anthropic-beta", OAUTH_BETA),
        ("Accept", "application/json"),
    ];
    let result = match http.get(USAGE_HOST, USAGE_PATH, &headers) {
        Err(e) => Err(UsageError::Network(e)),
        Ok(r) if r.status == 401 || r.status == 403 => Err(UsageError::TokenExpired),
        Ok(r) if r.status == 429 => Err(UsageError::RateLimited {
            retry_after_s: r.retry_after_s,
        }),
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
        Schedule {
            usage_due_ms: now_ms,
            status_due_ms: now_ms,
            usage_backoff_s: 0,
        }
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
                self.usage_backoff_s =
                    (self.usage_backoff_s * 2).clamp(BACKOFF_MIN_S, BACKOFF_MAX_S);
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
        let delay_s = if ok {
            base_s
        } else {
            base_s.min(NETWORK_RETRY_S * 2)
        };
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
                    self.subagents
                        .insert(e.session_id.clone(), list_subagents(&dir, now_ms));
                }
            }
            let mut s = e.to_session();
            s.transcript = self.transcripts.get(&e.session_id).map(|c| c.facts.clone());
            s.subagents = self
                .subagents
                .get(&e.session_id)
                .cloned()
                .unwrap_or_default();
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
        let Ok(meta) = std::fs::metadata(&path) else {
            return false;
        };
        let (len, mtime) = (meta.len(), mtime_ms(&meta));
        if let Some(c) = self.transcripts.get(&e.session_id) {
            if c.path == path && c.len == len && c.mtime_ms == mtime {
                return false;
            }
        }
        let Ok((text, truncated)) = read_tail(&path, TAIL_BYTES) else {
            return false;
        };
        let facts = parse_tail(&text, truncated);
        self.transcripts.insert(
            e.session_id.clone(),
            CachedTranscript {
                path,
                len,
                mtime_ms: mtime,
                facts,
            },
        );
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
