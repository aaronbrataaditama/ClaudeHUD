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
    #[allow(clippy::type_complexity)]
    calls: RefCell<Vec<(String, String, Vec<(String, String)>)>>,
}
impl Http {
    fn push(&self, status: u16, body: &str, retry_after_s: Option<u64>) {
        self.responses.borrow_mut().push(Ok(HttpResponse {
            status,
            body: body.into(),
            retry_after_s,
        }));
    }
}
impl HttpGet for Http {
    fn get(
        &self,
        host: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, String> {
        self.calls.borrow_mut().push((
            host.into(),
            path.into(),
            headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ));
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
    format!(
        r#"{{"pid":{pid},"sessionId":"sess-{pid}","cwd":"C:\\work\\app{pid}","procStart":"{}","pidDomain":"win32:test-pc","name":"app{pid}","status":"{status}","startedAt":{NOW},"statusUpdatedAt":{NOW}}}"#,
        5000 + pid as u64
    )
}
fn creds(expires_ms: i64) -> String {
    format!(
        r#"{{"claudeAiOauth":{{"accessToken":"tok-123","expiresAt":{expires_ms},"subscriptionType":"team","rateLimitTier":"default_claude_max_5x"}}}}"#
    )
}

// ---------- collector ----------

#[test]
fn live_session_gets_transcript_and_subagents() {
    let t = common::TempDir::new("collect-live");
    t.write("sessions/7.json", &registry(7, "busy"));
    t.write("projects/C--work-app7/sess-7.jsonl", r#"{"type":"assistant","message":{"model":"claude-opus-5","content":[],"usage":{"input_tokens":1,"cache_read_input_tokens":999,"cache_creation_input_tokens":0}}}"#);
    t.write(
        "projects/C--work-app7/sess-7/subagents/agent-a1.meta.json",
        r#"{"agentType":"Explore","description":"Look around"}"#,
    );
    t.write("projects/C--work-app7/sess-7/subagents/agent-a1.jsonl", r#"{"type":"assistant","timestamp":"2026-09-25T10:00:00Z","message":{"stop_reason":"tool_use","content":[]}}"#);
    let probe = Probe::default();
    probe.set(7, Some(5007));
    let mut c = Collector::new(t.path().to_path_buf());
    c.tick(claudehud::timefmt::now_ms(), &probe, true);
    let s = c.snapshot(NOW, 85);
    assert_eq!(s.registry, Health::Healthy);
    assert_eq!(s.sessions.len(), 1);
    let sess = &s.sessions[0];
    assert_eq!(
        (sess.name.as_str(), sess.status),
        ("app7", SessionStatus::Busy)
    );
    assert_eq!(
        sess.transcript.as_ref().unwrap().last_turn_input_tokens,
        Some(1000)
    );
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
        assert_eq!(
            c.snapshot(NOW, 85).sessions.len(),
            1,
            "tick {i}: reuse last good parse"
        );
    }
    c.tick(NOW, &probe, false);
    assert_eq!(
        c.snapshot(NOW, 85).sessions.len(),
        0,
        "gives up after {TORN_REUSE_TICKS} ticks"
    );
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
    assert_eq!(
        c.snapshot(NOW, 85).registry,
        Health::Healthy,
        "no sessions dir yet is fine"
    );
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
    assert_eq!(
        fetch_usage(t.path(), &http, NOW).result.unwrap_err(),
        UsageError::NoCredentials
    );
    t.write(".credentials.json", &creds(NOW - 1));
    assert_eq!(
        fetch_usage(t.path(), &http, NOW).result.unwrap_err(),
        UsageError::TokenExpired
    );
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
    assert_eq!(
        s.usage.health,
        Health::Degraded("Token expired — open Claude Code to refresh".into())
    );
    assert_eq!(
        s.usage.value.as_ref().unwrap().limits.len(),
        2,
        "stale values kept for display"
    );
    assert_eq!(s.plan.as_deref(), Some("Team · Max 5x"));
    assert_eq!(
        c.usage_age_ms(NOW + 10_000),
        Some(10_000),
        "age counts from the last success"
    );
}

#[test]
fn status_codes_and_bad_bodies() {
    let t = common::TempDir::new("usage-codes");
    t.write(".credentials.json", &creds(NOW + 3_600_000));
    let http = Http::default();
    http.push(429, "", Some(900));
    http.push(500, "", None);
    http.push(200, "<html>", None);
    assert_eq!(
        fetch_usage(t.path(), &http, NOW).result.unwrap_err(),
        UsageError::RateLimited {
            retry_after_s: Some(900)
        }
    );
    assert_eq!(
        fetch_usage(t.path(), &http, NOW).result.unwrap_err(),
        UsageError::Http(500)
    );
    assert!(matches!(
        fetch_usage(t.path(), &http, NOW).result.unwrap_err(),
        UsageError::Parse(_)
    ));
    assert!(matches!(
        fetch_usage(t.path(), &http, NOW).result.unwrap_err(),
        UsageError::Network(_)
    ));
    assert_eq!(
        UsageError::RateLimited {
            retry_after_s: None
        }
        .health_text(),
        "usage unavailable · rate limited"
    );
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
    let rl: Result<Usage, UsageError> = Err(UsageError::RateLimited {
        retry_after_s: None,
    });
    s.after_usage(NOW, &rl, 300);
    assert_eq!(s.usage_backoff_s(), 600);
    assert!(!s.usage_due(NOW + 599_000) && s.usage_due(NOW + 600_000));
    s.after_usage(NOW, &rl, 300);
    assert_eq!(s.usage_backoff_s(), 1200);
    s.refresh_now(NOW);
    assert!(
        !s.usage_due(NOW),
        "refresh_now never cuts a rate-limit backoff short"
    );
    let hinted: Result<Usage, UsageError> = Err(UsageError::RateLimited {
        retry_after_s: Some(3000),
    });
    s.after_usage(NOW, &hinted, 300);
    assert!(
        !s.usage_due(NOW + 2_999_000),
        "Retry-After longer than backoff wins"
    );
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
