//! `~/.claude/sessions/<pid>.json`: one file per Claude Code process (§3.1).
//! Never opens `*.key` files: they hold a capability token.

use super::strip_bom;
use crate::format::folder_name;
use crate::model::{Session, SessionStatus};
use serde::Deserialize;
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};

/// 1 s in FILETIME units (100 ns). A recycled pid cannot start within 1 s of the old one.
pub const PROC_START_TOLERANCE: u64 = 10_000_000;
pub const MAX_ENTRY_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct RegistryEntry {
    pub file_name: String,
    pub pid: u32,
    /// Process creation time as a Windows FILETIME.
    pub proc_start: Option<u64>,
    pub pid_domain: Option<String>,
    pub session_id: String,
    pub name: String,
    pub cwd: String,
    pub status: SessionStatus,
    pub waiting_for: Option<String>,
    pub started_at_ms: i64,
    pub status_updated_at_ms: i64,
}

impl RegistryEntry {
    /// Identity that survives pid reuse.
    pub fn key(&self) -> String {
        format!("{}:{}", self.pid, self.proc_start.unwrap_or(0))
    }

    pub fn to_session(&self) -> Session {
        Session {
            pid: self.pid,
            session_id: self.session_id.clone(),
            name: self.name.clone(),
            cwd: self.cwd.clone(),
            status: self.status,
            waiting_for: self.waiting_for.clone(),
            started_at_ms: self.started_at_ms,
            status_updated_at_ms: self.status_updated_at_ms,
            transcript: None,
            subagents: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegistryScan {
    pub live: Vec<RegistryEntry>,
    /// Files whose process is gone (stale leftovers or crashes).
    pub dead: Vec<RegistryEntry>,
    /// Present but unparseable this time (often caught mid-rewrite).
    pub unreadable: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    Alive,
    Dead,
    /// Written by another machine (shared home directory): ignore entirely.
    Foreign,
}

pub trait ProcessProbe {
    /// Creation time of a *running* process as FILETIME, or None if no such process.
    fn creation_filetime(&self, pid: u32) -> Option<u64>;
    /// This machine's domain, e.g. "win32:laptop-6n3ets5a". Compared case-insensitively.
    fn pid_domain(&self) -> String;
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    pid: Option<u32>,
    proc_start: Option<Value>,
    pid_domain: Option<String>,
    session_id: Option<String>,
    name: Option<String>,
    cwd: Option<String>,
    status: Option<String>,
    waiting_for: Option<String>,
    started_at: Option<f64>,
    status_updated_at: Option<f64>,
}

pub fn parse_entry(file_name: &str, text: &str) -> Result<RegistryEntry, String> {
    let raw: Raw =
        serde_json::from_str(strip_bom(text)).map_err(|e| format!("{file_name}: {e}"))?;
    let pid = raw.pid.ok_or_else(|| format!("{file_name}: no pid"))?;
    let session_id = raw
        .session_id
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{file_name}: no sessionId"))?;
    let proc_start = raw.proc_start.and_then(|v| match v {
        Value::String(s) => s.trim().parse::<u64>().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    });
    let cwd = raw.cwd.unwrap_or_default();
    let name = raw
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| {
            let f = folder_name(&cwd);
            if f.is_empty() {
                session_id.chars().take(8).collect()
            } else {
                f
            }
        });
    let started_at_ms = raw.started_at.unwrap_or(0.0) as i64;
    Ok(RegistryEntry {
        file_name: file_name.to_string(),
        pid,
        proc_start,
        pid_domain: raw.pid_domain,
        session_id,
        name,
        cwd,
        status: SessionStatus::from_registry(raw.status.as_deref()),
        waiting_for: raw.waiting_for,
        started_at_ms,
        status_updated_at_ms: raw
            .status_updated_at
            .map(|v| v as i64)
            .unwrap_or(started_at_ms),
    })
}

pub fn liveness(e: &RegistryEntry, probe: &dyn ProcessProbe) -> Liveness {
    if let Some(d) = &e.pid_domain {
        if !d.eq_ignore_ascii_case(&probe.pid_domain()) {
            return Liveness::Foreign;
        }
    }
    match probe.creation_filetime(e.pid) {
        None => Liveness::Dead,
        Some(created) => match e.proc_start {
            Some(ps) if created.abs_diff(ps) > PROC_START_TOLERANCE => Liveness::Dead,
            _ => Liveness::Alive,
        },
    }
}

fn read_small(path: &Path) -> io::Result<String> {
    if std::fs::metadata(path)?.len() > MAX_ENTRY_BYTES {
        return Err(io::Error::other("registry entry too large"));
    }
    std::fs::read_to_string(path)
}

/// Scans `dir` (normally `~/.claude/sessions`). A missing directory is not an
/// error: Claude Code has simply never run.
pub fn scan_dir(dir: &Path, probe: &dyn ProcessProbe) -> Result<RegistryScan, String> {
    let mut scan = RegistryScan::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(scan),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    };
    let mut files: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|d| {
            let name = d.file_name().to_string_lossy().into_owned();
            // Only *.json. This also guarantees *.key files are never opened.
            name.ends_with(".json").then(|| (name, d.path()))
        })
        .collect();
    files.sort();
    for (name, path) in files {
        let text = match read_small(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue, // deleted since listing
            Err(_) => {
                scan.unreadable.push(name);
                continue;
            }
        };
        match parse_entry(&name, &text) {
            Ok(e) => match liveness(&e, probe) {
                Liveness::Alive => scan.live.push(e),
                Liveness::Dead => scan.dead.push(e),
                Liveness::Foreign => {}
            },
            Err(_) => scan.unreadable.push(name),
        }
    }
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const SAMPLE: &str = r#"{"pid":22488,"sessionId":"34bea346-2ede-4248-8364-2c33033971a7","cwd":"C:\\Projects\\Personal\\ClaudeHUD","startedAt":1790301420182,"procStart":"134347750190770188","version":"2.1.281","peerProtocol":1,"peerFeatures":["notify_idle","artifact_yield"],"kind":"interactive","entrypoint":"cli","pidDomain":"win32:laptop-6n3ets5a","messagingSocketPath":"\\\\.\\pipe\\LOCAL\\cc-msg-2084","name":"claudehud-a4","nameSource":"derived","nameSince":1790301420182,"updatedAt":1790302083994,"status":"busy","statusUpdatedAt":1790302083994}"#;

    pub struct FakeProbe {
        pub procs: HashMap<u32, u64>,
        pub domain: String,
    }
    impl ProcessProbe for FakeProbe {
        fn creation_filetime(&self, pid: u32) -> Option<u64> {
            self.procs.get(&pid).copied()
        }
        fn pid_domain(&self) -> String {
            self.domain.clone()
        }
    }
    fn probe(pid: u32, created: u64) -> FakeProbe {
        FakeProbe {
            procs: HashMap::from([(pid, created)]),
            domain: "win32:LAPTOP-6N3ETS5A".into(),
        }
    }

    #[test]
    fn parses_live_sample() {
        let e = parse_entry("22488.json", SAMPLE).unwrap();
        assert_eq!(e.pid, 22488);
        assert_eq!(e.proc_start, Some(134_347_750_190_770_188));
        assert_eq!(e.session_id, "34bea346-2ede-4248-8364-2c33033971a7");
        assert_eq!(e.name, "claudehud-a4");
        assert_eq!(e.cwd, r"C:\Projects\Personal\ClaudeHUD");
        assert_eq!(e.status, SessionStatus::Busy);
        assert_eq!(e.started_at_ms, 1_790_301_420_182);
        assert_eq!(e.status_updated_at_ms, 1_790_302_083_994);
        assert_eq!(e.key(), "22488:134347750190770188");
        let s = e.to_session();
        assert_eq!(
            (s.pid, s.name.as_str(), s.status),
            (22488, "claudehud-a4", SessionStatus::Busy)
        );
    }

    #[test]
    fn parses_entry_with_bom() {
        let with_bom = format!("\u{feff}{SAMPLE}");
        assert_eq!(parse_entry("a.json", &with_bom).unwrap().pid, 22488);
    }

    #[test]
    fn waiting_status_and_reason() {
        let e = parse_entry("1.json", r#"{"pid":1,"sessionId":"s","status":"waiting","waitingFor":"approve the permission prompt"}"#).unwrap();
        assert_eq!(e.status, SessionStatus::Waiting);
        assert_eq!(
            e.waiting_for.as_deref(),
            Some("approve the permission prompt")
        );
    }

    #[test]
    fn tolerant_of_missing_and_odd_fields() {
        let e = parse_entry("1.json", r#"{"pid":1,"sessionId":"abcdef123456","cwd":"C:\\work\\portal-service","procStart":134347750190770188}"#).unwrap();
        assert_eq!(e.name, "portal-service", "name falls back to the folder");
        assert_eq!(
            e.status,
            SessionStatus::Unknown,
            "missing status is unknown"
        );
        assert_eq!(
            e.proc_start,
            Some(134_347_750_190_770_188),
            "numeric procStart accepted"
        );
        let e = parse_entry("1.json", r#"{"pid":1,"sessionId":"abcdef123456"}"#).unwrap();
        assert_eq!(
            e.name, "abcdef12",
            "no cwd: first 8 chars of the session id"
        );
    }

    #[test]
    fn rejects_unusable_entries() {
        assert!(
            parse_entry("x.json", r#"{"sessionId":"s"}"#).is_err(),
            "no pid"
        );
        assert!(
            parse_entry("x.json", r#"{"pid":1}"#).is_err(),
            "no session id"
        );
        assert!(
            parse_entry("x.json", r#"{"pid":1,"sessionId":"s""#).is_err(),
            "torn JSON"
        );
        assert!(parse_entry("x.json", "").is_err());
    }

    #[test]
    fn liveness_rules() {
        let e = parse_entry("22488.json", SAMPLE).unwrap();
        let ps = 134_347_750_190_770_188u64;
        assert_eq!(liveness(&e, &probe(22488, ps)), Liveness::Alive);
        assert_eq!(
            liveness(&e, &probe(22488, ps + 5_000_000)),
            Liveness::Alive,
            "within 1 s"
        );
        assert_eq!(
            liveness(&e, &probe(22488, ps + 20_000_000)),
            Liveness::Dead,
            "pid recycled"
        );
        assert_eq!(
            liveness(&e, &probe(1, ps)),
            Liveness::Dead,
            "no such process"
        );
        let mut other = probe(22488, ps);
        other.domain = "win32:other-pc".into();
        assert_eq!(liveness(&e, &other), Liveness::Foreign);
        let mut no_start = e.clone();
        no_start.proc_start = None;
        assert_eq!(liveness(&no_start, &probe(22488, 42)), Liveness::Alive);
    }
}
