mod common;

use claudehud::collectors::tail::{read_tail, TAIL_BYTES};
use claudehud::collectors::transcript::{find_transcript, parse_tail};

const LINE: &str = r#"{"type":"assistant","message":{"model":"claude-opus-5","content":[],"usage":{"input_tokens":1,"cache_read_input_tokens":99,"cache_creation_input_tokens":0}}}"#;

#[test]
fn reads_only_the_tail_of_a_large_file() {
    let t = common::TempDir::new("tail-large");
    let filler = format!(
        "{{\"type\":\"attachment\",\"pad\":\"{}\"}}\n",
        "x".repeat(1000)
    );
    let mut body = filler.repeat(200); // ~200 KB
    body.push_str(LINE);
    body.push('\n');
    let p = t.write("big.jsonl", &body);
    let (text, truncated) = read_tail(&p, TAIL_BYTES).unwrap();
    assert!(truncated);
    assert!(text.len() as u64 <= TAIL_BYTES);
    assert_eq!(
        parse_tail(&text, truncated).last_turn_input_tokens,
        Some(100)
    );
}

#[test]
fn small_file_is_not_truncated() {
    let t = common::TempDir::new("tail-small");
    let p = t.write("s.jsonl", LINE);
    let (text, truncated) = read_tail(&p, TAIL_BYTES).unwrap();
    assert!(!truncated);
    assert_eq!(
        parse_tail(&text, truncated).model.as_deref(),
        Some("claude-opus-5")
    );
}

#[test]
fn finds_transcript_by_slug_then_by_scan() {
    let t = common::TempDir::new("find");
    let projects = t.path().join("projects");
    let direct = t.write("projects/C--work-app/s1.jsonl", LINE);
    assert_eq!(
        find_transcript(&projects, r"C:\work\app", "s1"),
        Some(direct)
    );
    // stored under a lower-case drive slug: found by scanning
    let other = t.write("projects/c--other-thing/s2.jsonl", LINE);
    assert_eq!(
        find_transcript(&projects, r"D:\elsewhere", "s2"),
        Some(other)
    );
    assert_eq!(find_transcript(&projects, r"C:\work\app", "missing"), None);
}
