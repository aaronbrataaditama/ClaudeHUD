# Task 11: Settings, placement geometry, hover state machine

**Goal:** Three small pure modules. `settings` loads, saves and sanitises `claudehud.settings.json`. `geometry` computes where the strip and panel go on a monitor in physical pixels (mixed DPI, negative origins, taskbar excluded). `hover` is the reveal/hide/pin state machine as a pure `step()` function.

**Spec:** §6 (settings fields, file location), §2.1 (strip), §2.2 (panel placement), §2.3 (reveal, hide, pin), §2.4 (DPI, negative coordinates).

**Files:**
- Create: `src/settings.rs`, `src/geometry.rs`, `src/hover.rs`
- Create: `tests/settings_io.rs`
- Modify: `src/lib.rs` (add `pub mod settings; pub mod geometry; pub mod hover;`)

**Interfaces:**
- Consumes: nothing beyond `serde`.
- Produces:
  - `settings::Edge { Top, Left }` (serde lowercase, default Top)
  - `settings::Settings { edge: Edge, monitor: String /* "primary" or a MonitorInfo.id */, warn_percent: u8, usage_poll_s: u32, status_poll_s: u32, autostart: bool, first_run_done: bool }` with `Default`, `sanitised(self) -> Settings`
  - `settings::{FILE_NAME, load(&Path) -> Settings, save(&Path, &Settings) -> io::Result<()>, settings_path(exe_dir: &Path, appdata: Option<&Path>) -> PathBuf}`
  - `geometry::{Rect { x, y, w, h: i32 } (+ right(), bottom(), contains(i32,i32)), MonitorInfo { id: String, name: String, primary: bool, bounds: Rect, work: Rect, scale: f32 }, STRIP_LEN, STRIP_THICK, PANEL_W, PANEL_GAP, SHADOW, SLIDE, to_px(f32, f32) -> i32, pick_monitor(&[MonitorInfo], &str) -> Option<&MonitorInfo>, strip_rect(&MonitorInfo, Edge) -> Rect, panel_rect(&MonitorInfo, Edge, Rect, f32) -> Rect, max_content_h(&MonitorInfo) -> f32, edge_borders_other_monitor(&MonitorInfo, &[MonitorInfo], Edge) -> bool, slide_offset(Edge, f32 /*eased 0..1*/, f32 /*scale*/) -> (i32, i32)}`
  - `hover::{Hover { visible, pinned, in_strip, in_panel, reveal_suppressed: bool }, Event, Action}` with `Hover::step(&mut self, Event) -> Vec<Action>`.
    `Event = StripEnter | StripHover | StripLeave | StripClick | PanelEnter | PanelLeave | PinClick | TrayClick | CloseTimer | Suppress`.
    `Action = Show | Hide | StartCloseTimer | CancelCloseTimer | Acknowledge | PinChanged(bool)`. `Acknowledge` accompanies every `Hide` except the one from `Suppress`: a latched crash is acknowledged when the panel that showed it closes (the spec says "opened"; closing is used so the crash row does not vanish while being read).
    `StripHover` is sent by Windows only after the 250 ms dwell (`TME_HOVER`), so a pointer passing across the strip never produces it.

---

## Part A: settings

- [ ] **Step 1: Write failing tests**

`src/settings.rs`, tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.edge, Edge::Top);
        assert_eq!(s.monitor, "primary");
        assert_eq!((s.warn_percent, s.usage_poll_s, s.status_poll_s), (85, 300, 300));
        assert!(!s.autostart && !s.first_run_done);
    }

    #[test]
    fn sanitise_clamps_out_of_range_values() {
        let s = Settings { warn_percent: 5, usage_poll_s: 1, status_poll_s: 0, monitor: String::new(), ..Default::default() }.sanitised();
        assert_eq!((s.warn_percent, s.usage_poll_s, s.status_poll_s, s.monitor.as_str()), (50, 60, 60, "primary"));
        assert_eq!(Settings { warn_percent: 250, ..Default::default() }.sanitised().warn_percent, 99);
    }

    #[test]
    fn partial_json_uses_defaults_and_ignores_unknown_fields() {
        let s: Settings = serde_json::from_str(r#"{"edge":"left","colour":"pink"}"#).unwrap();
        assert_eq!(s.edge, Edge::Left);
        assert_eq!(s.warn_percent, 85);
    }
}
```

Add `pub mod settings;` to `src/lib.rs`.

- [ ] **Step 2: Implement**

Above the tests:

```rust
//! `claudehud.settings.json`, beside the exe when writable, else `%APPDATA%\ClaudeHUD\` (§6).

use crate::collectors::strip_bom;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "claudehud.settings.json";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    #[default]
    Top,
    Left,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub edge: Edge,
    pub monitor: String,
    pub warn_percent: u8,
    pub usage_poll_s: u32,
    pub status_poll_s: u32,
    pub autostart: bool,
    pub first_run_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            edge: Edge::Top,
            monitor: "primary".to_string(),
            warn_percent: 85,
            usage_poll_s: 300,
            status_poll_s: 300,
            autostart: false,
            first_run_done: false,
        }
    }
}

impl Settings {
    pub fn sanitised(mut self) -> Settings {
        self.warn_percent = self.warn_percent.clamp(50, 99);
        self.usage_poll_s = self.usage_poll_s.max(60);
        self.status_poll_s = self.status_poll_s.max(60);
        if self.monitor.trim().is_empty() {
            self.monitor = "primary".to_string();
        }
        self
    }
}

/// Missing or corrupt file → defaults. Never fails.
pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Settings>(strip_bom(&t)).ok())
        .unwrap_or_default()
        .sanitised()
}

/// Atomic: write a temp file then rename over the old one.
pub fn save(path: &Path, s: &Settings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(s).map_err(io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".claudehud-write-test");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn settings_path(exe_dir: &Path, appdata: Option<&Path>) -> PathBuf {
    let beside = exe_dir.join(FILE_NAME);
    if beside.is_file() || dir_writable(exe_dir) {
        return beside;
    }
    match appdata {
        Some(a) => a.join("ClaudeHUD").join(FILE_NAME),
        None => beside,
    }
}
```

- [ ] **Step 3: IO tests**

`tests/settings_io.rs`:

```rust
mod common;

use claudehud::settings::{load, save, settings_path, Edge, Settings, FILE_NAME};

#[test]
fn missing_and_corrupt_files_give_defaults() {
    let t = common::TempDir::new("settings");
    assert_eq!(load(&t.path().join("nope.json")), Settings::default());
    let p = t.write("bad.json", "{not json");
    assert_eq!(load(&p), Settings::default());
    let bom = t.write("bom.json", "\u{feff}{\"edge\":\"left\"}");
    assert_eq!(load(&bom).edge, Edge::Left);
}

#[test]
fn save_then_load_round_trips() {
    let t = common::TempDir::new("settings-rt");
    let p = t.path().join("sub").join(FILE_NAME);
    let s = Settings { edge: Edge::Left, warn_percent: 90, autostart: true, ..Default::default() };
    save(&p, &s).unwrap();
    assert_eq!(load(&p), s);
    save(&p, &Settings::default()).unwrap(); // overwrite existing
    assert_eq!(load(&p), Settings::default());
}

#[test]
fn path_prefers_exe_dir_and_falls_back_to_appdata() {
    let t = common::TempDir::new("settings-path");
    assert_eq!(settings_path(t.path(), None), t.path().join(FILE_NAME));
    let missing = t.path().join("does-not-exist");
    let appdata = t.path().join("appdata");
    assert_eq!(settings_path(&missing, Some(&appdata)), appdata.join("ClaudeHUD").join(FILE_NAME));
}
```

- [ ] **Step 4: Run**

Run: `cargo test --lib settings::` then `cargo test --test settings_io`
Expected: 3 + 3 passed.

## Part B: geometry

- [ ] **Step 5: Write failing tests**

`src/geometry.rs`, tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: &str, primary: bool, b: (i32, i32, i32, i32), taskbar: i32, scale: f32) -> MonitorInfo {
        let (x, y, w, h) = b;
        MonitorInfo {
            id: id.into(),
            name: id.into(),
            primary,
            bounds: Rect { x, y, w, h },
            work: Rect { x, y, w, h: h - taskbar },
            scale,
        }
    }

    #[test]
    fn strip_centred_on_work_area() {
        let m = mon("A", true, (0, 0, 1920, 1080), 40, 1.0);
        assert_eq!(strip_rect(&m, Edge::Top), Rect { x: 894, y: 0, w: 132, h: 4 });
        assert_eq!(strip_rect(&m, Edge::Left), Rect { x: 0, y: 454, w: 4, h: 132 });
    }

    #[test]
    fn negative_origin_and_high_dpi() {
        let left = mon("B", false, (-1920, 0, 1920, 1080), 40, 1.0);
        assert_eq!(strip_rect(&left, Edge::Top).x, -1026);
        let laptop = mon("C", true, (0, 0, 2560, 1600), 72, 1.64);
        let r = strip_rect(&laptop, Edge::Top);
        assert_eq!((r.w, r.h), (216, 7));
        assert_eq!(r.x, (2560 - 216) / 2);
    }

    #[test]
    fn panel_below_top_strip_with_shadow_margin() {
        let m = mon("A", true, (0, 0, 1920, 1080), 40, 1.0);
        let s = strip_rect(&m, Edge::Top);
        let p = panel_rect(&m, Edge::Top, s, 500.0);
        // content box: x = 894 + 66 - 180 = 780, y = 4 + 8 = 12; inflated by the 16 px shadow
        assert_eq!(p, Rect { x: 764, y: -4, w: 392, h: 532 });
    }

    #[test]
    fn panel_right_of_left_strip_and_clamped() {
        let m = mon("A", true, (0, 0, 1920, 1080), 40, 1.0);
        let s = strip_rect(&m, Edge::Left);
        let p = panel_rect(&m, Edge::Left, s, 500.0);
        assert_eq!((p.x + 16, p.y + 16), (12, 454 + 66 - 250));
        // taller than the work area: pinned to the top inset
        let tall = panel_rect(&m, Edge::Left, s, 5000.0);
        assert_eq!(tall.y + 16, 4);
    }

    #[test]
    fn max_height_is_80_percent_of_work_area_in_logical_px() {
        let m = mon("C", true, (0, 0, 2560, 1600), 72, 1.6);
        assert!((max_content_h(&m) - 1528.0 / 1.6 * 0.8).abs() < 0.01);
    }

    #[test]
    fn picks_named_then_primary_then_first() {
        let ms = vec![mon("A", false, (0, 0, 10, 10), 0, 1.0), mon("B", true, (10, 0, 10, 10), 0, 1.0)];
        assert_eq!(pick_monitor(&ms, "A").unwrap().id, "A");
        assert_eq!(pick_monitor(&ms, "primary").unwrap().id, "B");
        assert_eq!(pick_monitor(&ms, "gone").unwrap().id, "B");
        assert!(pick_monitor(&[], "primary").is_none());
    }

    #[test]
    fn detects_edges_shared_with_other_monitors() {
        let a = mon("A", true, (0, 0, 1920, 1080), 40, 1.0);
        let above = mon("U", false, (0, -1080, 1920, 1080), 0, 1.0);
        let left = mon("L", false, (-1920, 0, 1920, 1080), 0, 1.0);
        assert!(edge_borders_other_monitor(&a, &[a.clone(), above.clone()], Edge::Top));
        assert!(!edge_borders_other_monitor(&a, &[a.clone(), above], Edge::Left));
        assert!(edge_borders_other_monitor(&a, &[a.clone(), left.clone()], Edge::Left));
        assert!(!edge_borders_other_monitor(&a, &[a.clone(), left], Edge::Top));
        // a monitor above but offset so it does not cover the strip's span
        let offset = mon("O", false, (1500, -1080, 1920, 1080), 0, 1.0);
        assert!(!edge_borders_other_monitor(&a, &[a.clone(), offset], Edge::Top));
    }

    #[test]
    fn slide_starts_12px_toward_the_strip() {
        assert_eq!(slide_offset(Edge::Top, 0.0, 1.0), (0, -12));
        assert_eq!(slide_offset(Edge::Top, 1.0, 1.0), (0, 0));
        assert_eq!(slide_offset(Edge::Left, 0.0, 2.0), (-24, 0));
    }
}
```

Add `pub mod geometry;` to `src/lib.rs`.

- [ ] **Step 6: Implement**

Above the tests:

```rust
//! Where the strip and panel go, in physical pixels (§2.1, §2.2, §2.4).
//! Constants are logical px; `scale` is the monitor's DPI / 96.

use crate::settings::Edge;

pub const STRIP_LEN: f32 = 132.0;
pub const STRIP_THICK: f32 = 4.0;
pub const PANEL_W: f32 = 360.0;
pub const PANEL_GAP: f32 = 8.0;
/// Transparent margin around the panel's content for the drop shadow.
pub const SHADOW: f32 = 16.0;
pub const SLIDE: f32 = 12.0;
const EDGE_INSET: f32 = 4.0;
const MAX_PANEL_FRACTION: f32 = 0.8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonitorInfo {
    /// Stable device name, e.g. `\\.\DISPLAY1` (saved in settings)
    pub id: String,
    /// Menu label, e.g. "Display 1 · 2560×1600"
    pub name: String,
    pub primary: bool,
    pub bounds: Rect,
    /// Bounds minus the taskbar
    pub work: Rect,
    pub scale: f32,
}

pub fn to_px(logical: f32, scale: f32) -> i32 {
    (logical * scale).round() as i32
}

pub fn pick_monitor<'a>(mons: &'a [MonitorInfo], wanted: &str) -> Option<&'a MonitorInfo> {
    if wanted != "primary" {
        if let Some(m) = mons.iter().find(|m| m.id == wanted) {
            return Some(m);
        }
    }
    mons.iter().find(|m| m.primary).or_else(|| mons.first())
}

pub fn strip_rect(m: &MonitorInfo, edge: Edge) -> Rect {
    let len = to_px(STRIP_LEN, m.scale);
    let th = to_px(STRIP_THICK, m.scale).max(1);
    match edge {
        Edge::Top => Rect { x: m.work.x + (m.work.w - len) / 2, y: m.work.y, w: len, h: th },
        Edge::Left => Rect { x: m.work.x, y: m.work.y + (m.work.h - len) / 2, w: th, h: len },
    }
}

/// Panel *window* rect (content inflated by SHADOW on every side), clamped so the
/// content stays inside the work area. `content_h` is logical px.
pub fn panel_rect(m: &MonitorInfo, edge: Edge, strip: Rect, content_h: f32) -> Rect {
    let s = m.scale;
    let (w, h) = (to_px(PANEL_W, s), to_px(content_h, s));
    let (gap, sh, inset) = (to_px(PANEL_GAP, s), to_px(SHADOW, s), to_px(EDGE_INSET, s));
    let (x, y) = match edge {
        Edge::Top => (strip.x + strip.w / 2 - w / 2, strip.bottom() + gap),
        Edge::Left => (strip.right() + gap, strip.y + strip.h / 2 - h / 2),
    };
    let x = x.clamp(m.work.x + inset, (m.work.right() - inset - w).max(m.work.x + inset));
    let y = y.clamp(m.work.y + inset, (m.work.bottom() - inset - h).max(m.work.y + inset));
    Rect { x: x - sh, y: y - sh, w: w + 2 * sh, h: h + 2 * sh }
}

pub fn max_content_h(m: &MonitorInfo) -> f32 {
    m.work.h as f32 / m.scale * MAX_PANEL_FRACTION
}

fn overlaps(a0: i32, a1: i32, b0: i32, b1: i32) -> bool {
    a0 < b1 && b0 < a1
}

/// True when another monitor touches `edge` of `m` where the strip is, so
/// moving the pointer between displays would cross the strip (§2.3).
pub fn edge_borders_other_monitor(m: &MonitorInfo, all: &[MonitorInfo], edge: Edge) -> bool {
    let strip = strip_rect(m, edge);
    all.iter().filter(|o| o.id != m.id).any(|o| match edge {
        Edge::Top => (o.bounds.bottom() - m.bounds.y).abs() <= 1 && overlaps(o.bounds.x, o.bounds.right(), strip.x, strip.right()),
        Edge::Left => (o.bounds.right() - m.bounds.x).abs() <= 1 && overlaps(o.bounds.y, o.bounds.bottom(), strip.y, strip.bottom()),
    })
}

/// Offset of the panel from its final position during the slide; `eased` 0 → 1.
pub fn slide_offset(edge: Edge, eased: f32, scale: f32) -> (i32, i32) {
    let d = -to_px(SLIDE * (1.0 - eased.clamp(0.0, 1.0)), scale);
    match edge {
        Edge::Top => (0, d),
        Edge::Left => (d, 0),
    }
}
```

- [ ] **Step 7: Run**

Run: `cargo test --lib geometry::`
Expected: 8 passed.

## Part C: hover state machine

- [ ] **Step 8: Write failing tests**

`src/hover.rs`, tests only:

```rust
#[cfg(test)]
mod tests {
    use super::Action::*;
    use super::Event::*;
    use super::*;

    fn opened() -> Hover {
        let mut h = Hover::default();
        h.step(StripEnter);
        h.step(StripHover);
        h
    }

    #[test]
    fn passing_across_the_strip_opens_nothing() {
        let mut h = Hover::default();
        assert!(h.step(StripEnter).is_empty());
        assert!(h.step(StripLeave).is_empty());
        assert!(!h.visible);
    }

    #[test]
    fn dwell_reveals_and_acknowledges() {
        let mut h = Hover::default();
        h.step(StripEnter);
        assert_eq!(h.step(StripHover), vec![Show]);
        assert!(h.visible && !h.pinned);
    }

    #[test]
    fn leaving_both_closes_after_the_timer() {
        let mut h = opened();
        assert_eq!(h.step(StripLeave), vec![StartCloseTimer]);
        assert_eq!(h.step(CloseTimer), vec![Hide, Acknowledge]);
        assert!(!h.visible);
    }

    #[test]
    fn strip_to_panel_traversal_keeps_it_open() {
        let mut h = opened();
        assert_eq!(h.step(StripLeave), vec![StartCloseTimer]);
        assert_eq!(h.step(PanelEnter), vec![CancelCloseTimer]);
        assert!(h.step(CloseTimer).is_empty(), "late timer is ignored while inside");
        assert!(h.visible);
    }

    #[test]
    fn leave_and_return_within_grace() {
        let mut h = opened();
        h.step(StripLeave);
        h.step(PanelEnter);
        assert_eq!(h.step(PanelLeave), vec![StartCloseTimer]);
        assert_eq!(h.step(StripEnter), vec![CancelCloseTimer]);
        assert!(h.visible);
    }

    #[test]
    fn strip_click_pins_and_second_click_closes() {
        let mut h = Hover::default();
        assert_eq!(h.step(StripClick), vec![Show, CancelCloseTimer, PinChanged(true)]);
        h.step(StripLeave);
        assert!(h.step(CloseTimer).is_empty(), "pinned ignores the close timer");
        h.step(StripEnter);
        assert_eq!(h.step(StripClick), vec![PinChanged(false), Hide, Acknowledge]);
        assert!(!h.visible && !h.pinned);
    }

    #[test]
    fn pin_glyph_toggles() {
        let mut h = opened();
        h.step(StripLeave);
        h.step(PanelEnter);
        assert_eq!(h.step(PinClick), vec![PinChanged(true), CancelCloseTimer]);
        assert_eq!(h.step(PinClick), vec![PinChanged(false)], "pointer still inside: no close");
        assert_eq!(h.step(PanelLeave), vec![StartCloseTimer]);
    }

    #[test]
    fn tray_click_pins_an_open_panel_and_closes_a_pinned_one() {
        let mut h = opened();
        assert_eq!(h.step(TrayClick), vec![CancelCloseTimer, PinChanged(true)]);
        assert_eq!(h.step(TrayClick), vec![PinChanged(false), Hide, Acknowledge]);
        assert_eq!(h.step(TrayClick), vec![Show, CancelCloseTimer, PinChanged(true)]);
    }

    #[test]
    fn suppressed_edge_ignores_hover_but_not_clicks() {
        let mut h = Hover { reveal_suppressed: true, ..Default::default() };
        h.step(StripEnter);
        assert!(h.step(StripHover).is_empty());
        assert!(h.step(StripClick).contains(&Show));
    }

    #[test]
    fn suppress_hides_and_unpins() {
        let mut h = Hover::default();
        h.step(StripClick);
        assert_eq!(h.step(Suppress), vec![Hide, PinChanged(false)]);
        assert!(h.step(Suppress).is_empty());
    }
}
```

Add `pub mod hover;` to `src/lib.rs`.

- [ ] **Step 9: Implement**

Above the tests:

```rust
//! Reveal / hide / pin logic for the panel (§2.3), as a pure state machine.
//! The platform layer turns Win32 mouse messages into `Event`s and carries out `Action`s.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// First mouse move over the strip.
    StripEnter,
    /// Windows' TME_HOVER: the pointer rested on the strip for 250 ms.
    StripHover,
    StripLeave,
    StripClick,
    PanelEnter,
    PanelLeave,
    PinClick,
    TrayClick,
    /// The 300 ms close timer fired.
    CloseTimer,
    /// Workstation locked or a fullscreen app started.
    Suppress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Show,
    Hide,
    StartCloseTimer,
    CancelCloseTimer,
    /// The panel was seen and is closing: acknowledge any latched crash (§1).
    /// Sent with every Hide except the one caused by Suppress (lock/fullscreen),
    /// so a crash row stays visible for as long as the panel is open.
    Acknowledge,
    PinChanged(bool),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hover {
    pub visible: bool,
    pub pinned: bool,
    pub in_strip: bool,
    pub in_panel: bool,
    /// The strip's edge borders another monitor: hover never reveals.
    pub reveal_suppressed: bool,
}

impl Hover {
    pub fn step(&mut self, ev: Event) -> Vec<Action> {
        use Action::*;
        let mut out = Vec::new();
        match ev {
            Event::StripEnter => {
                self.in_strip = true;
                if self.visible {
                    out.push(CancelCloseTimer);
                }
            }
            Event::StripHover => {
                self.in_strip = true;
                if !self.visible && !self.reveal_suppressed {
                    self.visible = true;
                    out.push(Show);
                }
            }
            Event::StripLeave => {
                self.in_strip = false;
                self.close_if_outside(&mut out);
            }
            Event::PanelEnter => {
                self.in_panel = true;
                out.push(CancelCloseTimer);
            }
            Event::PanelLeave => {
                self.in_panel = false;
                self.close_if_outside(&mut out);
            }
            Event::StripClick | Event::TrayClick => {
                if self.visible && self.pinned {
                    self.pinned = false;
                    self.visible = false;
                    out.extend([PinChanged(false), Hide, Acknowledge]);
                } else {
                    if !self.visible {
                        self.visible = true;
                        out.push(Show);
                    }
                    self.pinned = true;
                    out.extend([CancelCloseTimer, PinChanged(true)]);
                }
            }
            Event::PinClick => {
                if self.visible {
                    self.pinned = !self.pinned;
                    out.push(PinChanged(self.pinned));
                    if self.pinned {
                        out.push(CancelCloseTimer);
                    } else {
                        self.close_if_outside(&mut out);
                    }
                }
            }
            Event::CloseTimer => {
                if self.visible && !self.pinned && !self.in_strip && !self.in_panel {
                    self.visible = false;
                    out.extend([Hide, Acknowledge]);
                }
            }
            Event::Suppress => {
                if self.visible {
                    self.visible = false;
                    out.push(Hide);
                }
                if self.pinned {
                    self.pinned = false;
                    out.push(PinChanged(false));
                }
            }
        }
        out
    }

    fn close_if_outside(&self, out: &mut Vec<Action>) {
        if self.visible && !self.pinned && !self.in_strip && !self.in_panel {
            out.push(Action::StartCloseTimer);
        }
    }
}
```

- [ ] **Step 10: Run all tests, lint, commit**

Run: `cargo test` (all), `cargo clippy --all-targets -- -D warnings`, `cargo fmt`
Expected: `hover::` 10 passed; everything else still green.

```powershell
git add src/lib.rs src/settings.rs src/geometry.rs src/hover.rs tests/settings_io.rs
git commit -m "feat(ui-core): settings, placement geometry, hover state machine"
```
