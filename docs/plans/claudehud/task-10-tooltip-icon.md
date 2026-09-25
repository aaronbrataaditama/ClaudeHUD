# Task 10: Tray tooltip text, tray-icon pixels, strip pixels

**Goal:** Pure functions that produce (a) the two-line tray tooltip (≤ 127 UTF-16 units) from a `Light` + `Snapshot`, (b) the tray icon as premultiplied BGRA pixels (the pixel creature plus a status badge, §5.1), and (c) the strip's pill-shaped pixels. The platform layer only copies these bytes into Windows objects.

**Spec:** §5 (tooltip examples), §5.1 (icon: pixel map, badge geometry, colours). Mockup `claudehud-mockup.html` §5 shows every state's icon and tooltip.

**Files:**
- Create: `src/tooltip.rs`, `src/icon.rs`
- Modify: `src/lib.rs` (add `pub mod tooltip; pub mod icon;`)

**Interfaces:**
- Consumes: `model::*`, `format::*`, `timefmt::LocalTime`.
- Produces:
  - `tooltip::tooltip(light: &Light, snap: &Snapshot, local: &dyn Fn(i64) -> LocalTime) -> String` (lines joined by `\n`), `tooltip::MAX_UTF16 = 127`
  - `icon::CREATURE: [&str; 7]`, `icon::{BODY, BODY_OFF, EYE}: u32`
  - `icon::Badge { rgb: u32, alpha: f32 }`
  - `icon::render_tray_icon(size: u32, badge: Option<Badge>, off: bool) -> Vec<u8>` (`size*size*4` bytes, BGRA premultiplied, top-down rows)
  - `icon::render_strip(w: u32, h: u32, rgb: u32, alpha: f32) -> Vec<u8>` (same format, rounded ends)

---

## Part A: tooltip

- [x] **Step 1: Write failing tests**

`src/tooltip.rs`, tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::timefmt::{parse_iso8601, utc_parts};

    fn now_ms() -> i64 {
        parse_iso8601("2026-09-25T10:00:00Z").unwrap() * 1000
    }
    fn busy(name: &str) -> Session {
        Session { name: name.into(), session_id: name.into(), status: SessionStatus::Busy, ..Default::default() }
    }
    fn usage(p5: f64, p7: f64) -> Collected<Usage> {
        Collected::healthy(Usage {
            limits: vec![
                Limit { key: "five_hour".into(), label: "Current session · 5h".into(), pct: p5, resets_at: None },
                Limit { key: "seven_day".into(), label: "Weekly · all models".into(), pct: p7, resets_at: parse_iso8601("2026-09-28T09:00:00Z") },
            ],
            spend: None,
        })
    }
    fn tip(reason: Reason, snap: &Snapshot) -> String {
        tooltip(&Light { colour: Colour::Green, dim: false, reason }, snap, &utc_parts)
    }

    #[test]
    fn waiting_with_counts_and_usage() {
        let snap = Snapshot { now_ms: now_ms(), sessions: vec![busy("a"), busy("b"), busy("portal-service")], usage: usage(61.0, 88.0), ..Default::default() };
        let t = tip(Reason::Waiting { name: "portal-service".into(), waiting_for: "approve the permission prompt".into() }, &snap);
        assert_eq!(t, "portal-service: approve the permission prompt\n3 running · 5h 61% · 7d 88%");
    }

    #[test]
    fn quota_lines() {
        let snap = Snapshot { now_ms: now_ms(), sessions: vec![busy("a"), busy("b")], usage: usage(61.0, 88.0), ..Default::default() };
        let r = Reason::QuotaWarn { key: "seven_day".into(), label: "Weekly · all models".into(), pct: 88.0, resets_at: parse_iso8601("2026-09-28T09:00:00Z") };
        assert_eq!(tip(r, &snap), "Weekly limit at 88% · resets Mon 09:00\n2 running · 5h 61% · 7d 88%");
        let idle = Session { status: SessionStatus::Idle, ..busy("a") };
        let snap = Snapshot { sessions: vec![idle], usage: usage(12.0, 100.0), ..snap };
        let r = Reason::QuotaSpent { key: "seven_day".into(), label: "Weekly · all models".into(), resets_at: parse_iso8601("2026-09-28T09:00:00Z") };
        assert_eq!(tip(r, &snap), "Weekly limit spent · resets Mon 09:00\n1 idle · 5h 12% · 7d 100%");
    }

    #[test]
    fn spend_off_crash_incident() {
        let snap = Snapshot { now_ms: now_ms(), sessions: vec![busy("billing-api")], ..Default::default() };
        let r = Reason::SpendWarn { used_minor: 54_600, limit_minor: 60_000, currency: "USD".into() };
        assert_eq!(tip(r, &snap), "Spend at 91% · $546 of $600 · resets 1 Oct\n1 running");
        let off = Snapshot { now_ms: now_ms(), usage: usage(23.0, 47.0), ..Default::default() };
        assert_eq!(tip(Reason::NoSessions, &off), "ClaudeHUD · no Claude sessions\n5h 23% · 7d 47%");
        assert_eq!(tip(Reason::Crashed { name: "portal-service".into() }, &snap), "portal-service crashed mid-turn · click to acknowledge\n1 running");
        let r = Reason::Incident { component: "Claude API (api.anthropic.com)".into(), state: ComponentState::PartialOutage };
        assert_eq!(tip(r, &snap), "Claude API: partial outage\n1 running");
    }

    #[test]
    fn degraded_usage_is_named_not_coloured() {
        let snap = Snapshot {
            now_ms: now_ms(),
            sessions: vec![busy("a")],
            usage: Collected { health: Health::Degraded("usage unavailable · rate limited".into()), value: None },
            ..Default::default()
        };
        assert_eq!(tip(Reason::Working { name: "a".into(), count: 1 }, &snap), "a working\n1 running · usage unavailable · rate limited");
    }

    #[test]
    fn long_names_are_shortened_and_total_fits() {
        let long = "x".repeat(200);
        let snap = Snapshot { now_ms: now_ms(), sessions: vec![busy(&long)], usage: usage(61.0, 88.0), ..Default::default() };
        let t = tip(Reason::Waiting { name: long.clone(), waiting_for: "approve the permission prompt".into() }, &snap);
        assert!(t.encode_utf16().count() <= MAX_UTF16, "{} units", t.encode_utf16().count());
        let first = t.lines().next().unwrap();
        assert!(first.ends_with(": approve the permission prompt"), "reason kept, name cut: {first}");
        assert!(first.contains('…'));
    }
}
```

Add `pub mod tooltip;` to `src/lib.rs`.

- [x] **Step 2: Run to verify failure**

Run: `cargo test --lib tooltip::`
Expected: compile errors.

- [x] **Step 3: Implement**

Above the tests in `src/tooltip.rs`:

```rust
//! Tray tooltip: line one says why the light has its colour, line two gives
//! counts and usage (§5). Windows truncates at 127 UTF-16 units + NUL.

use crate::format::{
    component_state_text, limit_noun, money, pct_label, reset_label, short_component, short_limit,
    spend_reset_label, truncate_chars,
};
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
    let reset = |t: &Option<i64>| t.map(|t| format!(" · {}", reset_label(t, now_s, local))).unwrap_or_default();
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
        Reason::QuotaWarn { key, label, pct, resets_at } => {
            format!("{} at {}{}", limit_noun(key, label), pct_label(*pct), reset(resets_at))
        }
        Reason::QuotaSpent { key, label, resets_at } => format!("{} spent{}", limit_noun(key, label), reset(resets_at)),
        Reason::SpendWarn { used_minor, limit_minor, currency } => format!(
            "Spend at {} · {} of {} · {}",
            spend_pct(*used_minor, *limit_minor),
            money(*used_minor, currency),
            money(*limit_minor, currency),
            spend_reset_label(&local(now_s))
        ),
        Reason::SpendSpent { used_minor, limit_minor, currency } => format!(
            "Spend limit reached · {} of {}",
            money(*used_minor, currency),
            money(*limit_minor, currency)
        ),
        Reason::ApiError { name } => with_name(name, |n| format!("{n}: API error, retries exhausted")),
        Reason::Crashed { name } => with_name(name, |n| format!("{n} crashed mid-turn · click to acknowledge")),
        Reason::Incident { component, state } => {
            format!("{}: {}", short_component(component), component_state_text(*state))
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
    let mut out = if l2.is_empty() { l1 } else { format!("{l1}\n{l2}") };
    while out.encode_utf16().count() > MAX_UTF16 {
        out.pop();
    }
    out
}
```

- [x] **Step 4: Run tests**

Run: `cargo test --lib tooltip::`
Expected: 5 passed.

## Part B: icon and strip pixels

- [x] **Step 5: Write failing tests**

`src/icon.rs`, tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn px(buf: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }
    fn bgra(rgb: u32) -> [u8; 4] {
        [(rgb & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, ((rgb >> 16) & 0xFF) as u8, 255]
    }
    const GREEN: u32 = 0x4FBE86;

    #[test]
    fn sizes() {
        for s in [16, 20, 24, 32] {
            assert_eq!(render_tray_icon(s, None, false).len(), (s * s * 4) as usize);
        }
        assert_eq!(render_strip(132, 4, GREEN, 1.0).len(), 132 * 4 * 4);
    }

    #[test]
    fn creature_at_16px() {
        let b = render_tray_icon(16, None, false);
        assert_eq!(px(&b, 16, 0, 0)[3], 0, "corner transparent");
        assert_eq!(px(&b, 16, 4, 4), bgra(EYE), "left eye at grid (3,1) + origin (1,3)");
        assert_eq!(px(&b, 16, 9, 4), bgra(EYE), "right eye at grid (8,1)");
        assert_eq!(px(&b, 16, 1, 5), bgra(BODY), "arm at grid (0,2)");
        assert_eq!(px(&b, 16, 12, 12)[3], 0, "no badge: bottom-right empty");
    }

    #[test]
    fn creature_scales_with_nearest_neighbour() {
        let b = render_tray_icon(32, None, false);
        assert_eq!(px(&b, 32, 8, 8), bgra(EYE));
        assert_eq!(px(&b, 32, 9, 9), bgra(EYE));
    }

    #[test]
    fn off_is_grey_without_badge() {
        let b = render_tray_icon(16, None, true);
        assert_eq!(px(&b, 16, 1, 5), bgra(BODY_OFF));
    }

    #[test]
    fn badge_is_drawn_and_cuts_out_the_body() {
        let plain = render_tray_icon(16, None, false);
        let b = render_tray_icon(16, Some(Badge { rgb: GREEN, alpha: 1.0 }), false);
        assert_eq!(px(&b, 16, 12, 12), bgra(GREEN), "badge centre");
        assert_eq!(px(&plain, 16, 10, 9)[3], 255, "leg pixel exists without badge");
        assert!(px(&b, 16, 10, 9)[3] < 128, "leg pixel inside the cut-out ring is mostly cleared");
    }

    #[test]
    fn dim_badge_is_translucent() {
        let b = render_tray_icon(16, Some(Badge { rgb: GREEN, alpha: 0.55 }), false);
        let a = px(&b, 16, 12, 12)[3];
        assert!((135..=145).contains(&a), "alpha {a}");
    }

    #[test]
    fn strip_is_a_pill() {
        let s = render_strip(132, 4, GREEN, 1.0);
        assert_eq!(px(&s, 132, 66, 2), bgra(GREEN));
        assert!(px(&s, 132, 0, 0)[3] < px(&s, 132, 66, 0)[3], "rounded ends");
        let half = render_strip(132, 4, GREEN, 0.55);
        assert!((135..=145).contains(&px(&half, 132, 66, 2)[3]));
    }
}
```

Add `pub mod icon;` to `src/lib.rs`.

- [x] **Step 6: Run to verify failure**

Run: `cargo test --lib icon::`
Expected: compile errors.

- [x] **Step 7: Implement**

Above the tests in `src/icon.rs`:

```rust
//! Pixels for the tray icon (pixel creature + status badge, §5.1) and the strip.
//! Output: premultiplied BGRA, top-down, 4 bytes per pixel, ready for a 32-bpp DIB.

/// 12 × 7 creature. `#` body, `o` eye, `.` empty. Drawn at (1, 3) on a 16-unit grid.
pub const CREATURE: [&str; 7] = [
    "..########..",
    "..#o####o#..",
    "############",
    "############",
    "..########..",
    "..#.#..#.#..",
    "..#.#..#.#..",
];
pub const BODY: u32 = 0xDE7356;
pub const BODY_OFF: u32 = 0x8A8D94;
pub const EYE: u32 = 0x141414;

const ORIGIN_X: i32 = 1;
const ORIGIN_Y: i32 = 3;
const BADGE_CX: f32 = 12.6;
const BADGE_CY: f32 = 12.4;
const BADGE_R: f32 = 3.4;
const BADGE_RING: f32 = 1.1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Badge {
    pub rgb: u32,
    /// 1.0, or DIM_ALPHA when every session is idle
    pub alpha: f32,
}

fn channels(rgb: u32) -> [f32; 3] {
    [((rgb >> 16) & 0xFF) as f32, ((rgb >> 8) & 0xFF) as f32, (rgb & 0xFF) as f32]
}

/// Straight-alpha "over" composite. Returns ([r,g,b], a).
fn over(under: Option<([f32; 3], f32)>, top: [f32; 3], a: f32) -> ([f32; 3], f32) {
    let (uc, ua) = under.unwrap_or(([0.0; 3], 0.0));
    let out_a = a + ua * (1.0 - a);
    if out_a <= 0.0 {
        return ([0.0; 3], 0.0);
    }
    let c = [0, 1, 2].map(|i| (top[i] * a + uc[i] * ua * (1.0 - a)) / out_a);
    (c, out_a)
}

fn write(buf: &mut [u8], i: usize, c: [f32; 3], a: f32) {
    let a = a.clamp(0.0, 1.0);
    let q = |v: f32| (v * a).round().clamp(0.0, 255.0) as u8;
    buf[i] = q(c[2]);
    buf[i + 1] = q(c[1]);
    buf[i + 2] = q(c[0]);
    buf[i + 3] = (a * 255.0).round() as u8;
}

pub fn render_tray_icon(size: u32, badge: Option<Badge>, off: bool) -> Vec<u8> {
    let n = size as usize;
    let mut buf = vec![0u8; n * n * 4];
    let s = size as f32 / 16.0;
    let body = channels(if off { BODY_OFF } else { BODY });
    let eye = channels(EYE);
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let gx = (fx / s).floor() as i32 - ORIGIN_X;
            let gy = (fy / s).floor() as i32 - ORIGIN_Y;
            let mut px: Option<([f32; 3], f32)> = None;
            if (0..7).contains(&gy) && (0..12).contains(&gx) {
                match CREATURE[gy as usize].as_bytes()[gx as usize] {
                    b'#' => px = Some((body, 1.0)),
                    b'o' => px = Some((eye, 1.0)),
                    _ => {}
                }
            }
            if let Some(b) = badge {
                let d = ((fx - BADGE_CX * s).powi(2) + (fy - BADGE_CY * s).powi(2)).sqrt();
                // clear the body inside the ring so the badge reads on any taskbar colour
                let cut = ((BADGE_R + BADGE_RING) * s + 0.5 - d).clamp(0.0, 1.0);
                if let Some((c, a)) = px {
                    px = Some((c, a * (1.0 - cut)));
                }
                let cov = (BADGE_R * s + 0.5 - d).clamp(0.0, 1.0) * b.alpha;
                if cov > 0.0 {
                    px = Some(over(px, channels(b.rgb), cov));
                }
            }
            if let Some((c, a)) = px {
                write(&mut buf, (y * n + x) * 4, c, a);
            }
        }
    }
    buf
}

/// Coverage of pixel centre (px, py) by a w×h rectangle with corner radius r.
fn rounded_cov(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    let qx = (px - w / 2.0).abs() - (w / 2.0 - r);
    let qy = (py - h / 2.0).abs() - (h / 2.0 - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
    (0.5 - outside).clamp(0.0, 1.0)
}

/// The light itself: a pill of the given colour. `alpha` is DIM_ALPHA when idle.
pub fn render_strip(w: u32, h: u32, rgb: u32, alpha: f32) -> Vec<u8> {
    let (wu, hu) = (w as usize, h as usize);
    let mut buf = vec![0u8; wu * hu * 4];
    let r = (w.min(h) as f32) / 2.0;
    let c = channels(rgb);
    for y in 0..hu {
        for x in 0..wu {
            let cov = rounded_cov(x as f32 + 0.5, y as f32 + 0.5, w as f32, h as f32, r);
            if cov > 0.0 {
                write(&mut buf, (y * wu + x) * 4, c, cov * alpha);
            }
        }
    }
    buf
}
```

- [x] **Step 8: Run tests**

Run: `cargo test --lib icon::`
Expected: 7 passed. If `badge_is_drawn_and_cuts_out_the_body` fails on pixel (10, 9), print that pixel's alpha and check the maths against §5.1 (badge radius 3.4/16, ring 1.1/16), not the test.

- [x] **Step 9: Lint and commit**

```powershell
cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/lib.rs src/tooltip.rs src/icon.rs
git commit -m "feat(tray): tooltip text and runtime icon/strip pixels"
```
