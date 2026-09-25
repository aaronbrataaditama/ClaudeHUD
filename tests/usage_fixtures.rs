use claudehud::collectors::usage::parse_usage;

#[test]
fn every_saved_live_response_parses() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/usage");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        let u = parse_usage(&text).unwrap_or_else(|err| panic!("{}: {err}", p.display()));
        assert!(
            !u.limits.is_empty() || u.spend.is_some(),
            "{}: no limits and no spend",
            p.display()
        );
    }
}

/// This account's `limits` is empty and every named window is null (spend-only
/// enterprise account, docs/spike-results.md). Its response also has a `spend`
/// object which must be preferred over `extra_usage`, even though here they carry
/// the same figures; this proves the `spend`-object path fired, not just the
/// `extra_usage` fallback.
#[test]
fn live_20260925_is_spend_only_via_the_spend_object() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/usage/live-20260925.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let u = parse_usage(&text).unwrap_or_else(|err| panic!("{}: {err}", path.display()));

    assert!(
        u.limits.is_empty(),
        "expected no limits, got {:?}",
        u.limits
    );

    let s = u.spend.expect("expected Some(Spend)");
    assert_eq!(s.used_minor, 9948);
    assert_eq!(s.limit_minor, Some(60_000));
    assert_eq!(s.currency, "USD");
    assert!(s.enabled);
}
