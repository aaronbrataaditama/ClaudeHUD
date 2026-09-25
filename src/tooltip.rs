//! Tray tooltip: line one says why the light has its colour, line two gives
//! counts and usage (§5). Windows truncates at 127 UTF-16 units + NUL.

use crate::format::{
    component_state_text, limit_noun, money, pct_label, reset_label, short_component, short_limit,
    spend_reset_label, truncate_chars,
};
#[cfg(test)]
use crate::model::{Collected, Colour, ComponentState, Limit, Session, Usage};
use crate::model::{Health, Light, Reason, SessionStatus, Snapshot, Spend};
use crate::timefmt::LocalTime;

pub const MAX_UTF16: usize = 127;
const LINE1_MAX: usize = 80;

/// Builds `f(name)` and, if too long, shortens the name (never the reason).
fn with_name(name: &str, f: impl Fn(&str) -> String) -> String {
    let full = f(name);
    let len = full.chars().count();
    if len <= LINE1_MAX {
        return full;
    }
    let keep = name.chars().count().saturating_sub(len - LINE1_MAX).max(4);
    truncate_chars(&f(&truncate_chars(name, keep)), LINE1_MAX)
}

fn line_one(r: &Reason, now_s: i64, local: &dyn Fn(i64) -> LocalTime) -> String {
    let reset = |t: &Option<i64>| {
        t.map(|t| format!(" · {}", reset_label(t, now_s, local)))
            .unwrap_or_default()
    };
    let spend_pct = |used: i64, limit: i64| pct_label(used as f64 * 100.0 / limit.max(1) as f64);
    match r {
        Reason::NoSessions => "ClaudeHUD · no Claude sessions".to_string(),
        Reason::RegistryUnknown => "Session registry unreadable".to_string(),
        Reason::AllIdle { count: 1 } => "1 session open, idle".to_string(),
        Reason::AllIdle { count } => format!("{count} sessions open, all idle"),
        Reason::Working { name, count } => with_name(name, |n| {
            if *count > 1 {
                format!("{n} and {} more working", count - 1)
            } else {
                format!("{n} working")
            }
        }),
        Reason::Waiting { name, waiting_for } => with_name(name, |n| format!("{n}: {waiting_for}")),
        Reason::QuotaWarn {
            key,
            label,
            pct,
            resets_at,
        } => {
            format!(
                "{} at {}{}",
                limit_noun(key, label),
                pct_label(*pct),
                reset(resets_at)
            )
        }
        Reason::QuotaSpent {
            key,
            label,
            resets_at,
        } => format!("{} spent{}", limit_noun(key, label), reset(resets_at)),
        Reason::SpendWarn {
            used_minor,
            limit_minor,
            currency,
        } => format!(
            "Spend at {} · {} of {} · {}",
            spend_pct(*used_minor, *limit_minor),
            money(*used_minor, currency),
            money(*limit_minor, currency),
            spend_reset_label(&local(now_s))
        ),
        Reason::SpendSpent {
            used_minor,
            limit_minor,
            currency,
        } => format!(
            "Spend limit reached · {} of {}",
            money(*used_minor, currency),
            money(*limit_minor, currency)
        ),
        Reason::ApiError { name } => {
            with_name(name, |n| format!("{n}: API error, retries exhausted"))
        }
        Reason::Crashed { name } => with_name(name, |n| {
            format!("{n} crashed mid-turn · click to acknowledge")
        }),
        Reason::Incident { component, state } => {
            format!(
                "{}: {}",
                short_component(component),
                component_state_text(*state)
            )
        }
    }
}

fn line_two(s: &Snapshot) -> String {
    let mut parts: Vec<String> = Vec::new();
    let running = s
        .sessions
        .iter()
        .filter(|x| !matches!(x.status, SessionStatus::Idle | SessionStatus::Unknown))
        .count();
    let idle = s.sessions.len() - running;
    if running > 0 {
        parts.push(format!("{running} running"));
    } else if idle > 0 {
        parts.push(format!("{idle} idle"));
    }
    match s.usage.healthy_value() {
        Some(u) => {
            let before = parts.len();
            for l in &u.limits {
                if let Some(short) = short_limit(&l.key) {
                    parts.push(format!("{short} {}", pct_label(l.pct)));
                }
            }
            if parts.len() == before {
                if let Some(p) = u.spend.as_ref().and_then(Spend::pct) {
                    parts.push(format!("spend {}", pct_label(p)));
                }
            }
        }
        None => {
            if let Health::Degraded(reason) = &s.usage.health {
                parts.push(truncate_chars(reason, 44));
            }
        }
    }
    parts.join(" · ")
}

pub fn tooltip(light: &Light, snap: &Snapshot, local: &dyn Fn(i64) -> LocalTime) -> String {
    let l1 = line_one(&light.reason, snap.now_ms / 1000, local);
    let l2 = line_two(snap);
    let mut out = if l2.is_empty() {
        l1
    } else {
        format!("{l1}\n{l2}")
    };
    while out.encode_utf16().count() > MAX_UTF16 {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timefmt::{parse_iso8601, utc_parts};

    fn now_ms() -> i64 {
        parse_iso8601("2026-09-25T10:00:00Z").unwrap() * 1000
    }
    fn busy(name: &str) -> Session {
        Session {
            name: name.into(),
            session_id: name.into(),
            status: SessionStatus::Busy,
            ..Default::default()
        }
    }
    fn usage(p5: f64, p7: f64) -> Collected<Usage> {
        Collected::healthy(Usage {
            limits: vec![
                Limit {
                    key: "five_hour".into(),
                    label: "Current session · 5h".into(),
                    pct: p5,
                    resets_at: None,
                },
                Limit {
                    key: "seven_day".into(),
                    label: "Weekly · all models".into(),
                    pct: p7,
                    resets_at: parse_iso8601("2026-09-28T09:00:00Z"),
                },
            ],
            spend: None,
        })
    }
    fn tip(reason: Reason, snap: &Snapshot) -> String {
        tooltip(
            &Light {
                colour: Colour::Green,
                dim: false,
                reason,
            },
            snap,
            &utc_parts,
        )
    }

    #[test]
    fn waiting_with_counts_and_usage() {
        let snap = Snapshot {
            now_ms: now_ms(),
            sessions: vec![busy("a"), busy("b"), busy("portal-service")],
            usage: usage(61.0, 88.0),
            ..Default::default()
        };
        let t = tip(
            Reason::Waiting {
                name: "portal-service".into(),
                waiting_for: "approve the permission prompt".into(),
            },
            &snap,
        );
        assert_eq!(
            t,
            "portal-service: approve the permission prompt\n3 running · 5h 61% · 7d 88%"
        );
    }

    #[test]
    fn quota_lines() {
        let snap = Snapshot {
            now_ms: now_ms(),
            sessions: vec![busy("a"), busy("b")],
            usage: usage(61.0, 88.0),
            ..Default::default()
        };
        let r = Reason::QuotaWarn {
            key: "seven_day".into(),
            label: "Weekly · all models".into(),
            pct: 88.0,
            resets_at: parse_iso8601("2026-09-28T09:00:00Z"),
        };
        assert_eq!(
            tip(r, &snap),
            "Weekly limit at 88% · resets Mon 09:00\n2 running · 5h 61% · 7d 88%"
        );
        let idle = Session {
            status: SessionStatus::Idle,
            ..busy("a")
        };
        let snap = Snapshot {
            sessions: vec![idle],
            usage: usage(12.0, 100.0),
            ..snap
        };
        let r = Reason::QuotaSpent {
            key: "seven_day".into(),
            label: "Weekly · all models".into(),
            resets_at: parse_iso8601("2026-09-28T09:00:00Z"),
        };
        assert_eq!(
            tip(r, &snap),
            "Weekly limit spent · resets Mon 09:00\n1 idle · 5h 12% · 7d 100%"
        );
    }

    #[test]
    fn spend_off_crash_incident() {
        let snap = Snapshot {
            now_ms: now_ms(),
            sessions: vec![busy("billing-api")],
            ..Default::default()
        };
        let r = Reason::SpendWarn {
            used_minor: 54_600,
            limit_minor: 60_000,
            currency: "USD".into(),
        };
        assert_eq!(
            tip(r, &snap),
            "Spend at 91% · $546 of $600 · resets 1 Oct\n1 running"
        );
        let off = Snapshot {
            now_ms: now_ms(),
            usage: usage(23.0, 47.0),
            ..Default::default()
        };
        assert_eq!(
            tip(Reason::NoSessions, &off),
            "ClaudeHUD · no Claude sessions\n5h 23% · 7d 47%"
        );
        assert_eq!(
            tip(
                Reason::Crashed {
                    name: "portal-service".into()
                },
                &snap
            ),
            "portal-service crashed mid-turn · click to acknowledge\n1 running"
        );
        let r = Reason::Incident {
            component: "Claude API (api.anthropic.com)".into(),
            state: ComponentState::PartialOutage,
        };
        assert_eq!(tip(r, &snap), "Claude API: partial outage\n1 running");
    }

    #[test]
    fn degraded_usage_is_named_not_coloured() {
        let snap = Snapshot {
            now_ms: now_ms(),
            sessions: vec![busy("a")],
            usage: Collected {
                health: Health::Degraded("usage unavailable · rate limited".into()),
                value: None,
            },
            ..Default::default()
        };
        assert_eq!(
            tip(
                Reason::Working {
                    name: "a".into(),
                    count: 1
                },
                &snap
            ),
            "a working\n1 running · usage unavailable · rate limited"
        );
    }

    #[test]
    fn long_names_are_shortened_and_total_fits() {
        let long = "x".repeat(200);
        let snap = Snapshot {
            now_ms: now_ms(),
            sessions: vec![busy(&long)],
            usage: usage(61.0, 88.0),
            ..Default::default()
        };
        let t = tip(
            Reason::Waiting {
                name: long.clone(),
                waiting_for: "approve the permission prompt".into(),
            },
            &snap,
        );
        assert!(
            t.encode_utf16().count() <= MAX_UTF16,
            "{} units",
            t.encode_utf16().count()
        );
        let first = t.lines().next().unwrap();
        assert!(
            first.ends_with(": approve the permission prompt"),
            "reason kept, name cut: {first}"
        );
        assert!(first.contains('…'));
    }
}
