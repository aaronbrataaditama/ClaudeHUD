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
    v.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then(b.status_updated_at_ms.cmp(&a.status_updated_at_ms))
    });
    v
}

fn red(reason: Reason) -> Light {
    Light {
        colour: Colour::Red,
        dim: false,
        reason,
    }
}

fn amber(reason: Reason) -> Light {
    Light {
        colour: Colour::Amber,
        dim: false,
        reason,
    }
}

pub fn fold(s: &Snapshot, previous: Option<&Light>) -> Light {
    if s.registry != Health::Healthy {
        return previous.cloned().unwrap_or(Light {
            colour: Colour::Off,
            dim: false,
            reason: Reason::RegistryUnknown,
        });
    }
    if let Some(c) = s.crashed.first() {
        return red(Reason::Crashed {
            name: c.name.clone(),
        });
    }
    if s.sessions.is_empty() {
        return Light::off();
    }
    let ordered = ordered_sessions(&s.sessions);

    if let Some(x) = ordered
        .iter()
        .find(|x| x.transcript.as_ref().is_some_and(|t| t.api_error_exhausted))
    {
        return red(Reason::ApiError {
            name: x.name.clone(),
        });
    }
    let usage = s.usage.healthy_value();
    if let Some(u) = usage {
        let spent = u
            .limits
            .iter()
            .filter(|l| l.pct >= 100.0)
            .max_by_key(|l| l.resets_at.unwrap_or(i64::MIN));
        if let Some(l) = spent {
            return red(Reason::QuotaSpent {
                key: l.key.clone(),
                label: l.label.clone(),
                resets_at: l.resets_at,
            });
        }
        if let Some(sp) = &u.spend {
            if let (Some(p), Some(limit)) = (sp.pct(), sp.limit_minor) {
                if p >= 100.0 {
                    return red(Reason::SpendSpent {
                        used_minor: sp.used_minor,
                        limit_minor: limit,
                        currency: sp.currency.clone(),
                    });
                }
            }
        }
    }
    if let Some(st) = s.status.healthy_value() {
        let broken = st.components.iter().find(|c| {
            RED_COMPONENTS.contains(&c.name.as_str()) && c.state != ComponentState::Operational
        });
        if let Some(c) = broken {
            return red(Reason::Incident {
                component: c.name.clone(),
                state: c.state,
            });
        }
    }

    if let Some((x, why)) = ordered
        .iter()
        .find_map(|x| x.waiting_reason().map(|w| (x, w)))
    {
        return Light {
            colour: Colour::Yellow,
            dim: false,
            reason: Reason::Waiting {
                name: x.name.clone(),
                waiting_for: why,
            },
        };
    }

    if let Some(u) = usage {
        let warn = f64::from(s.warn_percent);
        let near = u
            .limits
            .iter()
            .filter(|l| l.pct >= warn && l.pct < 100.0)
            .max_by(|a, b| a.pct.total_cmp(&b.pct));
        if let Some(l) = near {
            return amber(Reason::QuotaWarn {
                key: l.key.clone(),
                label: l.label.clone(),
                pct: l.pct,
                resets_at: l.resets_at,
            });
        }
        if let Some(sp) = &u.spend {
            if let (Some(p), Some(limit)) = (sp.pct(), sp.limit_minor) {
                if p >= warn {
                    return amber(Reason::SpendWarn {
                        used_minor: sp.used_minor,
                        limit_minor: limit,
                        currency: sp.currency.clone(),
                    });
                }
            }
        }
    }

    let working: Vec<&&Session> = ordered.iter().filter(|x| x.is_working()).collect();
    match working.first() {
        Some(first) => Light {
            colour: Colour::Green,
            dim: false,
            reason: Reason::Working {
                name: first.name.clone(),
                count: working.len(),
            },
        },
        None => Light {
            colour: Colour::Green,
            dim: true,
            reason: Reason::AllIdle {
                count: s.sessions.len(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SessionStatus;

    fn sess(name: &str, status: SessionStatus, updated: i64) -> Session {
        Session {
            name: name.into(),
            session_id: name.into(),
            status,
            status_updated_at_ms: updated,
            ..Default::default()
        }
    }

    #[test]
    fn unreadable_registry_holds_previous_light() {
        let s = Snapshot {
            registry: Health::Unknown("access denied".into()),
            ..Default::default()
        };
        let prev = Light {
            colour: Colour::Yellow,
            dim: false,
            reason: Reason::Waiting {
                name: "a".into(),
                waiting_for: "b".into(),
            },
        };
        assert_eq!(fold(&s, Some(&prev)), prev);
        let none = fold(&s, None);
        assert_eq!(
            (none.colour, none.reason.kind()),
            (Colour::Off, "registry_unknown")
        );
    }

    #[test]
    fn ordering_is_waiting_working_idle_then_recent_first() {
        let mut waiting = sess("w", SessionStatus::Waiting, 1);
        waiting.waiting_for = Some("approve plan".into());
        let list = vec![
            sess("idle-new", SessionStatus::Idle, 50),
            sess("busy-old", SessionStatus::Busy, 10),
            waiting,
            sess("busy-new", SessionStatus::Busy, 40),
            sess("idle-old", SessionStatus::Idle, 5),
        ];
        let names: Vec<&str> = ordered_sessions(&list)
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["w", "busy-new", "busy-old", "idle-new", "idle-old"]
        );
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
        let s = Snapshot {
            sessions: vec![
                sess("a", SessionStatus::Busy, 2),
                sess("b", SessionStatus::Shell, 3),
                sess("c", SessionStatus::Idle, 9),
            ],
            ..Default::default()
        };
        let l = fold(&s, None);
        assert_eq!(
            l.reason,
            Reason::Working {
                name: "b".into(),
                count: 2
            }
        );
        assert!(!l.dim);
    }
}
