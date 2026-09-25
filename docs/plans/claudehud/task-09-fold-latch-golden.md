# Task 9: `fold()`, session ordering, crash latch, fixtures and golden tests

**Goal:** The single function that turns a `Snapshot` into the light's colour and reason, the latch that remembers crashes until acknowledged, the `CLAUDEHUD_FIXTURE` loader, and a directory of golden snapshot files that pins every rule in §1.

**Spec:** §1 (read it fully: triggers, priority, rules), §3.1 (crash rule), §3.6 (which components drive red).

Rules `fold()` must implement, in this order:
1. Registry not `Healthy` → return the previous light unchanged (or off with reason `RegistryUnknown` if there is none).
2. Any latched crash → **red** `Crashed` (even with no live sessions).
3. No live sessions → **off** `NoSessions` (quota and status never light an empty desk).
4. Red, first match wins: a session whose transcript has `api_error_exhausted` → `ApiError`; a **healthy** usage limit ≥ 100 → `QuotaSpent`, naming the limit that resets **last** if several are spent; healthy spend ≥ 100% of its limit → `SpendSpent`; a **healthy** status with a `RED_COMPONENTS` component not `Operational` → `Incident`.
5. Any session with `waiting_reason()` → **yellow** `Waiting` (first in `ordered_sessions` order).
6. Healthy limit ≥ `warn_percent` and < 100 → **amber** `QuotaWarn` (highest pct); else healthy spend ≥ warn → **amber** `SpendWarn`.
7. Any `is_working()` session → **green** `Working` (dim = false); otherwise **green dim** `AllIdle`.

No rule looks at elapsed time.

**Files:**
- Create: `src/state.rs`, `src/latch.rs`, `src/fixture.rs`
- Create: `tests/golden.rs`, `fixtures/snapshots/*.json` (21 files, Step 8)
- Modify: `src/lib.rs` (add `pub mod state; pub mod latch; pub mod fixture;`)

**Interfaces:**
- Consumes: everything in `model`; `collectors::status::RED_COMPONENTS`; `collectors::registry::RegistryEntry` (`key()`, `status`, `name`, `cwd`); `collectors::strip_bom`.
- Produces:
  - `state::fold(&Snapshot, previous: Option<&Light>) -> Light`
  - `state::ordered_sessions(&[Session]) -> Vec<&Session>` (waiting, then working, then the rest; ties: `status_updated_at_ms` descending)
  - `state::severity_colour(pct: f64, warn_percent: u8) -> Colour` (≥100 red, ≥warn amber, else green)
  - `latch::CrashLatch` with `new()`, `update(&mut self, live: &[RegistryEntry], dead: &[RegistryEntry])`, `crashed(&self) -> &[CrashedSession]`, `acknowledge(&mut self)`
  - `fixture::{parse_snapshot(&str) -> Result<Snapshot,String>, load_snapshot(&Path) -> Result<Snapshot,String>, anchor(&mut Snapshot, now_ms: i64)}`. Accepts a bare `Snapshot` or `{"snapshot": {...}, "expect": {...}}`. `anchor` makes hand-written fixtures relative to now (the app calls it; golden tests do not).

---

## Part A: `fold()` and ordering

- [x] **Step 1: Write failing unit tests**

`src/state.rs`, tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sess(name: &str, status: SessionStatus, updated: i64) -> Session {
        Session { name: name.into(), session_id: name.into(), status, status_updated_at_ms: updated, ..Default::default() }
    }

    #[test]
    fn unreadable_registry_holds_previous_light() {
        let s = Snapshot { registry: Health::Unknown("access denied".into()), ..Default::default() };
        let prev = Light { colour: Colour::Yellow, dim: false, reason: Reason::Waiting { name: "a".into(), waiting_for: "b".into() } };
        assert_eq!(fold(&s, Some(&prev)), prev);
        let none = fold(&s, None);
        assert_eq!((none.colour, none.reason.kind()), (Colour::Off, "registry_unknown"));
    }

    #[test]
    fn ordering_is_waiting_working_idle_then_recent_first() {
        let mut waiting = sess("w", SessionStatus::Waiting, 1);
        waiting.waiting_for = Some("approve plan".into());
        let list = vec![sess("idle-new", SessionStatus::Idle, 50), sess("busy-old", SessionStatus::Busy, 10), waiting, sess("busy-new", SessionStatus::Busy, 40), sess("idle-old", SessionStatus::Idle, 5)];
        let names: Vec<&str> = ordered_sessions(&list).iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["w", "busy-new", "busy-old", "idle-new", "idle-old"]);
    }

    #[test]
    fn severity_thresholds() {
        assert_eq!(severity_colour(84.9, 85), Colour::Green);
        assert_eq!(severity_colour(85.0, 85), Colour::Amber);
        assert_eq!(severity_colour(99.9, 85), Colour::Amber);
        assert_eq!(severity_colour(100.0, 85), Colour::Red);
    }

    #[test]
    fn working_reason_counts_working_sessions_only() {
        let s = Snapshot { sessions: vec![sess("a", SessionStatus::Busy, 2), sess("b", SessionStatus::Shell, 3), sess("c", SessionStatus::Idle, 9)], ..Default::default() };
        let l = fold(&s, None);
        assert_eq!(l.reason, Reason::Working { name: "b".into(), count: 2 });
        assert!(!l.dim);
    }
}
```

- [x] **Step 2: Run to verify failure**

Add `pub mod state;` to `src/lib.rs`. Run: `cargo test --lib state::`
Expected: compile errors.

- [x] **Step 3: Implement**

Above the tests in `src/state.rs`:

```rust
//! `fold()`: the only place colour rules live (§1). Collectors report facts;
//! this decides what the light shows.

use crate::collectors::status::RED_COMPONENTS;
use crate::model::{Colour, ComponentState, Health, Light, Reason, Session, Snapshot};

pub fn severity_colour(pct: f64, warn_percent: u8) -> Colour {
    if pct >= 100.0 {
        Colour::Red
    } else if pct >= f64::from(warn_percent) {
        Colour::Amber
    } else {
        Colour::Green
    }
}

fn rank(s: &Session) -> u8 {
    if s.waiting_reason().is_some() {
        0
    } else if s.is_working() {
        1
    } else {
        2
    }
}

/// Waiting, then working, then everything else; most recent status change first.
pub fn ordered_sessions(sessions: &[Session]) -> Vec<&Session> {
    let mut v: Vec<&Session> = sessions.iter().collect();
    v.sort_by(|a, b| rank(a).cmp(&rank(b)).then(b.status_updated_at_ms.cmp(&a.status_updated_at_ms)));
    v
}

fn red(reason: Reason) -> Light {
    Light { colour: Colour::Red, dim: false, reason }
}

fn amber(reason: Reason) -> Light {
    Light { colour: Colour::Amber, dim: false, reason }
}

pub fn fold(s: &Snapshot, previous: Option<&Light>) -> Light {
    if s.registry != Health::Healthy {
        return previous
            .cloned()
            .unwrap_or(Light { colour: Colour::Off, dim: false, reason: Reason::RegistryUnknown });
    }
    if let Some(c) = s.crashed.first() {
        return red(Reason::Crashed { name: c.name.clone() });
    }
    if s.sessions.is_empty() {
        return Light::off();
    }
    let ordered = ordered_sessions(&s.sessions);

    if let Some(x) = ordered.iter().find(|x| x.transcript.as_ref().is_some_and(|t| t.api_error_exhausted)) {
        return red(Reason::ApiError { name: x.name.clone() });
    }
    let usage = s.usage.healthy_value();
    if let Some(u) = usage {
        let spent = u
            .limits
            .iter()
            .filter(|l| l.pct >= 100.0)
            .max_by_key(|l| l.resets_at.unwrap_or(i64::MIN));
        if let Some(l) = spent {
            return red(Reason::QuotaSpent { key: l.key.clone(), label: l.label.clone(), resets_at: l.resets_at });
        }
        if let Some(sp) = &u.spend {
            if let (Some(p), Some(limit)) = (sp.pct(), sp.limit_minor) {
                if p >= 100.0 {
                    return red(Reason::SpendSpent { used_minor: sp.used_minor, limit_minor: limit, currency: sp.currency.clone() });
                }
            }
        }
    }
    if let Some(st) = s.status.healthy_value() {
        let broken = st
            .components
            .iter()
            .find(|c| RED_COMPONENTS.contains(&c.name.as_str()) && c.state != ComponentState::Operational);
        if let Some(c) = broken {
            return red(Reason::Incident { component: c.name.clone(), state: c.state });
        }
    }

    if let Some((x, why)) = ordered.iter().find_map(|x| x.waiting_reason().map(|w| (x, w))) {
        return Light { colour: Colour::Yellow, dim: false, reason: Reason::Waiting { name: x.name.clone(), waiting_for: why } };
    }

    if let Some(u) = usage {
        let warn = f64::from(s.warn_percent);
        let near = u
            .limits
            .iter()
            .filter(|l| l.pct >= warn && l.pct < 100.0)
            .max_by(|a, b| a.pct.total_cmp(&b.pct));
        if let Some(l) = near {
            return amber(Reason::QuotaWarn { key: l.key.clone(), label: l.label.clone(), pct: l.pct, resets_at: l.resets_at });
        }
        if let Some(sp) = &u.spend {
            if let (Some(p), Some(limit)) = (sp.pct(), sp.limit_minor) {
                if p >= warn {
                    return amber(Reason::SpendWarn { used_minor: sp.used_minor, limit_minor: limit, currency: sp.currency.clone() });
                }
            }
        }
    }

    let working: Vec<&&Session> = ordered.iter().filter(|x| x.is_working()).collect();
    match working.first() {
        Some(first) => Light {
            colour: Colour::Green,
            dim: false,
            reason: Reason::Working { name: first.name.clone(), count: working.len() },
        },
        None => Light { colour: Colour::Green, dim: true, reason: Reason::AllIdle { count: s.sessions.len() } },
    }
}
```

- [x] **Step 4: Run tests**

Run: `cargo test --lib state::`
Expected: 4 passed.

## Part B: crash latch

- [x] **Step 5: Write failing tests**

`src/latch.rs`, tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SessionStatus;

    fn entry(pid: u32, start: u64, status: SessionStatus) -> RegistryEntry {
        RegistryEntry {
            file_name: format!("{pid}.json"),
            pid,
            proc_start: Some(start),
            pid_domain: None,
            session_id: format!("s{pid}"),
            name: format!("sess-{pid}"),
            cwd: format!("C:\\w\\{pid}"),
            status,
            waiting_for: None,
            started_at_ms: 0,
            status_updated_at_ms: 0,
        }
    }

    #[test]
    fn dead_busy_entry_never_seen_alive_does_not_latch() {
        let mut l = CrashLatch::new();
        l.update(&[], &[entry(1, 100, SessionStatus::Busy)]);
        assert!(l.crashed().is_empty(), "stale file from before ClaudeHUD started");
    }

    #[test]
    fn seen_alive_then_dead_mid_turn_latches_once() {
        let mut l = CrashLatch::new();
        l.update(&[entry(1, 100, SessionStatus::Busy)], &[]);
        l.update(&[], &[entry(1, 100, SessionStatus::Busy)]);
        assert_eq!(l.crashed().len(), 1);
        assert_eq!(l.crashed()[0].name, "sess-1");
        l.update(&[], &[entry(1, 100, SessionStatus::Busy)]);
        assert_eq!(l.crashed().len(), 1, "no duplicates on later ticks");
    }

    #[test]
    fn waiting_and_shell_count_as_mid_turn_idle_does_not() {
        let mut l = CrashLatch::new();
        let live = [entry(1, 1, SessionStatus::Waiting), entry(2, 2, SessionStatus::Shell), entry(3, 3, SessionStatus::Idle)];
        l.update(&live, &[]);
        l.update(&[], &[entry(1, 1, SessionStatus::Waiting), entry(2, 2, SessionStatus::Shell), entry(3, 3, SessionStatus::Idle)]);
        let names: Vec<&str> = l.crashed().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["sess-1", "sess-2"]);
    }

    #[test]
    fn acknowledge_clears_and_never_relatches() {
        let mut l = CrashLatch::new();
        l.update(&[entry(1, 100, SessionStatus::Busy)], &[]);
        l.update(&[], &[entry(1, 100, SessionStatus::Busy)]);
        l.acknowledge();
        assert!(l.crashed().is_empty());
        l.update(&[], &[entry(1, 100, SessionStatus::Busy)]);
        assert!(l.crashed().is_empty());
    }

    #[test]
    fn recycled_pid_is_a_different_session() {
        let mut l = CrashLatch::new();
        l.update(&[entry(7, 100, SessionStatus::Busy)], &[]);
        // old file now reports dead, a new process got pid 7
        l.update(&[entry(7, 900, SessionStatus::Idle)], &[entry(7, 100, SessionStatus::Busy)]);
        assert_eq!(l.crashed().len(), 1);
    }
}
```

Add `pub mod latch;` to `src/lib.rs`.

- [x] **Step 6: Implement**

Above the tests in `src/latch.rs`:

```rust
//! Remembers sessions that died mid-turn until the user acknowledges them (§1, §3.1).

use crate::collectors::registry::RegistryEntry;
use crate::model::CrashedSession;
use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct CrashLatch {
    seen_alive: HashSet<String>,
    latched: Vec<CrashedSession>,
    acknowledged: HashSet<String>,
}

impl CrashLatch {
    pub fn new() -> CrashLatch {
        CrashLatch::default()
    }

    /// Call once per registry scan.
    pub fn update(&mut self, live: &[RegistryEntry], dead: &[RegistryEntry]) {
        for e in live {
            self.seen_alive.insert(e.key());
        }
        for e in dead {
            let key = e.key();
            // Only a session seen alive during this run can crash; a stale file
            // left over from before ClaudeHUD started never lights red.
            if !self.seen_alive.remove(&key) {
                continue;
            }
            if e.status.is_mid_turn()
                && !self.acknowledged.contains(&key)
                && !self.latched.iter().any(|c| c.key == key)
            {
                self.latched.push(CrashedSession { key, name: e.name.clone(), cwd: e.cwd.clone() });
            }
        }
    }

    pub fn crashed(&self) -> &[CrashedSession] {
        &self.latched
    }

    pub fn acknowledge(&mut self) {
        for c in self.latched.drain(..) {
            self.acknowledged.insert(c.key);
        }
    }
}
```

- [x] **Step 7: Run tests**

Run: `cargo test --lib latch::`
Expected: 5 passed.

## Part C: fixture loader and golden files

- [x] **Step 8: Fixture loader**

`src/fixture.rs`:

```rust
//! `CLAUDEHUD_FIXTURE=<path>` replaces every collector with a fixed Snapshot.
//! Golden files wrap it as `{"snapshot": {...}, "expect": {...}}`; both forms load.

use crate::collectors::strip_bom;
use crate::model::Snapshot;
use serde_json::Value;
use std::path::Path;

pub fn parse_snapshot(text: &str) -> Result<Snapshot, String> {
    let v: Value = serde_json::from_str(strip_bom(text)).map_err(|e| e.to_string())?;
    let inner = match v.get("snapshot") {
        Some(s) => s.clone(),
        None => v,
    };
    serde_json::from_value(inner).map_err(|e| e.to_string())
}

pub fn load_snapshot(path: &Path) -> Result<Snapshot, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_snapshot(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Absolute times are ~1.8e12 ms / 1.8e9 s; anything this small is an offset from now.
const RELATIVE_MS: i64 = 10_000_000_000;
const RELATIVE_S: i64 = 10_000_000;

fn rel_ms(v: &mut i64, now_ms: i64) {
    if v.abs() < RELATIVE_MS {
        *v += now_ms;
    }
}

/// Makes hand-written fixtures live: `now_ms` 0 becomes the real now, and small
/// timestamps (e.g. `"started_at_ms": -720000`, `"resets_at": 16320`) become
/// offsets from now. Golden files use absolute values and are unaffected.
pub fn anchor(s: &mut Snapshot, now_ms: i64) {
    if s.now_ms == 0 {
        s.now_ms = now_ms;
    }
    let now = s.now_ms;
    for sess in &mut s.sessions {
        rel_ms(&mut sess.started_at_ms, now);
        rel_ms(&mut sess.status_updated_at_ms, now);
        for a in &mut sess.subagents {
            for t in [&mut a.started_ms, &mut a.ended_ms].into_iter().flatten() {
                rel_ms(t, now);
            }
        }
    }
    if let Some(u) = &mut s.usage.value {
        for r in u.limits.iter_mut().filter_map(|l| l.resets_at.as_mut()) {
            if r.abs() < RELATIVE_S {
                *r += now / 1000;
            }
        }
    }
    if let Some(st) = &mut s.status.value {
        rel_ms(&mut st.checked_at_ms, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_and_wrapped_forms() {
        let bare = parse_snapshot(r#"{"warn_percent":80}"#).unwrap();
        assert_eq!(bare.warn_percent, 80);
        let wrapped = parse_snapshot(r#"{"snapshot":{"warn_percent":90},"expect":{"colour":"off"}}"#).unwrap();
        assert_eq!(wrapped.warn_percent, 90);
        assert!(parse_snapshot("nope").is_err());
    }

    #[test]
    fn anchor_turns_small_values_into_offsets() {
        let now = 1_790_330_000_000;
        let mut s = parse_snapshot(r#"{"sessions":[{"name":"a","started_at_ms":-720000,"status_updated_at_ms":1790329000000}],"usage":{"value":{"limits":[{"key":"five_hour","pct":1,"resets_at":16320}]}}}"#).unwrap();
        anchor(&mut s, now);
        assert_eq!(s.now_ms, now);
        assert_eq!(s.sessions[0].started_at_ms, now - 720_000);
        assert_eq!(s.sessions[0].status_updated_at_ms, 1_790_329_000_000, "absolute value untouched");
        assert_eq!(s.usage.value.unwrap().limits[0].resets_at, Some(now / 1000 + 16_320));
    }
}
```

Add `pub mod fixture;` to `src/lib.rs`.

- [x] **Step 9: Create the golden files**

Create each file below under `fixtures/snapshots/` with exactly this content. `expect.detail` (optional) is the session name, limit label or component the reason names.

`off_no_sessions.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[],"usage":{"value":{"limits":[{"key":"seven_day","label":"Weekly · all models","pct":100}]}}},
 "expect":{"colour":"off","dim":false,"reason":"no_sessions"}}
```

`green_working.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"claudehud","session_id":"s1","status":"busy","status_updated_at_ms":1790329990000}]},
 "expect":{"colour":"green","dim":false,"reason":"working","detail":"claudehud"}}
```

`green_dim_all_idle.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"a","session_id":"a","status":"idle"},{"name":"b","session_id":"b","status":"idle"}]},
 "expect":{"colour":"green","dim":true,"reason":"all_idle"}}
```

`green_unknown_status_is_dim.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"mystery","session_id":"m","status":"unknown"}]},
 "expect":{"colour":"green","dim":true,"reason":"all_idle"}}
```

`long_turn_stays_green.json` (20 minutes since the last status change, 3 hours since start):
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"build","session_id":"b","status":"busy","started_at_ms":1790319200000,"status_updated_at_ms":1790328800000}]},
 "expect":{"colour":"green","dim":false,"reason":"working","detail":"build"}}
```

`yellow_waiting_beats_amber.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"docs","session_id":"d","status":"busy","status_updated_at_ms":1790329999000},{"name":"portal-service","session_id":"p","status":"waiting","waiting_for":"approve the permission prompt","status_updated_at_ms":1790329000000}],
   "usage":{"value":{"limits":[{"key":"seven_day","label":"Weekly · all models","pct":92}]}}},
 "expect":{"colour":"yellow","dim":false,"reason":"waiting","detail":"portal-service"}}
```

`yellow_question_fallback.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"old-cc","session_id":"o","status":"busy","transcript":{"pending_user_tool":"AskUserQuestion"}}]},
 "expect":{"colour":"yellow","dim":false,"reason":"waiting","detail":"old-cc"}}
```

`amber_quota.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"a","session_id":"a","status":"busy"}],
   "usage":{"value":{"limits":[{"key":"five_hour","label":"Current session · 5h","pct":61},{"key":"seven_day","label":"Weekly · all models","pct":88}]}}},
 "expect":{"colour":"amber","dim":false,"reason":"quota_warn","detail":"Weekly · all models"}}
```

`amber_custom_warn.json`:
```json
{"snapshot":{"now_ms":1790330000000,"warn_percent":80,"sessions":[{"name":"a","session_id":"a","status":"idle"}],
   "usage":{"value":{"limits":[{"key":"five_hour","label":"Current session · 5h","pct":82}]}}},
 "expect":{"colour":"amber","dim":false,"reason":"quota_warn","detail":"Current session · 5h"}}
```

`below_warn_is_green.json`:
```json
{"snapshot":{"now_ms":1790330000000,"warn_percent":90,"sessions":[{"name":"a","session_id":"a","status":"busy"}],
   "usage":{"value":{"limits":[{"key":"seven_day","label":"Weekly · all models","pct":88}]}}},
 "expect":{"colour":"green","dim":false,"reason":"working","detail":"a"}}
```

`amber_spend.json`:
```json
{"snapshot":{"now_ms":1790330000000,"plan":"Enterprise","sessions":[{"name":"billing-api","session_id":"b","status":"busy"}],
   "usage":{"value":{"limits":[],"spend":{"used_minor":54600,"limit_minor":60000,"currency":"USD","enabled":true}}}},
 "expect":{"colour":"amber","dim":false,"reason":"spend_warn"}}
```

`red_quota_spent.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"a","session_id":"a","status":"idle"}],
   "usage":{"value":{"limits":[{"key":"five_hour","label":"Current session · 5h","pct":12},{"key":"seven_day","label":"Weekly · all models","pct":100,"resets_at":1790586000}]}}},
 "expect":{"colour":"red","dim":false,"reason":"quota_spent","detail":"Weekly · all models"}}
```

`red_two_windows_spent.json` (the weekly window resets later, so it is the one that blocks):
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"a","session_id":"a","status":"idle"}],
   "usage":{"value":{"limits":[{"key":"five_hour","label":"Current session · 5h","pct":100,"resets_at":1790346720},{"key":"seven_day","label":"Weekly · all models","pct":100,"resets_at":1790586000}]}}},
 "expect":{"colour":"red","dim":false,"reason":"quota_spent","detail":"Weekly · all models"}}
```

`red_spend_spent.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"a","session_id":"a","status":"busy"}],
   "usage":{"value":{"spend":{"used_minor":60000,"limit_minor":60000,"currency":"USD","enabled":false}}}},
 "expect":{"colour":"red","dim":false,"reason":"spend_spent"}}
```

`red_api_error.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"flaky","session_id":"f","status":"busy","transcript":{"api_error_exhausted":true}}]},
 "expect":{"colour":"red","dim":false,"reason":"api_error","detail":"flaky"}}
```

`red_crash_no_sessions.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[],"crashed":[{"key":"12:34","name":"portal-service","cwd":"C:\\w\\portal-service"}]},
 "expect":{"colour":"red","dim":false,"reason":"crashed","detail":"portal-service"}}
```

`red_crash_beats_waiting.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"p","session_id":"p","status":"waiting"}],"crashed":[{"key":"1:1","name":"gone","cwd":"C:\\w"}]},
 "expect":{"colour":"red","dim":false,"reason":"crashed","detail":"gone"}}
```

`red_incident.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"a","session_id":"a","status":"busy"}],
   "status":{"value":{"description":"Partial outage","components":[{"name":"Claude Code","state":"operational"},{"name":"Claude API (api.anthropic.com)","state":"partial_outage"},{"name":"claude.ai","state":"operational"}]}}},
 "expect":{"colour":"red","dim":false,"reason":"incident","detail":"Claude API (api.anthropic.com)"}}
```

`claude_ai_outage_is_not_red.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"a","session_id":"a","status":"busy"}],
   "status":{"value":{"description":"Major outage","components":[{"name":"Claude Code","state":"operational"},{"name":"Claude API (api.anthropic.com)","state":"operational"},{"name":"claude.ai","state":"major_outage"}]}}},
 "expect":{"colour":"green","dim":false,"reason":"working","detail":"a"}}
```

`unhealthy_usage_gives_no_colour.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"a","session_id":"a","status":"busy"}],
   "usage":{"health":{"state":"degraded","reason":"usage unavailable · rate limited"},"value":{"limits":[{"key":"seven_day","label":"Weekly · all models","pct":100}]}}},
 "expect":{"colour":"green","dim":false,"reason":"working","detail":"a"}}
```

`red_beats_yellow_quota.json`:
```json
{"snapshot":{"now_ms":1790330000000,"sessions":[{"name":"p","session_id":"p","status":"waiting","waiting_for":"approve plan"}],
   "usage":{"value":{"limits":[{"key":"seven_day","label":"Weekly · all models","pct":100}]}}},
 "expect":{"colour":"red","dim":false,"reason":"quota_spent","detail":"Weekly · all models"}}
```

- [x] **Step 10: Golden test runner**

`tests/golden.rs`:

```rust
use serde_json::Value;
use claudehud::fixture::parse_snapshot;
use claudehud::model::Reason;
use claudehud::state::fold;

fn detail(r: &Reason) -> Option<String> {
    match r {
        Reason::Working { name, .. } | Reason::Waiting { name, .. } | Reason::ApiError { name } | Reason::Crashed { name } => Some(name.clone()),
        Reason::QuotaWarn { label, .. } | Reason::QuotaSpent { label, .. } => Some(label.clone()),
        Reason::Incident { component, .. } => Some(component.clone()),
        _ => None,
    }
}

#[test]
fn golden_snapshots() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/snapshots");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    let mut failures = Vec::new();
    let mut count = 0;
    for p in files.iter().filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json")) {
        count += 1;
        let text = std::fs::read_to_string(p).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let expect = &v["expect"];
        let snap = parse_snapshot(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let light = fold(&snap, None);
        let colour = serde_json::to_value(light.colour).unwrap();
        let mut problems = Vec::new();
        if colour != expect["colour"] {
            problems.push(format!("colour {colour} != {}", expect["colour"]));
        }
        if let Some(d) = expect.get("dim").and_then(Value::as_bool) {
            if d != light.dim {
                problems.push(format!("dim {} != {d}", light.dim));
            }
        }
        if Some(light.reason.kind()) != expect["reason"].as_str() {
            problems.push(format!("reason {} != {}", light.reason.kind(), expect["reason"]));
        }
        if let Some(d) = expect.get("detail").and_then(Value::as_str) {
            if detail(&light.reason).as_deref() != Some(d) {
                problems.push(format!("detail {:?} != {d}", detail(&light.reason)));
            }
        }
        if !problems.is_empty() {
            failures.push(format!("{}: {}", p.file_name().unwrap().to_string_lossy(), problems.join("; ")));
        }
    }
    assert!(count >= 21, "expected at least 21 golden files, found {count}");
    assert!(failures.is_empty(), "golden failures:\n{}", failures.join("\n"));
}
```

- [x] **Step 11: Run all tests**

Run: `cargo test`
Expected: every test passes, including `golden_snapshots`. If a golden file fails, re-read §1: fix `fold()` if it disagrees with the spec, never the expectation.

- [x] **Step 12: Lint and commit**

```powershell
cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/lib.rs src/state.rs src/latch.rs src/fixture.rs tests/golden.rs fixtures/snapshots
git commit -m "feat(state): fold rules, crash latch, golden snapshots"
```
