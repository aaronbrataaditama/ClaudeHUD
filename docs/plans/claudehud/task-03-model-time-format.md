# Task 3: Data model, time helpers, display formatting

**Goal:** One module of plain data types that every later task shares (`src/model.rs`), a dependency-free time module (`src/timefmt.rs`), and every user-visible string formatter (`src/format.rs`), all unit-tested.

**Spec:** §1 (colours, reasons), §3 (field meanings), §4 (row text such as "Working · 12m", "last turn 132.6k in", "resets Mon 09:00", "$50 of $600").

**Files:**
- Create: `src/timefmt.rs`, `src/format.rs`, `src/model.rs`
- Modify: `src/lib.rs` (add three `pub mod` lines)

**Interfaces:**
- Consumes: nothing.
- Produces (exact names; later tasks import them):
  - `timefmt::{LocalTime, days_from_civil(i32,u32,u32)->i64, civil_from_days(i64)->(i32,u32,u32), utc_parts(i64)->LocalTime, parse_iso8601(&str)->Option<i64>, now_ms()->i64}`. `LocalTime { year: i32, month: u32 /*1-12*/, day: u32, hour: u32, minute: u32, weekday: u32 /*0 = Sunday*/ }`. Unix times are **seconds** in `parse_iso8601`/`utc_parts`, **milliseconds** in `now_ms`.
  - `format::{tokens(u64), uptime(i64 ms), elapsed(i64 ms), ago(i64 ms), money(i64 minor, &str currency), reset_label(i64 reset_s, i64 now_s, &dyn Fn(i64)->LocalTime), spend_reset_label(&LocalTime), pct_label(f64), middle_ellipsis(&str, f32, &dyn Fn(&str)->f32), sentence_case(&str), truncate_chars(&str, usize), limit_noun(&str key, &str label), short_limit(&str)->Option<&'static str>, short_component(&str), component_state_text(ComponentState), model_name(&str), folder_name(&str)}`, all returning `String` unless stated.
  - `model::*`: see Step 7; the whole file is the interface.

---

## Part A: `timefmt`

- [ ] **Step 1: Write the failing tests**

Create `src/timefmt.rs` with only the tests first:

```rust
//! Calendar maths without a time crate. Unix seconds unless a name says `_ms`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for z in [-800_000i64, -1, 0, 1, 11_017, 20_000, 400_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z, "day {z}");
        }
    }

    #[test]
    fn parses_iso_variants() {
        assert_eq!(parse_iso8601("2001-09-09T01:46:40Z"), Some(1_000_000_000));
        assert_eq!(parse_iso8601("2001-09-09T01:46:40.123456Z"), Some(1_000_000_000));
        assert_eq!(parse_iso8601("2001-09-09T01:46:40+00:00"), Some(1_000_000_000));
        assert_eq!(parse_iso8601("2001-09-09T08:46:40+07:00"), Some(1_000_000_000));
        assert_eq!(parse_iso8601("2001-09-08T20:46:40-05:00"), Some(1_000_000_000));
        assert_eq!(parse_iso8601("2001-09-09 01:46:40"), Some(1_000_000_000));
        assert_eq!(parse_iso8601("2001-09-09T01:46:40"), Some(1_000_000_000));
    }

    #[test]
    fn rejects_garbage() {
        for s in ["", "yesterday", "2001-13-01T00:00:00Z", "2001-09-09", "2001-09-09T25:00:00Z", "2001-09-09T01:46:40X"] {
            assert_eq!(parse_iso8601(s), None, "{s}");
        }
    }

    #[test]
    fn utc_parts_of_known_instant() {
        let t = utc_parts(1_000_000_000);
        assert_eq!((t.year, t.month, t.day, t.hour, t.minute, t.weekday), (2001, 9, 9, 1, 46, 0));
        // 2026-09-28 is a Monday
        let mon = parse_iso8601("2026-09-28T09:00:00Z").unwrap();
        assert_eq!(utc_parts(mon).weekday, 1);
    }

    #[test]
    fn now_is_plausible() {
        assert!(now_ms() > 1_700_000_000_000);
    }
}
```

Add to `src/lib.rs`: `pub mod timefmt;`

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --lib timefmt::`
Expected: compile errors `cannot find function days_from_civil` (and the others).

- [ ] **Step 3: Implement**

Put this above the `#[cfg(test)]` block in `src/timefmt.rs`:

```rust
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// 0 = Sunday … 6 = Saturday
    pub weekday: u32,
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = (if m <= 2 { y - 1 } else { y }) as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let d = d as i64;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of `days_from_civil`.
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Breaks a Unix time (seconds) into UTC calendar parts. The platform layer
/// provides the local-time equivalent; tests use this one.
pub fn utc_parts(unix_s: i64) -> LocalTime {
    let days = unix_s.div_euclid(86_400);
    let secs = unix_s.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    LocalTime {
        year,
        month,
        day,
        hour: (secs / 3_600) as u32,
        minute: ((secs % 3_600) / 60) as u32,
        weekday: (days + 4).rem_euclid(7) as u32, // 1970-01-01 was a Thursday
    }
}

/// Parses `YYYY-MM-DD[T ]HH:MM:SS[.frac][Z|±HH:MM|±HHMM]`. No offset means UTC.
/// Returns Unix seconds.
pub fn parse_iso8601(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    if b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        let part = s.get(from..to)?;
        if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse::<i64>().ok()
        } else {
            None
        }
    };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
    }
    let offset = match b.get(i) {
        None => 0,
        Some(b'Z') | Some(b'z') => {
            if i + 1 != b.len() {
                return None;
            }
            0
        }
        Some(&sign) if sign == b'+' || sign == b'-' => {
            let oh = num(i + 1, i + 3)?;
            let om = if b.get(i + 3) == Some(&b':') { num(i + 4, i + 6)? } else { num(i + 3, i + 5).unwrap_or(0) };
            let o = oh * 3_600 + om * 60;
            if sign == b'+' { o } else { -o }
        }
        _ => return None,
    };
    Some(days_from_civil(y as i32, mo as u32, d as u32) * 86_400 + h * 3_600 + mi * 60 + se - offset)
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib timefmt::`
Expected: 5 passed.

---

## Part B: `model`

- [ ] **Step 5: Write the failing tests**

Create `src/model.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_status_mapping() {
        assert_eq!(SessionStatus::from_registry(Some("busy")), SessionStatus::Busy);
        assert_eq!(SessionStatus::from_registry(Some("shell")), SessionStatus::Shell);
        assert_eq!(SessionStatus::from_registry(Some("idle")), SessionStatus::Idle);
        assert_eq!(SessionStatus::from_registry(Some("waiting")), SessionStatus::Waiting);
        // unknown value: still alive, counts as busy (spec §3.1)
        assert_eq!(SessionStatus::from_registry(Some("thinking")), SessionStatus::Busy);
        assert_eq!(SessionStatus::from_registry(None), SessionStatus::Unknown);
    }

    #[test]
    fn waiting_reason_prefers_registry_then_transcript() {
        let mut s = Session { status: SessionStatus::Waiting, waiting_for: Some("approve plan".into()), ..Default::default() };
        assert_eq!(s.waiting_reason().as_deref(), Some("approve plan"));
        s.waiting_for = None;
        assert_eq!(s.waiting_reason().as_deref(), Some("input needed"));

        let mut b = Session { status: SessionStatus::Busy, ..Default::default() };
        assert_eq!(b.waiting_reason(), None);
        assert!(b.is_working());
        b.transcript = Some(TranscriptFacts { pending_user_tool: Some("AskUserQuestion".into()), ..Default::default() });
        assert_eq!(b.waiting_reason().as_deref(), Some("answer a question"));
        assert!(!b.is_working());

        // an idle session with a stale pending question is not waiting
        let i = Session { status: SessionStatus::Idle, transcript: b.transcript.clone(), ..Default::default() };
        assert_eq!(i.waiting_reason(), None);
    }

    #[test]
    fn collected_only_exposes_healthy_values() {
        let c = Collected::healthy(5);
        assert_eq!(c.healthy_value(), Some(&5));
        let d = Collected { health: Health::Degraded("rate limited".into()), value: Some(5) };
        assert_eq!(d.healthy_value(), None);
        assert_eq!(d.value, Some(5));
    }

    #[test]
    fn spend_percentage() {
        let s = Spend { used_minor: 54_600, limit_minor: Some(60_000), currency: "USD".into(), enabled: true };
        assert!((s.pct().unwrap() - 91.0).abs() < 1e-9);
        assert_eq!(Spend { limit_minor: None, ..s.clone() }.pct(), None);
        assert_eq!(Spend { limit_minor: Some(0), ..s }.pct(), None);
    }

    #[test]
    fn colours_match_spec() {
        assert_eq!(Colour::Green.rgb(), 0x4FBE86);
        assert_eq!(Colour::Yellow.rgb(), 0xE9DA4C);
        assert_eq!(Colour::Amber.rgb(), 0xF08A3E);
        assert_eq!(Colour::Red.rgb(), 0xE0444E);
        assert_eq!(Colour::Off.rgb(), 0x3B3F46);
    }

    #[test]
    fn snapshot_deserialises_with_defaults() {
        let s: Snapshot = serde_json::from_str(r#"{"sessions":[{"name":"a","status":"busy"}]}"#).unwrap();
        assert_eq!(s.warn_percent, 85);
        assert_eq!(s.registry, Health::Healthy);
        assert_eq!(s.sessions[0].status, SessionStatus::Busy);
        assert!(matches!(s.usage.health, Health::Unknown(_)));
        let h: Health = serde_json::from_str(r#"{"state":"degraded","reason":"rate limited"}"#).unwrap();
        assert_eq!(h, Health::Degraded("rate limited".into()));
    }

    #[test]
    fn reason_kinds_are_stable() {
        assert_eq!(Reason::NoSessions.kind(), "no_sessions");
        assert_eq!(Reason::Waiting { name: "a".into(), waiting_for: "b".into() }.kind(), "waiting");
        assert_eq!(Reason::Incident { component: "x".into(), state: ComponentState::MajorOutage }.kind(), "incident");
    }
}
```

Add to `src/lib.rs`: `pub mod model;`

- [ ] **Step 6: Run to verify it fails**

Run: `cargo test --lib model::`
Expected: compile errors (types not defined).

- [ ] **Step 7: Implement**

Put above the tests in `src/model.rs`:

```rust
//! Plain data shared by collectors, `fold()`, the panel, the tray and fixtures.

use serde::{Deserialize, Serialize};

/// Opacity of the strip / badge when every session is idle.
pub const DIM_ALPHA: f32 = 0.55;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Busy,
    Shell,
    Idle,
    Waiting,
    #[default]
    Unknown,
}

impl SessionStatus {
    /// Maps the registry's `status` string (§3.1).
    pub fn from_registry(value: Option<&str>) -> SessionStatus {
        match value {
            Some("busy") => SessionStatus::Busy,
            Some("shell") => SessionStatus::Shell,
            Some("idle") => SessionStatus::Idle,
            Some("waiting") => SessionStatus::Waiting,
            Some(_) => SessionStatus::Busy,
            None => SessionStatus::Unknown,
        }
    }

    /// A process that dies in one of these states crashed mid-turn.
    pub fn is_mid_turn(self) -> bool {
        matches!(self, SessionStatus::Busy | SessionStatus::Shell | SessionStatus::Waiting)
    }
}

/// Facts from the last 64 KB of a session transcript (§3.2).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptFacts {
    pub model: Option<String>,
    /// input + cache_read + cache_creation tokens of the last assistant line
    pub last_turn_input_tokens: Option<u64>,
    pub compacted: bool,
    /// an api_error whose retries are exhausted, or with rateLimits set, not yet
    /// followed by an assistant line
    pub api_error_exhausted: bool,
    /// "AskUserQuestion" or "ExitPlanMode" when such a tool call has no result yet
    pub pending_user_tool: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    #[default]
    Running,
    Done,
    Failed,
    Stopped,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Subagent {
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    /// 1 = spawned by the session, 2 = spawned by a sub-agent, …
    pub depth: u32,
    pub state: AgentState,
    pub started_ms: Option<i64>,
    pub ended_ms: Option<i64>,
    pub context_tokens: Option<u64>,
}

/// One live Claude Code session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub pid: u32,
    pub session_id: String,
    pub name: String,
    pub cwd: String,
    pub status: SessionStatus,
    pub waiting_for: Option<String>,
    pub started_at_ms: i64,
    pub status_updated_at_ms: i64,
    pub transcript: Option<TranscriptFacts>,
    pub subagents: Vec<Subagent>,
}

impl Session {
    /// Why this session needs the user, or None. Registry first; the transcript
    /// fallback only applies while the registry says busy/unknown.
    pub fn waiting_reason(&self) -> Option<String> {
        if self.status == SessionStatus::Waiting {
            let why = self.waiting_for.clone().filter(|s| !s.trim().is_empty());
            return Some(why.unwrap_or_else(|| "input needed".to_string()));
        }
        if matches!(self.status, SessionStatus::Busy | SessionStatus::Unknown) {
            match self.transcript.as_ref().and_then(|t| t.pending_user_tool.as_deref()) {
                Some("AskUserQuestion") => return Some("answer a question".to_string()),
                Some("ExitPlanMode") => return Some("approve plan".to_string()),
                _ => {}
            }
        }
        None
    }

    /// Busy or running a shell command, and not waiting on the user.
    pub fn is_working(&self) -> bool {
        matches!(self.status, SessionStatus::Busy | SessionStatus::Shell) && self.waiting_reason().is_none()
    }
}

/// A session whose process died mid-turn; latched until acknowledged.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrashedSession {
    pub key: String,
    pub name: String,
    pub cwd: String,
}

/// JSON: `{"state":"healthy"}`, `{"state":"degraded","reason":"…"}`, `{"state":"unknown","reason":"…"}`
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "state", content = "reason")]
pub enum Health {
    #[default]
    Healthy,
    Degraded(String),
    Unknown(String),
}

/// A collector's latest value plus its health. A degraded collector may keep a
/// stale value for display; only a healthy one may produce colour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collected<T> {
    #[serde(default)]
    pub health: Health,
    pub value: Option<T>,
}

/// Health reason of a collector that has not run yet; not shown to the user.
pub const NOT_COLLECTED: &str = "not collected yet";

impl<T> Default for Collected<T> {
    fn default() -> Self {
        Collected { health: Health::Unknown(NOT_COLLECTED.to_string()), value: None }
    }
}

impl<T> Collected<T> {
    pub fn healthy(value: T) -> Self {
        Collected { health: Health::Healthy, value: Some(value) }
    }

    pub fn healthy_value(&self) -> Option<&T> {
        if self.health == Health::Healthy {
            self.value.as_ref()
        } else {
            None
        }
    }
}

/// One usage window, e.g. key "five_hour", label "Current session · 5h".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limit {
    pub key: String,
    pub label: String,
    /// 0–100 (may exceed 100)
    pub pct: f64,
    /// Unix seconds
    pub resets_at: Option<i64>,
}

/// `extra_usage` spend, amounts in minor units of `currency` (cents for USD).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spend {
    pub used_minor: i64,
    pub limit_minor: Option<i64>,
    pub currency: String,
    pub enabled: bool,
}

impl Spend {
    pub fn pct(&self) -> Option<f64> {
        self.limit_minor
            .filter(|l| *l > 0)
            .map(|l| self.used_minor as f64 * 100.0 / l as f64)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    pub limits: Vec<Limit>,
    pub spend: Option<Spend>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentState {
    Operational,
    Degraded,
    PartialOutage,
    MajorOutage,
    Maintenance,
    #[default]
    Other,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Component {
    pub name: String,
    pub state: ComponentState,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServiceStatus {
    pub description: String,
    pub components: Vec<Component>,
    pub checked_at_ms: i64,
}

/// Everything `fold()`, the panel and the tray need, captured at one instant.
/// Also the `CLAUDEHUD_FIXTURE` file format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub now_ms: i64,
    pub registry: Health,
    pub sessions: Vec<Session>,
    pub crashed: Vec<CrashedSession>,
    pub plan: Option<String>,
    pub usage: Collected<Usage>,
    pub status: Collected<ServiceStatus>,
    pub warn_percent: u8,
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            now_ms: 0,
            registry: Health::Healthy,
            sessions: Vec::new(),
            crashed: Vec::new(),
            plan: None,
            usage: Collected::default(),
            status: Collected::default(),
            warn_percent: 85,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Colour {
    Off,
    Green,
    Yellow,
    Amber,
    Red,
}

impl Colour {
    /// 0xRRGGBB (§1).
    pub fn rgb(self) -> u32 {
        match self {
            Colour::Off => 0x3B3F46,
            Colour::Green => 0x4FBE86,
            Colour::Yellow => 0xE9DA4C,
            Colour::Amber => 0xF08A3E,
            Colour::Red => 0xE0444E,
        }
    }
}

/// Why the light has its colour. Drives the tooltip's first line and the panel summary.
#[derive(Clone, Debug, PartialEq)]
pub enum Reason {
    NoSessions,
    RegistryUnknown,
    AllIdle { count: usize },
    Working { name: String, count: usize },
    Waiting { name: String, waiting_for: String },
    QuotaWarn { key: String, label: String, pct: f64, resets_at: Option<i64> },
    QuotaSpent { key: String, label: String, resets_at: Option<i64> },
    SpendWarn { used_minor: i64, limit_minor: i64, currency: String },
    SpendSpent { used_minor: i64, limit_minor: i64, currency: String },
    ApiError { name: String },
    Crashed { name: String },
    Incident { component: String, state: ComponentState },
}

impl Reason {
    /// Stable snake_case name, used by golden-file tests.
    pub fn kind(&self) -> &'static str {
        match self {
            Reason::NoSessions => "no_sessions",
            Reason::RegistryUnknown => "registry_unknown",
            Reason::AllIdle { .. } => "all_idle",
            Reason::Working { .. } => "working",
            Reason::Waiting { .. } => "waiting",
            Reason::QuotaWarn { .. } => "quota_warn",
            Reason::QuotaSpent { .. } => "quota_spent",
            Reason::SpendWarn { .. } => "spend_warn",
            Reason::SpendSpent { .. } => "spend_spent",
            Reason::ApiError { .. } => "api_error",
            Reason::Crashed { .. } => "crashed",
            Reason::Incident { .. } => "incident",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    pub colour: Colour,
    /// true = draw at DIM_ALPHA (every session idle)
    pub dim: bool,
    pub reason: Reason,
}

impl Light {
    pub fn off() -> Light {
        Light { colour: Colour::Off, dim: false, reason: Reason::NoSessions }
    }
}
```

- [ ] **Step 8: Run tests**

Run: `cargo test --lib model::`
Expected: 7 passed.

---

## Part C: `format`

- [ ] **Step 9: Write the failing tests**

Create `src/format.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::timefmt::{parse_iso8601, utc_parts};

    #[test]
    fn token_counts() {
        assert_eq!(tokens(950), "950");
        assert_eq!(tokens(1_000), "1k");
        assert_eq!(tokens(9_100), "9.1k");
        assert_eq!(tokens(132_600), "132.6k");
        assert_eq!(tokens(999_999), "1M");
        assert_eq!(tokens(1_234_567), "1.2M");
    }

    #[test]
    fn durations() {
        assert_eq!(uptime(30_000), "<1m");
        assert_eq!(uptime(12 * 60_000), "12m");
        assert_eq!(uptime(64 * 60_000), "1h 04m");
        assert_eq!(uptime((2 * 24 + 3) * 3_600_000), "2d 3h");
        assert_eq!(uptime(-5), "<1m");
        assert_eq!(elapsed(48_000), "48s");
        assert_eq!(elapsed(185_000), "3m 05s");
        assert_eq!(elapsed(62 * 60_000), "1h 02m");
        assert_eq!(ago(10_000), "just now");
        assert_eq!(ago(125_000), "2m ago");
        assert_eq!(ago(3 * 3_600_000), "3h ago");
    }

    #[test]
    fn money_formats() {
        assert_eq!(money(5_000, "USD"), "$50");
        assert_eq!(money(54_625, "USD"), "$546.25");
        assert_eq!(money(120_000, "usd"), "$1,200");
        assert_eq!(money(5_000, "EUR"), "€50");
        assert_eq!(money(5_000, "GBP"), "50 GBP");
        assert_eq!(money(0, "USD"), "$0");
    }

    #[test]
    fn reset_labels() {
        let now = parse_iso8601("2026-09-25T10:00:00Z").unwrap(); // a Friday
        assert_eq!(reset_label(parse_iso8601("2026-09-25T14:32:00Z").unwrap(), now, &utc_parts), "resets 14:32");
        assert_eq!(reset_label(parse_iso8601("2026-09-28T09:00:00Z").unwrap(), now, &utc_parts), "resets Mon 09:00");
        assert_eq!(reset_label(parse_iso8601("2026-10-03T09:00:00Z").unwrap(), now, &utc_parts), "resets 3 Oct");
        assert_eq!(spend_reset_label(&utc_parts(now)), "resets 1 Oct");
        let dec = parse_iso8601("2026-12-10T10:00:00Z").unwrap();
        assert_eq!(spend_reset_label(&utc_parts(dec)), "resets 1 Jan");
    }

    #[test]
    fn percentages_never_round_up_to_100() {
        assert_eq!(pct_label(61.4), "61%");
        assert_eq!(pct_label(99.6), "99%");
        assert_eq!(pct_label(100.0), "100%");
        assert_eq!(pct_label(130.0), "100%");
        assert_eq!(pct_label(-3.0), "0%");
    }

    #[test]
    fn middle_ellipsis_keeps_head_and_leaf() {
        let m = |s: &str| s.chars().count() as f32 * 7.0;
        let p = r"C:\Projects\Personal\ClaudeHUD";
        assert_eq!(middle_ellipsis(p, 1000.0, &m), p);
        assert_eq!(middle_ellipsis(p, 180.0, &m), r"C:\Projects\…\ClaudeHUD");
        assert_eq!(middle_ellipsis(p, 100.0, &m), r"C:\…\ClaudeHUD");
        assert_eq!(middle_ellipsis(p, 90.0, &m), r"…\ClaudeHUD");
        assert_eq!(middle_ellipsis(p, 50.0, &m), "…udeHUD");
        assert_eq!(middle_ellipsis("/home/a/b/c/leaf", 70.0, &m), "/…/leaf");
    }

    #[test]
    fn small_helpers() {
        assert_eq!(sentence_case("approve the permission prompt"), "Approve the permission prompt");
        assert_eq!(sentence_case(""), "");
        assert_eq!(truncate_chars("abcdef", 4), "abc…");
        assert_eq!(truncate_chars("abc", 4), "abc");
        assert_eq!(limit_noun("five_hour", "x"), "5-hour limit");
        assert_eq!(limit_noun("weekly_all", "x"), "Weekly limit");
        assert_eq!(limit_noun("seven_day_opus", "x"), "Weekly Opus limit");
        assert_eq!(limit_noun("mystery", "Mystery window"), "Mystery window");
        assert_eq!(short_limit("session"), Some("5h"));
        assert_eq!(short_limit("seven_day"), Some("7d"));
        assert_eq!(short_limit("seven_day_opus"), None);
        assert_eq!(short_component("Claude API (api.anthropic.com)"), "Claude API");
        assert_eq!(short_component("claude.ai"), "claude.ai");
        assert_eq!(component_state_text(crate::model::ComponentState::PartialOutage), "partial outage");
        assert_eq!(folder_name(r"C:\Projects\Personal\ClaudeHUD\"), "ClaudeHUD");
        assert_eq!(folder_name("/home/me/app"), "app");
    }

    #[test]
    fn model_names() {
        assert_eq!(model_name("claude-opus-5"), "Opus 5");
        assert_eq!(model_name("claude-sonnet-5-5"), "Sonnet 5.5");
        assert_eq!(model_name("claude-haiku-4-5-20251001"), "Haiku 4.5");
        assert_eq!(model_name("claude-opus-5[1m]"), "Opus 5");
        assert_eq!(model_name("gpt-9"), "gpt-9");
    }
}
```

Add to `src/lib.rs`: `pub mod format;`

- [ ] **Step 10: Run to verify it fails**

Run: `cargo test --lib format::`
Expected: compile errors (functions missing).

- [ ] **Step 11: Implement**

Above the tests in `src/format.rs`:

```rust
//! Every user-visible string that is built from data. Pure functions.

use crate::model::ComponentState;
use crate::timefmt::LocalTime;

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// 950 → "950", 132_600 → "132.6k", 1_234_567 → "1.2M".
pub fn tokens(n: u64) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    let (v, unit) = if n < 999_950 { (n as f64 / 1_000.0, "k") } else { (n as f64 / 1_000_000.0, "M") };
    let s = format!("{v:.1}");
    let s = s.strip_suffix(".0").unwrap_or(&s);
    format!("{s}{unit}")
}

/// Session running time: "<1m", "12m", "1h 04m", "2d 3h".
pub fn uptime(ms: i64) -> String {
    let m = ms.max(0) / 60_000;
    if m < 1 {
        "<1m".to_string()
    } else if m < 60 {
        format!("{m}m")
    } else if m < 24 * 60 {
        format!("{}h {:02}m", m / 60, m % 60)
    } else {
        format!("{}d {}h", m / (24 * 60), (m / 60) % 24)
    }
}

/// Sub-agent elapsed time: "48s", "3m 05s", "1h 02m".
pub fn elapsed(ms: i64) -> String {
    let s = ms.max(0) / 1_000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3_600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}h {:02}m", s / 3_600, (s / 60) % 60)
    }
}

/// "just now", "2m ago", "3h ago", "2d ago".
pub fn ago(ms: i64) -> String {
    let m = ms.max(0) / 60_000;
    if m < 1 {
        "just now".to_string()
    } else if m < 60 {
        format!("{m}m ago")
    } else if m < 24 * 60 {
        format!("{}h ago", m / 60)
    } else {
        format!("{}d ago", m / (24 * 60))
    }
}

fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Minor units → display. Assumes two decimal places (true for USD/EUR/GBP).
pub fn money(minor: i64, currency: &str) -> String {
    let a = minor.unsigned_abs();
    let (whole, cents) = (a / 100, a % 100);
    let num = if cents == 0 { group_thousands(whole) } else { format!("{}.{:02}", group_thousands(whole), cents) };
    let s = match currency.to_ascii_uppercase().as_str() {
        "USD" => format!("${num}"),
        "EUR" => format!("€{num}"),
        other => format!("{num} {other}"),
    };
    if minor < 0 { format!("-{s}") } else { s }
}

/// "resets 14:32" (same local day), "resets Mon 09:00" (within 7 days), else "resets 3 Oct".
pub fn reset_label(reset_s: i64, now_s: i64, local: &dyn Fn(i64) -> LocalTime) -> String {
    let r = local(reset_s);
    let n = local(now_s);
    if (r.year, r.month, r.day) == (n.year, n.month, n.day) {
        format!("resets {:02}:{:02}", r.hour, r.minute)
    } else if reset_s > now_s && reset_s - now_s < 7 * 86_400 {
        format!("resets {} {:02}:{:02}", WEEKDAYS[(r.weekday % 7) as usize], r.hour, r.minute)
    } else {
        format!("resets {} {}", r.day, MONTHS[(r.month.clamp(1, 12) - 1) as usize])
    }
}

/// Monthly spend resets on the 1st of next month: "resets 1 Oct".
pub fn spend_reset_label(now_local: &LocalTime) -> String {
    let next = if now_local.month >= 12 { 1 } else { now_local.month + 1 };
    format!("resets 1 {}", MONTHS[(next - 1) as usize])
}

/// Floors below 100 so "100%" only ever means spent.
pub fn pct_label(p: f64) -> String {
    let v = if p >= 100.0 { 100.0 } else { p.max(0.0).floor() };
    format!("{}%", v as i64)
}

/// Shortens a path to fit `max_w`, keeping as many leading segments and the
/// final segment as possible: `C:\Projects\…\ClaudeHUD`.
pub fn middle_ellipsis(path: &str, max_w: f32, measure: &dyn Fn(&str) -> f32) -> String {
    if measure(path) <= max_w {
        return path.to_string();
    }
    let sep = if path.contains('\\') { '\\' } else { '/' };
    let parts: Vec<&str> = path.split(sep).collect();
    let last = parts.iter().rev().find(|p| !p.is_empty()).copied().unwrap_or(path);
    if parts.len() > 2 {
        for keep in (1..parts.len() - 1).rev() {
            let head = parts[..keep].join(&sep.to_string());
            let cand = format!("{head}{sep}…{sep}{last}");
            if measure(&cand) <= max_w {
                return cand;
            }
        }
    }
    let cand = format!("…{sep}{last}");
    if measure(&cand) <= max_w {
        return cand;
    }
    let chars: Vec<char> = last.chars().collect();
    for start in 0..chars.len() {
        let cand: String = std::iter::once('…').chain(chars[start..].iter().copied()).collect();
        if measure(&cand) <= max_w {
            return cand;
        }
    }
    "…".to_string()
}

pub fn sentence_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// At most `max` chars; the last one becomes "…" when cut.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1);
    s.chars().take(keep).chain(std::iter::once('…')).collect()
}

/// "5-hour limit", "Weekly limit", "Weekly Opus limit", else the label.
pub fn limit_noun(key: &str, label: &str) -> String {
    match key {
        "five_hour" | "session" => "5-hour limit".to_string(),
        "seven_day" | "weekly_all" => "Weekly limit".to_string(),
        k if k.contains("opus") => "Weekly Opus limit".to_string(),
        k if k.contains("sonnet") => "Weekly Sonnet limit".to_string(),
        _ => label.to_string(),
    }
}

/// Short tooltip names for the two headline windows.
pub fn short_limit(key: &str) -> Option<&'static str> {
    match key {
        "five_hour" | "session" => Some("5h"),
        "seven_day" | "weekly_all" => Some("7d"),
        _ => None,
    }
}

/// "Claude API (api.anthropic.com)" → "Claude API".
pub fn short_component(name: &str) -> String {
    match name.find(" (") {
        Some(i) if name.ends_with(')') => name[..i].to_string(),
        _ => name.to_string(),
    }
}

pub fn component_state_text(state: ComponentState) -> &'static str {
    match state {
        ComponentState::Operational => "operational",
        ComponentState::Degraded => "degraded performance",
        ComponentState::PartialOutage => "partial outage",
        ComponentState::MajorOutage => "major outage",
        ComponentState::Maintenance => "under maintenance",
        ComponentState::Other => "status unknown",
    }
}

/// "claude-opus-5" → "Opus 5", "claude-haiku-4-5-20251001" → "Haiku 4.5".
/// Unrecognised ids are returned unchanged.
pub fn model_name(id: &str) -> String {
    let core = id.split('[').next().unwrap_or(id);
    let rest = core.strip_prefix("claude-").unwrap_or(core);
    let parts: Vec<&str> = rest
        .split('-')
        .filter(|p| !(p.len() == 8 && p.chars().all(|c| c.is_ascii_digit())))
        .collect();
    match parts.first() {
        Some(f) if matches!(*f, "opus" | "sonnet" | "haiku" | "fable") => {
            let version = parts[1..].join(".");
            let fam = sentence_case(f);
            if version.is_empty() { fam } else { format!("{fam} {version}") }
        }
        _ => id.to_string(),
    }
}

/// Last non-empty path segment.
pub fn folder_name(path: &str) -> String {
    path.split(['\\', '/']).rev().find(|p| !p.is_empty()).unwrap_or(path).to_string()
}
```

- [ ] **Step 12: Run all tests, clippy, fmt**

Run: `cargo test --lib` then `cargo clippy --all-targets -- -D warnings` then `cargo fmt`
Expected: all timefmt/model/format tests pass; no clippy warnings.

- [ ] **Step 13: Commit**

```powershell
git add src/lib.rs src/model.rs src/timefmt.rs src/format.rs
git commit -m "feat(model): shared data types, time helpers, display formatting"
```
