# Task 12: Pure panel layout

**Goal:** Turn a `Snapshot` + `Light` + view state into a flat list of positioned draw ops and hit regions for the hover panel, in logical px. The Direct2D renderer (Task 18) only draws these ops; every layout decision is made and tested here.

**Spec:** §4 (panel content, top to bottom; read it fully), §2.2 (width 360, height cap, scrolling list with fixed header/usage/footer). Visual reference: `claudehud-mockup.html` sections 1–3 (open in a browser; match its arrangement, not its CSS).

Layout, top to bottom (y in logical px from the panel content's top):
1. **Header** (0–60): app icon 30×30 at (16,14); plan label (Title font) at x=56, y=12; spend after it ("· $50 of $600 spent"); summary line at y=34 with a status dot; pin glyph 24×24 at (320,14).
2. **Banner** (only for `QuotaSpent`/`SpendSpent`): 34 px tall red-tinted box, then 10 px gap.
3. **Usage**: divider, caption "USAGE" (or "SPEND" when there are no windows but there is spend), one 40 px row per limit (label left, reset right, meter, % right), a spend row if present.
4. **Sessions**: divider, caption "SESSIONS", then a **scrolling, clipped** list: crashed rows, then sessions in `ordered_sessions` order (44 px each) with expanded sub-agent rows (22 px each), a "+N idle" row past 12 sessions, or the empty state.
5. **Footer** (74 px): divider, overall status line with dot + "status.claude.com ↗" link, per-component dots, health line ("checked 2m ago · usage unavailable · rate limited").

The list viewport is `max_h − list_top − 74`, so the header, usage and footer never scroll out of view.

**Files:**
- Create: `src/panel/mod.rs`, `src/panel/layout.rs`
- Modify: `src/lib.rs` (add `pub mod panel;`)

**Interfaces:**
- Consumes: `model::*` (incl. `DIM_ALPHA`, `NOT_COLLECTED`), `state::{ordered_sessions, severity_colour, fold}` (fold only in tests), `format::*`, `timefmt::{LocalTime, utc_parts, parse_iso8601}` (last two in tests).
- Produces (`panel::layout::…`):
  - `W: f32 = 360.0`, `RADIUS: f32 = 12.0`, `MAX_ROWS: usize = 12`
  - `RectF { x, y, w, h: f32 }` with `new`, `right`, `bottom`, `contains(f32,f32)`, `shifted(dy)`, `intersect(&RectF) -> Option<RectF>`
  - `Font { Title, Body, BodyStrong, Small, SmallStrong, Caption, Mono }`, `Ink { Primary, Secondary, Muted }`, `Align { Left, Right }`
  - `TextOp { rect, text, font, ink, align }` (single line; renderer trims with an ellipsis and centres vertically)
  - `Op` variants: `Background{rect,radius}`, `Divider{y}`, `Highlight{rect}`, `Text(TextOp)`, `Dot{cx,cy,r,rgb,alpha}`, `Meter{rect,frac,rgb}`, `AppIcon{rect}`, `Pin{rect,on}`, `Chevron{cx,cy,open}`, `Banner{rect}`, `ClipPush(RectF)`, `ClipPop`, `Tooltip{rect,title,body}`
  - `Hit { Pin, Session(String), SessionName(String), StatusLink }` (the String is `session_id`, or the crash `key` for crashed rows), `HitRegion { rect, hit }`
  - `ViewState { expanded: HashMap<String,bool>, hovered: Option<Hit>, scroll: f32, pinned: bool }`
  - `trait Measure { fn width(&self, text: &str, font: Font) -> f32; }`
  - `Ctx<'a> { snap, light, view, max_h: f32, measure: &dyn Measure, local: &dyn Fn(i64)->LocalTime }`
  - `Layout { ops: Vec<Op>, hits: Vec<HitRegion>, height: f32, scroll_max: f32 }` with `hit_at(&self, x, y) -> Option<&Hit>` (last-pushed region wins, so a name beats its row)
  - `layout(&Ctx) -> Layout`
  - `is_expanded(&ViewState, &Session) -> bool` (used by the click handler in Task 18 to toggle the right way)

---

- [ ] **Step 1: Module file**

`src/panel/mod.rs`:

```rust
//! The hover panel. `layout` is pure; drawing lives in `platform::render`.

pub mod layout;
```

Add `pub mod panel;` to `src/lib.rs`.

- [ ] **Step 2: Write the failing tests**

Create `src/panel/layout.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::fold;
    use crate::timefmt::{parse_iso8601, utc_parts};

    struct Mono;
    impl Measure for Mono {
        fn width(&self, t: &str, f: Font) -> f32 {
            t.chars().count() as f32 * if f == Font::Title { 8.0 } else { 7.0 }
        }
    }

    fn now() -> i64 {
        parse_iso8601("2026-09-25T10:00:00Z").unwrap() * 1000
    }
    fn sess(id: &str, status: SessionStatus) -> Session {
        Session {
            session_id: id.into(),
            name: id.into(),
            cwd: format!(r"C:\Projects\{id}"),
            status,
            started_at_ms: now() - 12 * 60_000,
            status_updated_at_ms: now() - 3 * 60_000,
            ..Default::default()
        }
    }
    fn usage(p5: f64, p7: f64) -> Collected<Usage> {
        Collected::healthy(Usage {
            limits: vec![
                Limit { key: "five_hour".into(), label: "Current session · 5h".into(), pct: p5, resets_at: parse_iso8601("2026-09-25T14:32:00Z") },
                Limit { key: "seven_day".into(), label: "Weekly · all models".into(), pct: p7, resets_at: parse_iso8601("2026-09-28T09:00:00Z") },
            ],
            spend: None,
        })
    }
    fn snap(sessions: Vec<Session>) -> Snapshot {
        Snapshot { now_ms: now(), plan: Some("Team · Max 5x".into()), sessions, usage: usage(61.0, 88.0), ..Default::default() }
    }
    fn run(s: &Snapshot, view: &ViewState, max_h: f32) -> Layout {
        let light = fold(s, None);
        layout(&Ctx { snap: s, light: &light, view, max_h, measure: &Mono, local: &utc_parts })
    }
    fn texts(l: &Layout) -> Vec<String> {
        l.ops.iter().filter_map(|o| if let Op::Text(t) = o { Some(t.text.clone()) } else { None }).collect()
    }
    fn text_op<'a>(l: &'a Layout, s: &str) -> &'a TextOp {
        l.ops
            .iter()
            .find_map(|o| match o {
                Op::Text(t) if t.text == s => Some(t),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no text {s:?} in {:?}", texts(l)))
    }
    fn waiting(id: &str) -> Session {
        Session { waiting_for: Some("approve the permission prompt".into()), ..sess(id, SessionStatus::Waiting) }
    }

    #[test]
    fn header_plan_spend_and_summary() {
        let mut s = snap(vec![sess("a", SessionStatus::Busy), sess("b", SessionStatus::Busy), waiting("portal")]);
        s.plan = Some("Enterprise".into());
        s.usage = Collected::healthy(Usage { limits: vec![], spend: Some(Spend { used_minor: 5_000, limit_minor: Some(60_000), currency: "USD".into(), enabled: true }) });
        let l = run(&s, &ViewState::default(), 800.0);
        let t = texts(&l);
        assert!(t.contains(&"Enterprise".to_string()));
        assert!(t.contains(&"· $50 of $600 spent".to_string()));
        assert!(t.contains(&"3 sessions · 1 needs you".to_string()));
        assert!(t.contains(&"SPEND".to_string()), "no windows but spend: caption says SPEND");
        assert!(t.contains(&"$50 / $600".to_string()));
        assert!(matches!(l.ops[0], Op::Background { .. }), "background first");
    }

    #[test]
    fn usage_rows_with_reset_times_and_severity() {
        let l = run(&snap(vec![sess("a", SessionStatus::Busy)]), &ViewState::default(), 800.0);
        let t = texts(&l);
        for want in ["USAGE", "Current session · 5h", "resets 14:32", "61%", "Weekly · all models", "resets Mon 09:00", "88%"] {
            assert!(t.contains(&want.to_string()), "missing {want}: {t:?}");
        }
        let meters: Vec<(f32, u32)> = l.ops.iter().filter_map(|o| if let Op::Meter { frac, rgb, .. } = o { Some((*frac, *rgb)) } else { None }).collect();
        assert_eq!(meters.len(), 2);
        assert!((meters[0].0 - 0.61).abs() < 1e-6);
        assert_eq!(meters[0].1, Colour::Green.rgb());
        assert_eq!(meters[1].1, Colour::Amber.rgb());
    }

    #[test]
    fn sessions_in_order_with_state_and_detail() {
        let mut busy = sess("builder", SessionStatus::Busy);
        busy.transcript = Some(TranscriptFacts { model: Some("claude-opus-5".into()), last_turn_input_tokens: Some(132_600), ..Default::default() });
        let l = run(&snap(vec![sess("docs", SessionStatus::Idle), busy, waiting("portal")]), &ViewState::default(), 800.0);
        let t = texts(&l);
        let pos = |n: &str| t.iter().position(|x| x == n).unwrap();
        assert!(pos("portal") < pos("builder") && pos("builder") < pos("docs"));
        assert!(t.contains(&"Needs you".to_string()));
        assert!(t.contains(&"Working · 12m".to_string()));
        assert!(t.contains(&"Idle · 3m".to_string()));
        assert!(t.contains(&"Opus 5 · last turn 132.6k in".to_string()));
        assert!(t.iter().any(|x| x.starts_with("Approve the permission prompt")));
    }

    #[test]
    fn caps_at_twelve_rows() {
        let many: Vec<Session> = (0..15).map(|i| sess(&format!("s{i:02}"), SessionStatus::Idle)).collect();
        let l = run(&snap(many), &ViewState::default(), 2000.0);
        let names = l.ops.iter().filter(|o| matches!(o, Op::Text(t) if t.font == Font::BodyStrong && t.text.starts_with('s'))).count();
        assert_eq!(names, 12, "session names are BodyStrong; idle detail lines repeat the folder name in Small");
        assert!(texts(&l).contains(&"+3 idle".to_string()));
    }

    #[test]
    fn list_scrolls_while_header_usage_and_footer_stay_fixed() {
        let many: Vec<Session> = (0..20).map(|i| sess(&format!("s{i:02}"), SessionStatus::Busy)).collect();
        let s = snap(many);
        let top = run(&s, &ViewState::default(), 400.0);
        assert!(top.scroll_max > 0.0);
        assert!(top.height <= 400.0 + 0.01, "height {}", top.height);
        let scrolled = run(&s, &ViewState { scroll: 50.0, ..Default::default() }, 400.0);
        assert_eq!(text_op(&top, "Current session · 5h").rect.y, text_op(&scrolled, "Current session · 5h").rect.y);
        assert_eq!(text_op(&top, "status.claude.com ↗").rect.y, text_op(&scrolled, "status.claude.com ↗").rect.y);
        let first = ordered_sessions(&s.sessions)[0].name.clone();
        assert!((text_op(&top, &first).rect.y - text_op(&scrolled, &first).rect.y - 50.0).abs() < 1e-3);
        let over = run(&s, &ViewState { scroll: 99_999.0, ..Default::default() }, 400.0);
        assert!(over.ops.iter().any(|o| matches!(o, Op::ClipPush(_))));
        // rows scrolled out of the viewport are not clickable
        let r = text_op(&top, &first).rect;
        assert!(matches!(top.hit_at(r.x + 1.0, r.y + 1.0), Some(Hit::SessionName(_))));
        assert!(!matches!(scrolled.hit_at(r.x + 1.0, r.y + 1.0), Some(Hit::SessionName(n)) if *n == first));
    }

    #[test]
    fn name_tooltip_shows_the_folder_and_stays_inside() {
        let mut s = sess("deep", SessionStatus::Busy);
        s.cwd = r"C:\Projects\Personal\ClaudeHUD\very\deep\folder\structure\that\is\long".into();
        let view = ViewState { hovered: Some(Hit::SessionName("deep".into())), ..Default::default() };
        let l = run(&snap(vec![s]), &view, 800.0);
        match l.ops.last() {
            Some(Op::Tooltip { rect, title, body }) => {
                assert_eq!(title, "Working folder");
                assert!(body.contains('…') && body.ends_with("long"), "{body}");
                assert!(rect.right() <= W - 8.0 + 1e-3);
                assert!(rect.y > text_op(&l, "deep").rect.bottom());
            }
            other => panic!("tooltip must be the last op, got {other:?}"),
        }
    }

    #[test]
    fn sub_agents_expand_for_active_sessions_and_on_request() {
        let agent = Subagent { agent_id: "a1".into(), agent_type: "Explore".into(), description: "Map endpoints".into(), depth: 1, state: AgentState::Running, started_ms: Some(now() - 72_000), ended_ms: None, context_tokens: Some(18_400) };
        let mut busy = sess("busy", SessionStatus::Busy);
        busy.subagents = vec![agent.clone()];
        let mut idle = sess("idle", SessionStatus::Idle);
        idle.subagents = vec![Subagent { agent_id: "a2".into(), agent_type: "Plan".into(), state: AgentState::Done, ..agent }];
        let s = snap(vec![busy, idle]);
        let l = run(&s, &ViewState::default(), 800.0);
        let t = texts(&l);
        assert!(t.contains(&"Explore".to_string()) && t.contains(&"1m 12s · 18.4k".to_string()));
        assert!(!t.contains(&"Plan".to_string()), "idle session collapsed by default");
        assert!(t.iter().any(|x| x.ends_with("1 sub-agent (1 running)")));
        let view = ViewState { expanded: HashMap::from([("idle".to_string(), true)]), ..Default::default() };
        assert!(texts(&run(&s, &view, 800.0)).contains(&"Plan".to_string()));
        let chevrons = l.ops.iter().filter(|o| matches!(o, Op::Chevron { .. })).count();
        assert_eq!(chevrons, 2);
    }

    #[test]
    fn empty_state() {
        let mut s = snap(vec![]);
        s.plan = None;
        let t = texts(&run(&s, &ViewState::default(), 800.0));
        assert!(t.contains(&"No Claude sessions running".to_string()));
        assert!(t.contains(&"Nothing running".to_string()));
        assert!(t.contains(&"Claude".to_string()), "fallback plan label");
    }

    #[test]
    fn limit_reached_banner() {
        let mut s = snap(vec![sess("a", SessionStatus::Idle)]);
        s.usage = usage(12.0, 100.0);
        let l = run(&s, &ViewState::default(), 800.0);
        assert!(l.ops.iter().any(|o| matches!(o, Op::Banner { .. })));
        let t = texts(&l);
        assert!(t.contains(&"Weekly limit spent. New turns will fail until Mon 09:00.".to_string()), "{t:?}");
        assert!(t.contains(&"Weekly limit spent".to_string()), "summary line");
    }

    #[test]
    fn crashed_row_comes_first() {
        let mut s = snap(vec![sess("a", SessionStatus::Busy)]);
        s.crashed = vec![CrashedSession { key: "9:9".into(), name: "gone".into(), cwd: r"C:\w\gone".into() }];
        let t = texts(&run(&s, &ViewState::default(), 800.0));
        let pos = |n: &str| t.iter().position(|x| x == n).unwrap();
        assert!(pos("gone") < pos("a"));
        assert!(t.contains(&"Crashed mid-turn".to_string()));
    }

    #[test]
    fn footer_status_and_health() {
        let mut s = snap(vec![sess("a", SessionStatus::Busy)]);
        s.status = Collected::healthy(ServiceStatus {
            description: "All Systems Operational".into(),
            components: vec![
                Component { name: "Claude Code".into(), state: ComponentState::Operational },
                Component { name: "Claude API (api.anthropic.com)".into(), state: ComponentState::Operational },
                Component { name: "claude.ai".into(), state: ComponentState::Operational },
            ],
            checked_at_ms: now() - 120_000,
        });
        s.usage.health = Health::Degraded("usage unavailable · rate limited".into());
        let t = texts(&run(&s, &ViewState::default(), 800.0));
        assert!(t.contains(&"All systems operational".to_string()));
        assert!(t.contains(&"Claude API".to_string()) && t.contains(&"claude.ai".to_string()));
        assert!(t.contains(&"checked 2m ago · usage unavailable · rate limited".to_string()), "{t:?}");

        if let Some(st) = s.status.value.as_mut() {
            st.components[1].state = ComponentState::PartialOutage;
        }
        let t = texts(&run(&s, &ViewState::default(), 800.0));
        assert!(t.contains(&"Partial outage · Claude API".to_string()), "{t:?}");
        let none = snap(vec![]);
        assert!(texts(&run(&none, &ViewState::default(), 800.0)).contains(&"Service status unavailable".to_string()));
    }

    #[test]
    fn hits_pin_name_row_and_link() {
        let l = run(&snap(vec![sess("a", SessionStatus::Busy)]), &ViewState::default(), 800.0);
        assert_eq!(l.hit_at(332.0, 26.0), Some(&Hit::Pin));
        let name = text_op(&l, "a").rect;
        assert_eq!(l.hit_at(name.x + 2.0, name.y + 5.0), Some(&Hit::SessionName("a".into())));
        assert_eq!(l.hit_at(300.0, name.y + 5.0), Some(&Hit::Session("a".into())));
        let link = text_op(&l, "status.claude.com ↗").rect;
        assert_eq!(l.hit_at(link.x + 60.0, link.y + 8.0), Some(&Hit::StatusLink));
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --lib panel::`
Expected: compile errors.

- [ ] **Step 4: Implement**

Above the tests in `src/panel/layout.rs`:

```rust
//! Pure panel layout (§4): Snapshot → positioned draw ops + hit regions, in
//! logical px relative to the panel content's top-left corner.

use crate::format::{
    ago, component_state_text, elapsed, folder_name, limit_noun, middle_ellipsis, model_name, money, pct_label,
    reset_label, sentence_case, short_component, spend_reset_label, tokens, uptime,
};
use crate::model::{
    AgentState, Colour, ComponentState, Health, Light, Reason, Session, SessionStatus, Snapshot, Subagent, DIM_ALPHA,
    NOT_COLLECTED,
};
use crate::state::{ordered_sessions, severity_colour};
use crate::timefmt::LocalTime;
use std::collections::HashMap;

pub const W: f32 = 360.0;
pub const RADIUS: f32 = 12.0;
pub const MAX_ROWS: usize = 12;
const PAD: f32 = 16.0;
const HEADER_H: f32 = 60.0;
const ROW_H: f32 = 44.0;
const AGENT_H: f32 = 22.0;
const FOOTER_H: f32 = 74.0;
const NAME_X: f32 = 46.0;
const NAME_MAX_W: f32 = 170.0;
const MUTED_DOT: u32 = 0x6E7078;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> RectF {
        RectF { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
    pub fn shifted(&self, dy: f32) -> RectF {
        RectF { y: self.y + dy, ..*self }
    }
    pub fn intersect(&self, o: &RectF) -> Option<RectF> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        (x1 > x0 && y1 > y0).then(|| RectF::new(x0, y0, x1 - x0, y1 - y0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Font {
    /// 14 px semibold
    Title,
    /// 13 px regular
    Body,
    /// 13 px semibold
    BodyStrong,
    /// 12 px regular
    Small,
    /// 12 px semibold
    SmallStrong,
    /// 11 px regular, letter-spaced caps
    Caption,
    /// 12 px Cascadia Mono
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// #ECECEE
    Primary,
    /// #A4A6AD
    Secondary,
    /// #6E7078
    Muted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextOp {
    pub rect: RectF,
    pub text: String,
    pub font: Font,
    pub ink: Ink,
    pub align: Align,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Background { rect: RectF, radius: f32 },
    Divider { y: f32 },
    Highlight { rect: RectF },
    Text(TextOp),
    Dot { cx: f32, cy: f32, r: f32, rgb: u32, alpha: f32 },
    Meter { rect: RectF, frac: f32, rgb: u32 },
    AppIcon { rect: RectF },
    Pin { rect: RectF, on: bool },
    Chevron { cx: f32, cy: f32, open: bool },
    Banner { rect: RectF },
    ClipPush(RectF),
    ClipPop,
    Tooltip { rect: RectF, title: String, body: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Hit {
    Pin,
    Session(String),
    SessionName(String),
    StatusLink,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HitRegion {
    pub rect: RectF,
    pub hit: Hit,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    /// User overrides of the default expanded/collapsed state, by session_id.
    pub expanded: HashMap<String, bool>,
    pub hovered: Option<Hit>,
    pub scroll: f32,
    pub pinned: bool,
}

pub trait Measure {
    fn width(&self, text: &str, font: Font) -> f32;
}

pub struct Ctx<'a> {
    pub snap: &'a Snapshot,
    pub light: &'a Light,
    pub view: &'a ViewState,
    pub max_h: f32,
    pub measure: &'a dyn Measure,
    pub local: &'a dyn Fn(i64) -> LocalTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub ops: Vec<Op>,
    pub hits: Vec<HitRegion>,
    pub height: f32,
    pub scroll_max: f32,
}

impl Layout {
    /// The most recently added region under the point wins.
    pub fn hit_at(&self, x: f32, y: f32) -> Option<&Hit> {
        self.hits.iter().rev().find(|h| h.rect.contains(x, y)).map(|h| &h.hit)
    }
}

fn text(rect: RectF, s: &str, font: Font, ink: Ink, align: Align) -> Op {
    Op::Text(TextOp { rect, text: s.to_string(), font, ink, align })
}

fn shift(op: Op, dy: f32) -> Op {
    match op {
        Op::Background { rect, radius } => Op::Background { rect: rect.shifted(dy), radius },
        Op::Divider { y } => Op::Divider { y: y + dy },
        Op::Highlight { rect } => Op::Highlight { rect: rect.shifted(dy) },
        Op::Text(t) => Op::Text(TextOp { rect: t.rect.shifted(dy), ..t }),
        Op::Dot { cx, cy, r, rgb, alpha } => Op::Dot { cx, cy: cy + dy, r, rgb, alpha },
        Op::Meter { rect, frac, rgb } => Op::Meter { rect: rect.shifted(dy), frac, rgb },
        Op::AppIcon { rect } => Op::AppIcon { rect: rect.shifted(dy) },
        Op::Pin { rect, on } => Op::Pin { rect: rect.shifted(dy), on },
        Op::Chevron { cx, cy, open } => Op::Chevron { cx, cy: cy + dy, open },
        Op::Banner { rect } => Op::Banner { rect: rect.shifted(dy) },
        Op::ClipPush(r) => Op::ClipPush(r.shifted(dy)),
        Op::ClipPop => Op::ClipPop,
        Op::Tooltip { rect, title, body } => Op::Tooltip { rect: rect.shifted(dy), title, body },
    }
}

pub fn layout(c: &Ctx) -> Layout {
    let mut ops = Vec::new();
    let mut hits = Vec::new();
    let mut y = header(c, &mut ops, &mut hits);
    y = banner(c, &mut ops, y);
    y = usage_block(c, &mut ops, y);

    ops.push(Op::Divider { y });
    ops.push(text(RectF::new(PAD, y + 10.0, W - 2.0 * PAD, 14.0), "SESSIONS", Font::Caption, Ink::Muted, Align::Left));
    let list_top = y + 30.0;
    let (rows, row_hits, content_h) = session_rows(c);
    let available = (c.max_h - list_top - FOOTER_H).max(ROW_H + 8.0);
    let viewport = content_h.min(available);
    let scroll_max = (content_h - viewport).max(0.0);
    let dy = list_top - c.view.scroll.clamp(0.0, scroll_max);
    let clip = RectF::new(0.0, list_top, W, viewport);
    ops.push(Op::ClipPush(clip));
    ops.extend(rows.into_iter().map(|o| shift(o, dy)));
    ops.push(Op::ClipPop);
    for h in row_hits {
        if let Some(visible) = h.rect.shifted(dy).intersect(&clip) {
            hits.push(HitRegion { rect: visible, hit: h.hit });
        }
    }

    let footer_top = list_top + viewport;
    let (f_ops, f_hits) = footer(c);
    ops.extend(f_ops.into_iter().map(|o| shift(o, footer_top)));
    hits.extend(f_hits.into_iter().map(|h| HitRegion { rect: h.rect.shifted(footer_top), hit: h.hit }));

    let height = footer_top + FOOTER_H;
    ops.insert(0, Op::Background { rect: RectF::new(0.0, 0.0, W, height), radius: RADIUS });
    if let Some(t) = tooltip_op(c, &hits) {
        ops.push(t);
    }
    Layout { ops, hits, height, scroll_max }
}

fn header(c: &Ctx, ops: &mut Vec<Op>, hits: &mut Vec<HitRegion>) -> f32 {
    ops.push(Op::AppIcon { rect: RectF::new(PAD, 14.0, 30.0, 30.0) });
    let text_x = 56.0;
    let pin_x = W - PAD - 24.0;
    let text_end = pin_x - 8.0;
    let plan = c.snap.plan.clone().unwrap_or_else(|| "Claude".to_string());
    let plan_w = c.measure.width(&plan, Font::Title).min(text_end - text_x);
    ops.push(text(RectF::new(text_x, 12.0, plan_w, 20.0), &plan, Font::Title, Ink::Primary, Align::Left));
    if let Some(sp) = c.snap.usage.value.as_ref().and_then(|u| u.spend.as_ref()) {
        let s = match sp.limit_minor {
            Some(l) => format!("· {} of {} spent", money(sp.used_minor, &sp.currency), money(l, &sp.currency)),
            None => format!("· {} spent", money(sp.used_minor, &sp.currency)),
        };
        let x = text_x + plan_w + 6.0;
        if x < text_end {
            ops.push(text(RectF::new(x, 12.0, text_end - x, 20.0), &s, Font::Body, Ink::Secondary, Align::Left));
        }
    }
    let mut sx = text_x;
    if c.light.colour != Colour::Off {
        let alpha = if c.light.dim { DIM_ALPHA } else { 1.0 };
        ops.push(Op::Dot { cx: text_x + 3.0, cy: 42.0, r: 3.0, rgb: c.light.colour.rgb(), alpha });
        sx += 10.0;
    }
    ops.push(text(RectF::new(sx, 34.0, text_end - sx, 16.0), &summary(c), Font::Small, Ink::Secondary, Align::Left));
    let pin = RectF::new(pin_x, 14.0, 24.0, 24.0);
    ops.push(Op::Pin { rect: pin, on: c.view.pinned });
    hits.push(HitRegion { rect: pin, hit: Hit::Pin });
    HEADER_H
}

fn summary(c: &Ctx) -> String {
    match &c.light.reason {
        Reason::QuotaSpent { key, label, .. } => return format!("{} spent", limit_noun(key, label)),
        Reason::SpendSpent { .. } => return "Spend limit reached".to_string(),
        Reason::Crashed { name } => return format!("{name} crashed mid-turn"),
        Reason::ApiError { name } => return format!("{name}: API error"),
        Reason::Incident { component, state } => {
            return format!("{}: {}", short_component(component), component_state_text(*state))
        }
        Reason::RegistryUnknown => return "Session registry unreadable".to_string(),
        _ => {}
    }
    let s = c.snap;
    let n = s.sessions.len();
    if n == 0 {
        return "Nothing running".to_string();
    }
    let waiting = s.sessions.iter().filter(|x| x.waiting_reason().is_some()).count();
    let working = s.sessions.iter().filter(|x| x.is_working()).count();
    let head = if n == 1 { "1 session".to_string() } else { format!("{n} sessions") };
    if waiting > 0 {
        format!("{head} · {waiting} {}", if waiting == 1 { "needs you" } else { "need you" })
    } else if working == 0 {
        format!("{head} · {}", if n == 1 { "idle" } else { "all idle" })
    } else if n == 1 {
        format!("{head} · working")
    } else {
        format!("{head} · {working} working")
    }
}

fn banner(c: &Ctx, ops: &mut Vec<Op>, y: f32) -> f32 {
    let now_s = c.snap.now_ms / 1000;
    let msg = match &c.light.reason {
        Reason::QuotaSpent { key, label, resets_at } => {
            let until = resets_at
                .map(|t| {
                    let r = reset_label(t, now_s, c.local);
                    format!(" New turns will fail until {}.", r.trim_start_matches("resets "))
                })
                .unwrap_or_default();
            format!("{} spent.{until}", limit_noun(key, label))
        }
        Reason::SpendSpent { .. } => "Spend limit reached. New turns may fail until it resets.".to_string(),
        _ => return y,
    };
    let r = RectF::new(12.0, y, W - 24.0, 34.0);
    ops.push(Op::Banner { rect: r });
    ops.push(Op::Dot { cx: 24.0, cy: y + 17.0, r: 4.0, rgb: Colour::Red.rgb(), alpha: 1.0 });
    ops.push(text(RectF::new(34.0, y, W - 24.0 - 30.0, 34.0), &msg, Font::Small, Ink::Primary, Align::Left));
    y + 44.0
}

#[allow(clippy::too_many_arguments)]
fn limit_row(ops: &mut Vec<Op>, y: f32, label: &str, reset: Option<String>, pct: f64, right: &str, right_w: f32, colour: Colour) -> f32 {
    ops.push(text(RectF::new(PAD, y, 200.0, 16.0), label, Font::Small, Ink::Primary, Align::Left));
    if let Some(r) = reset {
        ops.push(text(RectF::new(W - PAD - 150.0, y, 150.0, 16.0), &r, Font::Small, Ink::Muted, Align::Right));
    }
    let meter_w = W - 2.0 * PAD - right_w - 4.0;
    ops.push(Op::Meter { rect: RectF::new(PAD, y + 23.0, meter_w, 6.0), frac: (pct / 100.0).clamp(0.0, 1.0) as f32, rgb: colour.rgb() });
    ops.push(text(RectF::new(W - PAD - right_w, y + 17.0, right_w, 18.0), right, Font::SmallStrong, Ink::Primary, Align::Right));
    y + 40.0
}

fn usage_block(c: &Ctx, ops: &mut Vec<Op>, y0: f32) -> f32 {
    ops.push(Op::Divider { y: y0 });
    let mut y = y0 + 10.0;
    let value = c.snap.usage.value.as_ref();
    let spend_only = value.is_some_and(|u| u.limits.is_empty() && u.spend.is_some());
    ops.push(text(RectF::new(PAD, y, W - 2.0 * PAD, 14.0), if spend_only { "SPEND" } else { "USAGE" }, Font::Caption, Ink::Muted, Align::Left));
    y += 22.0;
    let warn = c.snap.warn_percent;
    let now_s = c.snap.now_ms / 1000;
    match value {
        None => {
            let why = match &c.snap.usage.health {
                Health::Degraded(r) | Health::Unknown(r) if r != NOT_COLLECTED => r.clone(),
                _ => "waiting for first poll".to_string(),
            };
            ops.push(text(RectF::new(PAD, y, W - 2.0 * PAD, 16.0), &format!("Usage unavailable · {why}"), Font::Small, Ink::Muted, Align::Left));
            y += 26.0;
        }
        Some(u) => {
            for l in &u.limits {
                let reset = l.resets_at.map(|t| reset_label(t, now_s, c.local));
                y = limit_row(ops, y, &l.label, reset, l.pct, &pct_label(l.pct), 44.0, severity_colour(l.pct, warn));
            }
            if let Some(sp) = &u.spend {
                let reset = spend_reset_label(&(c.local)(now_s));
                match (sp.pct(), sp.limit_minor) {
                    (Some(p), Some(limit)) => {
                        let right = format!("{} / {}", money(sp.used_minor, &sp.currency), money(limit, &sp.currency));
                        y = limit_row(ops, y, "Monthly spend", Some(reset), p, &right, 96.0, severity_colour(p, warn));
                    }
                    _ => {
                        ops.push(text(RectF::new(PAD, y, 200.0, 16.0), "Monthly spend", Font::Small, Ink::Primary, Align::Left));
                        let s = format!("{} spent · no limit", money(sp.used_minor, &sp.currency));
                        ops.push(text(RectF::new(PAD, y + 18.0, W - 2.0 * PAD, 16.0), &s, Font::Small, Ink::Secondary, Align::Left));
                        y += 40.0;
                    }
                }
            }
            if u.limits.is_empty() && u.spend.is_none() {
                ops.push(text(RectF::new(PAD, y, W - 2.0 * PAD, 16.0), "No usage limits reported", Font::Small, Ink::Muted, Align::Left));
                y += 26.0;
            }
        }
    }
    y + 2.0
}

/// Whether a session row shows its sub-agents: the user's override, else open for
/// waiting/working sessions that have sub-agents.
pub fn is_expanded(view: &ViewState, s: &Session) -> bool {
    let default = !s.subagents.is_empty() && (s.waiting_reason().is_some() || s.is_working());
    view.expanded.get(&s.session_id).copied().unwrap_or(default)
}

fn is_hovered(c: &Ctx, id: &str) -> bool {
    matches!(&c.view.hovered, Some(Hit::Session(x)) | Some(Hit::SessionName(x)) if x == id)
}

/// (name rect, x where the right-aligned state text may start)
fn name_box(c: &Ctx, name: &str, y: f32) -> (RectF, f32) {
    let w = c.measure.width(name, Font::BodyStrong).min(NAME_MAX_W);
    (RectF::new(NAME_X, y + 5.0, w, 18.0), NAME_X + w + 8.0)
}

fn detail_line(s: &Session, waiting: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(w) = waiting {
        parts.push(sentence_case(w));
    }
    if let Some(t) = &s.transcript {
        if let Some(m) = &t.model {
            parts.push(model_name(m));
        }
        if let Some(n) = t.last_turn_input_tokens {
            parts.push(format!("last turn {} in", tokens(n)));
        }
        if t.compacted {
            parts.push("compacted".to_string());
        }
    }
    if !s.subagents.is_empty() {
        let n = s.subagents.len();
        let running = s.subagents.iter().filter(|a| a.state == AgentState::Running).count();
        let mut t = if n == 1 { "1 sub-agent".to_string() } else { format!("{n} sub-agents") };
        if running > 0 {
            t.push_str(&format!(" ({running} running)"));
        }
        parts.push(t);
    }
    if parts.is_empty() {
        parts.push(folder_name(&s.cwd));
    }
    parts.join(" · ")
}

fn agent_row(ops: &mut Vec<Op>, a: &Subagent, y: f32, now_ms: i64) -> f32 {
    let x0 = 56.0 + (a.depth.saturating_sub(1).min(3) as f32) * 12.0;
    let rgb = match a.state {
        AgentState::Running => Colour::Green.rgb(),
        AgentState::Failed => Colour::Red.rgb(),
        _ => MUTED_DOT,
    };
    ops.push(Op::Dot { cx: x0 + 3.0, cy: y + 11.0, r: 3.0, rgb, alpha: 1.0 });
    ops.push(text(RectF::new(x0 + 12.0, y + 3.0, 90.0, 16.0), &a.agent_type, Font::Small, Ink::Secondary, Align::Left));
    let meta_w = 96.0;
    let desc_x = x0 + 108.0;
    let desc = if a.description.is_empty() { "—" } else { a.description.as_str() };
    let ink = if a.state == AgentState::Running { Ink::Primary } else { Ink::Muted };
    let desc_w = (W - PAD - meta_w - 4.0 - desc_x).max(20.0);
    ops.push(text(RectF::new(desc_x, y + 3.0, desc_w, 16.0), desc, Font::Small, ink, Align::Left));
    let time = match (a.state, a.started_ms, a.ended_ms) {
        (AgentState::Stopped, _, _) => "stopped".to_string(),
        (_, Some(s), Some(e)) => elapsed(e - s),
        (_, Some(s), None) => elapsed(now_ms - s),
        _ => String::new(),
    };
    let tok = a.context_tokens.map(tokens).unwrap_or_default();
    let meta = [time, tok].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
    ops.push(text(RectF::new(W - PAD - meta_w, y + 3.0, meta_w, 16.0), &meta, Font::Small, Ink::Muted, Align::Right));
    y + AGENT_H
}

/// Rows in list coordinates (y from 0). Returns (ops, hits, content height).
fn session_rows(c: &Ctx) -> (Vec<Op>, Vec<HitRegion>, f32) {
    let mut ops = Vec::new();
    let mut hits = Vec::new();
    let mut y = 0.0;
    let now = c.snap.now_ms;

    for cr in &c.snap.crashed {
        let row = RectF::new(0.0, y, W, ROW_H);
        if is_hovered(c, &cr.key) {
            ops.push(Op::Highlight { rect: row });
        }
        ops.push(Op::Dot { cx: 36.0, cy: y + 14.0, r: 4.0, rgb: Colour::Red.rgb(), alpha: 1.0 });
        let (name_rect, state_x) = name_box(c, &cr.name, y);
        ops.push(text(name_rect, &cr.name, Font::BodyStrong, Ink::Primary, Align::Left));
        ops.push(text(RectF::new(state_x, y + 5.0, W - PAD - state_x, 18.0), "Crashed mid-turn", Font::Small, Ink::Secondary, Align::Right));
        let l2 = format!("Process ended while working · {}", folder_name(&cr.cwd));
        ops.push(text(RectF::new(NAME_X, y + 24.0, W - NAME_X - PAD, 16.0), &l2, Font::Small, Ink::Muted, Align::Left));
        hits.push(HitRegion { rect: row, hit: Hit::Session(cr.key.clone()) });
        hits.push(HitRegion { rect: name_rect, hit: Hit::SessionName(cr.key.clone()) });
        y += ROW_H;
    }

    let ordered = ordered_sessions(&c.snap.sessions);
    let shown = ordered.len().min(MAX_ROWS);
    for s in ordered.iter().take(MAX_ROWS) {
        let row = RectF::new(0.0, y, W, ROW_H);
        if is_hovered(c, &s.session_id) {
            ops.push(Op::Highlight { rect: row });
        }
        let waiting = s.waiting_reason();
        let working = s.is_working();
        let (rgb, alpha) = if waiting.is_some() {
            (Colour::Yellow.rgb(), 1.0)
        } else if working {
            (Colour::Green.rgb(), 1.0)
        } else {
            (Colour::Green.rgb(), DIM_ALPHA)
        };
        let has_agents = !s.subagents.is_empty();
        let expanded = is_expanded(c.view, s);
        if has_agents {
            ops.push(Op::Chevron { cx: 24.0, cy: y + 14.0, open: expanded });
        }
        ops.push(Op::Dot { cx: 36.0, cy: y + 14.0, r: 4.0, rgb, alpha });
        let (name_rect, state_x) = name_box(c, &s.name, y);
        ops.push(text(name_rect, &s.name, Font::BodyStrong, Ink::Primary, Align::Left));
        let state = if waiting.is_some() {
            "Needs you".to_string()
        } else if working {
            format!("Working · {}", uptime(now - s.started_at_ms))
        } else if s.status == SessionStatus::Idle {
            format!("Idle · {}", uptime(now - s.status_updated_at_ms))
        } else {
            format!("Running · {}", uptime(now - s.started_at_ms))
        };
        ops.push(text(RectF::new(state_x, y + 5.0, W - PAD - state_x, 18.0), &state, Font::Small, Ink::Secondary, Align::Right));
        ops.push(text(RectF::new(NAME_X, y + 24.0, W - NAME_X - PAD, 16.0), &detail_line(s, waiting.as_deref()), Font::Small, Ink::Muted, Align::Left));
        hits.push(HitRegion { rect: row, hit: Hit::Session(s.session_id.clone()) });
        hits.push(HitRegion { rect: name_rect, hit: Hit::SessionName(s.session_id.clone()) });
        y += ROW_H;
        if has_agents && expanded {
            for a in &s.subagents {
                y = agent_row(&mut ops, a, y, now);
            }
            y += 4.0;
        }
    }

    let rest = ordered.len() - shown;
    if rest > 0 {
        let all_idle = ordered[shown..].iter().all(|s| s.waiting_reason().is_none() && !s.is_working());
        let t = if all_idle { format!("+{rest} idle") } else { format!("+{rest} more") };
        ops.push(text(RectF::new(NAME_X, y + 4.0, W - NAME_X - PAD, 16.0), &t, Font::Small, Ink::Muted, Align::Left));
        y += 26.0;
    }
    if c.snap.sessions.is_empty() && c.snap.crashed.is_empty() {
        ops.push(text(RectF::new(PAD, y + 6.0, W - 2.0 * PAD, 18.0), "No Claude sessions running", Font::BodyStrong, Ink::Primary, Align::Left));
        ops.push(text(RectF::new(PAD, y + 26.0, W - 2.0 * PAD, 16.0), "Start claude in a terminal and it appears here.", Font::Small, Ink::Secondary, Align::Left));
        y += 50.0;
    }
    (ops, hits, y + 4.0)
}

fn state_rank(s: ComponentState) -> u8 {
    match s {
        ComponentState::Operational => 0,
        ComponentState::Degraded | ComponentState::Maintenance | ComponentState::Other => 1,
        ComponentState::PartialOutage | ComponentState::MajorOutage => 2,
    }
}

fn state_rgb(s: ComponentState) -> u32 {
    match state_rank(s) {
        0 => Colour::Green.rgb(),
        1 => Colour::Amber.rgb(),
        _ => Colour::Red.rgb(),
    }
}

/// Footer in local coordinates (y from 0, height FOOTER_H).
fn footer(c: &Ctx) -> (Vec<Op>, Vec<HitRegion>) {
    let mut ops = vec![Op::Divider { y: 0.0 }];
    let mut hits = Vec::new();
    let status = c.snap.status.value.as_ref();
    match status {
        Some(st) => {
            let worst = st.components.iter().max_by_key(|x| state_rank(x.state));
            let (rgb, line) = match worst {
                Some(w) if w.state != ComponentState::Operational => (
                    state_rgb(w.state),
                    format!("{} · {}", sentence_case(component_state_text(w.state)), short_component(&w.name)),
                ),
                _ => (Colour::Green.rgb(), sentence_case(&st.description.to_lowercase())),
            };
            ops.push(Op::Dot { cx: 22.0, cy: 18.0, r: 4.0, rgb, alpha: 1.0 });
            ops.push(text(RectF::new(32.0, 10.0, 200.0, 16.0), &line, Font::Small, Ink::Primary, Align::Left));
            let mut x = 32.0;
            for comp in &st.components {
                let name = short_component(&comp.name);
                let w = c.measure.width(&name, Font::Small);
                ops.push(Op::Dot { cx: x + 3.0, cy: 38.0, r: 3.0, rgb: state_rgb(comp.state), alpha: 1.0 });
                ops.push(text(RectF::new(x + 10.0, 30.0, w + 2.0, 16.0), &name, Font::Small, Ink::Muted, Align::Left));
                x += 10.0 + w + 14.0;
            }
        }
        None => {
            ops.push(Op::Dot { cx: 22.0, cy: 18.0, r: 4.0, rgb: MUTED_DOT, alpha: 1.0 });
            ops.push(text(RectF::new(32.0, 10.0, 200.0, 16.0), "Service status unavailable", Font::Small, Ink::Primary, Align::Left));
        }
    }
    let link = RectF::new(W - PAD - 120.0, 10.0, 120.0, 16.0);
    ops.push(text(link, "status.claude.com ↗", Font::Small, Ink::Muted, Align::Right));
    hits.push(HitRegion { rect: link, hit: Hit::StatusLink });

    let mut parts: Vec<String> = Vec::new();
    if let Some(st) = status {
        parts.push(format!("checked {}", ago(c.snap.now_ms - st.checked_at_ms)));
    }
    for h in [&c.snap.registry, &c.snap.usage.health, &c.snap.status.health] {
        if let Health::Degraded(r) | Health::Unknown(r) = h {
            if r != NOT_COLLECTED {
                parts.push(r.clone());
            }
        }
    }
    ops.push(text(RectF::new(32.0, 50.0, W - 48.0, 16.0), &parts.join(" · "), Font::Small, Ink::Muted, Align::Left));
    (ops, hits)
}

fn tooltip_op(c: &Ctx, hits: &[HitRegion]) -> Option<Op> {
    let Some(Hit::SessionName(id)) = &c.view.hovered else { return None };
    let name_rect = hits.iter().find(|h| matches!(&h.hit, Hit::SessionName(x) if x == id))?.rect;
    let cwd = c
        .snap
        .sessions
        .iter()
        .find(|s| &s.session_id == id)
        .map(|s| s.cwd.clone())
        .or_else(|| c.snap.crashed.iter().find(|x| &x.key == id).map(|x| x.cwd.clone()))?;
    let body = middle_ellipsis(&cwd, 300.0 - 16.0, &|t| c.measure.width(t, Font::Mono));
    let w = (c.measure.width(&body, Font::Mono) + 16.0)
        .max(c.measure.width("Working folder", Font::Caption) + 16.0)
        .min(W - 16.0);
    let x = name_rect.x.min(W - 8.0 - w);
    Some(Op::Tooltip { rect: RectF::new(x, name_rect.bottom() + 6.0, w, 40.0), title: "Working folder".to_string(), body })
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib panel::`
Expected: 12 passed. When an assertion about a string fails, the test prints every text op. Compare against §4 and the mockup and fix the layout code, not the expectation, unless the expectation contradicts §4.

- [ ] **Step 6: Lint and commit**

```powershell
cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/lib.rs src/panel
git commit -m "feat(panel): pure layout of header, usage, sessions, sub-agents, footer"
```
