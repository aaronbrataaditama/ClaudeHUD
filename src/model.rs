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
        matches!(
            self,
            SessionStatus::Busy | SessionStatus::Shell | SessionStatus::Waiting
        )
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
            match self
                .transcript
                .as_ref()
                .and_then(|t| t.pending_user_tool.as_deref())
            {
                Some("AskUserQuestion") => return Some("answer a question".to_string()),
                Some("ExitPlanMode") => return Some("approve plan".to_string()),
                _ => {}
            }
        }
        None
    }

    /// Busy or running a shell command, and not waiting on the user.
    pub fn is_working(&self) -> bool {
        matches!(self.status, SessionStatus::Busy | SessionStatus::Shell)
            && self.waiting_reason().is_none()
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
        Collected {
            health: Health::Unknown(NOT_COLLECTED.to_string()),
            value: None,
        }
    }
}

impl<T> Collected<T> {
    pub fn healthy(value: T) -> Self {
        Collected {
            health: Health::Healthy,
            value: Some(value),
        }
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
    AllIdle {
        count: usize,
    },
    Working {
        name: String,
        count: usize,
    },
    Waiting {
        name: String,
        waiting_for: String,
    },
    QuotaWarn {
        key: String,
        label: String,
        pct: f64,
        resets_at: Option<i64>,
    },
    QuotaSpent {
        key: String,
        label: String,
        resets_at: Option<i64>,
    },
    SpendWarn {
        used_minor: i64,
        limit_minor: i64,
        currency: String,
    },
    SpendSpent {
        used_minor: i64,
        limit_minor: i64,
        currency: String,
    },
    ApiError {
        name: String,
    },
    Crashed {
        name: String,
    },
    Incident {
        component: String,
        state: ComponentState,
    },
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
        Light {
            colour: Colour::Off,
            dim: false,
            reason: Reason::NoSessions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_status_mapping() {
        assert_eq!(
            SessionStatus::from_registry(Some("busy")),
            SessionStatus::Busy
        );
        assert_eq!(
            SessionStatus::from_registry(Some("shell")),
            SessionStatus::Shell
        );
        assert_eq!(
            SessionStatus::from_registry(Some("idle")),
            SessionStatus::Idle
        );
        assert_eq!(
            SessionStatus::from_registry(Some("waiting")),
            SessionStatus::Waiting
        );
        // unknown value: still alive, counts as busy (spec §3.1)
        assert_eq!(
            SessionStatus::from_registry(Some("thinking")),
            SessionStatus::Busy
        );
        assert_eq!(SessionStatus::from_registry(None), SessionStatus::Unknown);
    }

    #[test]
    fn waiting_reason_prefers_registry_then_transcript() {
        let mut s = Session {
            status: SessionStatus::Waiting,
            waiting_for: Some("approve plan".into()),
            ..Default::default()
        };
        assert_eq!(s.waiting_reason().as_deref(), Some("approve plan"));
        s.waiting_for = None;
        assert_eq!(s.waiting_reason().as_deref(), Some("input needed"));

        let mut b = Session {
            status: SessionStatus::Busy,
            ..Default::default()
        };
        assert_eq!(b.waiting_reason(), None);
        assert!(b.is_working());
        b.transcript = Some(TranscriptFacts {
            pending_user_tool: Some("AskUserQuestion".into()),
            ..Default::default()
        });
        assert_eq!(b.waiting_reason().as_deref(), Some("answer a question"));
        assert!(!b.is_working());

        // an idle session with a stale pending question is not waiting
        let i = Session {
            status: SessionStatus::Idle,
            transcript: b.transcript.clone(),
            ..Default::default()
        };
        assert_eq!(i.waiting_reason(), None);
    }

    #[test]
    fn collected_only_exposes_healthy_values() {
        let c = Collected::healthy(5);
        assert_eq!(c.healthy_value(), Some(&5));
        let d = Collected {
            health: Health::Degraded("rate limited".into()),
            value: Some(5),
        };
        assert_eq!(d.healthy_value(), None);
        assert_eq!(d.value, Some(5));
    }

    #[test]
    fn spend_percentage() {
        let s = Spend {
            used_minor: 54_600,
            limit_minor: Some(60_000),
            currency: "USD".into(),
            enabled: true,
        };
        assert!((s.pct().unwrap() - 91.0).abs() < 1e-9);
        assert_eq!(
            Spend {
                limit_minor: None,
                ..s.clone()
            }
            .pct(),
            None
        );
        assert_eq!(
            Spend {
                limit_minor: Some(0),
                ..s
            }
            .pct(),
            None
        );
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
        let s: Snapshot =
            serde_json::from_str(r#"{"sessions":[{"name":"a","status":"busy"}]}"#).unwrap();
        assert_eq!(s.warn_percent, 85);
        assert_eq!(s.registry, Health::Healthy);
        assert_eq!(s.sessions[0].status, SessionStatus::Busy);
        assert!(matches!(s.usage.health, Health::Unknown(_)));
        let h: Health =
            serde_json::from_str(r#"{"state":"degraded","reason":"rate limited"}"#).unwrap();
        assert_eq!(h, Health::Degraded("rate limited".into()));
    }

    #[test]
    fn reason_kinds_are_stable() {
        assert_eq!(Reason::NoSessions.kind(), "no_sessions");
        assert_eq!(
            Reason::Waiting {
                name: "a".into(),
                waiting_for: "b".into()
            }
            .kind(),
            "waiting"
        );
        assert_eq!(
            Reason::Incident {
                component: "x".into(),
                state: ComponentState::MajorOutage
            }
            .kind(),
            "incident"
        );
    }
}
