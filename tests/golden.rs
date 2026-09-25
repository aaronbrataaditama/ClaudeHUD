use claudehud::fixture::parse_snapshot;
use claudehud::model::Reason;
use claudehud::state::fold;
use serde_json::Value;

fn detail(r: &Reason) -> Option<String> {
    match r {
        Reason::Working { name, .. }
        | Reason::Waiting { name, .. }
        | Reason::ApiError { name }
        | Reason::Crashed { name } => Some(name.clone()),
        Reason::QuotaWarn { label, .. } | Reason::QuotaSpent { label, .. } => Some(label.clone()),
        Reason::Incident { component, .. } => Some(component.clone()),
        _ => None,
    }
}

#[test]
fn golden_snapshots() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/snapshots");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    let mut failures = Vec::new();
    let mut count = 0;
    for p in files
        .iter()
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
    {
        count += 1;
        let text = std::fs::read_to_string(p).unwrap();
        let v: Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
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
            problems.push(format!(
                "reason {} != {}",
                light.reason.kind(),
                expect["reason"]
            ));
        }
        if let Some(d) = expect.get("detail").and_then(Value::as_str) {
            if detail(&light.reason).as_deref() != Some(d) {
                problems.push(format!("detail {:?} != {d}", detail(&light.reason)));
            }
        }
        if !problems.is_empty() {
            failures.push(format!(
                "{}: {}",
                p.file_name().unwrap().to_string_lossy(),
                problems.join("; ")
            ));
        }
    }
    assert!(
        count >= 21,
        "expected at least 21 golden files, found {count}"
    );
    assert!(
        failures.is_empty(),
        "golden failures:\n{}",
        failures.join("\n")
    );
}
