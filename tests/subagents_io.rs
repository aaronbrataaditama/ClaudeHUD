mod common;

use claudehud::collectors::subagents::list_subagents;
use claudehud::model::AgentState;
use claudehud::timefmt::now_ms;
use std::time::{Duration, SystemTime};

fn assistant(ts: &str, stop: &str) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{ts}","message":{{"stop_reason":"{stop}","content":[],"usage":{{"input_tokens":1,"cache_read_input_tokens":9,"cache_creation_input_tokens":0}}}}}}"#
    )
}

#[test]
fn lists_recent_agents_running_first_and_skips_old_ones() {
    let t = common::TempDir::new("subagents");
    let sd = t.path().join("sess");
    t.write(
        "sess/subagents/agent-aaa.meta.json",
        r#"{"agentType":"Explore","description":"Map endpoints","spawnDepth":1}"#,
    );
    t.write(
        "sess/subagents/agent-aaa.jsonl",
        &assistant("2026-09-25T10:00:00Z", "tool_use"),
    );
    t.write(
        "sess/subagents/agent-bbb.meta.json",
        r#"{"agentType":"general-purpose","description":"Write tests","spawnDepth":2}"#,
    );
    t.write(
        "sess/subagents/agent-bbb.jsonl",
        &format!(
            "{}\n{}",
            assistant("2026-09-25T10:01:00Z", "tool_use"),
            assistant("2026-09-25T10:02:00Z", "end_turn")
        ),
    );
    let old = t.write(
        "sess/subagents/agent-ccc.jsonl",
        &assistant("2026-09-25T08:00:00Z", "tool_use"),
    );
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    t.write("sess/subagents/notes.txt", "ignored");

    let list = list_subagents(&sd, now_ms());
    let ids: Vec<&str> = list.iter().map(|a| a.agent_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["aaa", "bbb"],
        "running first; ccc is older than 15 min"
    );
    assert_eq!(list[0].state, AgentState::Running);
    assert_eq!(list[0].agent_type, "Explore");
    assert_eq!(list[1].state, AgentState::Done);
    assert_eq!(list[1].depth, 2);
    assert_eq!(list[1].context_tokens, Some(10));
    assert!(list[1].ended_ms.is_some());
}

#[test]
fn missing_folder_means_no_agents() {
    let t = common::TempDir::new("subagents-none");
    assert!(list_subagents(&t.path().join("nope"), now_ms()).is_empty());
}
