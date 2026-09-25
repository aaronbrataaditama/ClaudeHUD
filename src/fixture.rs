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
        let wrapped =
            parse_snapshot(r#"{"snapshot":{"warn_percent":90},"expect":{"colour":"off"}}"#)
                .unwrap();
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
        assert_eq!(
            s.sessions[0].status_updated_at_ms, 1_790_329_000_000,
            "absolute value untouched"
        );
        assert_eq!(
            s.usage.value.unwrap().limits[0].resets_at,
            Some(now / 1000 + 16_320)
        );
    }
}
