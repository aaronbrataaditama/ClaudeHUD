mod common;

use claudehud::collectors::registry::{scan_dir, ProcessProbe};
use std::collections::HashMap;

struct Probe(HashMap<u32, u64>);
impl ProcessProbe for Probe {
    fn creation_filetime(&self, pid: u32) -> Option<u64> {
        self.0.get(&pid).copied()
    }
    fn pid_domain(&self) -> String {
        "win32:test-pc".into()
    }
}

fn entry(pid: u32, status: &str) -> String {
    format!(
        r#"{{"pid":{pid},"sessionId":"sess-{pid}","cwd":"C:\\w\\p{pid}","procStart":"{}","pidDomain":"win32:test-pc","status":"{status}"}}"#,
        1000 + pid as u64
    )
}

#[test]
fn missing_dir_is_empty_not_error() {
    let t = common::TempDir::new("reg-missing");
    let scan = scan_dir(&t.path().join("sessions"), &Probe(HashMap::new())).unwrap();
    assert!(scan.live.is_empty() && scan.dead.is_empty() && scan.unreadable.is_empty());
}

#[test]
fn classifies_live_dead_unreadable_and_ignores_keys() {
    let t = common::TempDir::new("reg-mixed");
    t.write("sessions/10.json", &entry(10, "busy"));
    t.write("sessions/20.json", &entry(20, "idle"));
    t.write("sessions/30.json", r#"{"pid":30,"sessionId":"x""#); // torn
    t.write("sessions/10.4363ad5d.key", "SECRET-DO-NOT-READ");
    t.write("sessions/notes.txt", "hello");
    t.write(
        "sessions/40.json",
        r#"{"pid":40,"sessionId":"remote","procStart":"1040","pidDomain":"win32:other-pc","status":"busy"}"#,
    );
    let probe = Probe(HashMap::from([(10, 1010), (40, 1040)])); // 20 is gone
    let scan = scan_dir(&t.path().join("sessions"), &probe).unwrap();
    assert_eq!(
        scan.live.iter().map(|e| e.pid).collect::<Vec<_>>(),
        vec![10]
    );
    assert_eq!(
        scan.dead.iter().map(|e| e.pid).collect::<Vec<_>>(),
        vec![20]
    );
    assert_eq!(scan.unreadable, vec!["30.json".to_string()]);
    // pid 40 belongs to another machine: in no list at all
    assert!(scan
        .live
        .iter()
        .chain(scan.dead.iter())
        .all(|e| e.pid != 40));
}
