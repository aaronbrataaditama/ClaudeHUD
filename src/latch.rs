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
                self.latched.push(CrashedSession {
                    key,
                    name: e.name.clone(),
                    cwd: e.cwd.clone(),
                });
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
        assert!(
            l.crashed().is_empty(),
            "stale file from before ClaudeHUD started"
        );
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
        let live = [
            entry(1, 1, SessionStatus::Waiting),
            entry(2, 2, SessionStatus::Shell),
            entry(3, 3, SessionStatus::Idle),
        ];
        l.update(&live, &[]);
        l.update(
            &[],
            &[
                entry(1, 1, SessionStatus::Waiting),
                entry(2, 2, SessionStatus::Shell),
                entry(3, 3, SessionStatus::Idle),
            ],
        );
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
        l.update(
            &[entry(7, 900, SessionStatus::Idle)],
            &[entry(7, 100, SessionStatus::Busy)],
        );
        assert_eq!(l.crashed().len(), 1);
    }
}
