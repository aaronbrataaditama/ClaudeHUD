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
        Edge::Top => Rect {
            x: m.work.x + (m.work.w - len) / 2,
            y: m.work.y,
            w: len,
            h: th,
        },
        Edge::Left => Rect {
            x: m.work.x,
            y: m.work.y + (m.work.h - len) / 2,
            w: th,
            h: len,
        },
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
    let x = x.clamp(
        m.work.x + inset,
        (m.work.right() - inset - w).max(m.work.x + inset),
    );
    let y = y.clamp(
        m.work.y + inset,
        (m.work.bottom() - inset - h).max(m.work.y + inset),
    );
    Rect {
        x: x - sh,
        y: y - sh,
        w: w + 2 * sh,
        h: h + 2 * sh,
    }
}

pub fn max_content_h(m: &MonitorInfo) -> f32 {
    m.work.h as f32 / m.scale * MAX_PANEL_FRACTION
}

/// Offset of the panel from its final position during the slide; `eased` 0 → 1.
pub fn slide_offset(edge: Edge, eased: f32, scale: f32) -> (i32, i32) {
    let d = -to_px(SLIDE * (1.0 - eased.clamp(0.0, 1.0)), scale);
    match edge {
        Edge::Top => (0, d),
        Edge::Left => (d, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(
        id: &str,
        primary: bool,
        b: (i32, i32, i32, i32),
        taskbar: i32,
        scale: f32,
    ) -> MonitorInfo {
        let (x, y, w, h) = b;
        MonitorInfo {
            id: id.into(),
            name: id.into(),
            primary,
            bounds: Rect { x, y, w, h },
            work: Rect {
                x,
                y,
                w,
                h: h - taskbar,
            },
            scale,
        }
    }

    #[test]
    fn strip_centred_on_work_area() {
        let m = mon("A", true, (0, 0, 1920, 1080), 40, 1.0);
        assert_eq!(
            strip_rect(&m, Edge::Top),
            Rect {
                x: 894,
                y: 0,
                w: 132,
                h: 4
            }
        );
        assert_eq!(
            strip_rect(&m, Edge::Left),
            Rect {
                x: 0,
                y: 454,
                w: 4,
                h: 132
            }
        );
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
        assert_eq!(
            p,
            Rect {
                x: 764,
                y: -4,
                w: 392,
                h: 532
            }
        );
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
        let ms = vec![
            mon("A", false, (0, 0, 10, 10), 0, 1.0),
            mon("B", true, (10, 0, 10, 10), 0, 1.0),
        ];
        assert_eq!(pick_monitor(&ms, "A").unwrap().id, "A");
        assert_eq!(pick_monitor(&ms, "primary").unwrap().id, "B");
        assert_eq!(pick_monitor(&ms, "gone").unwrap().id, "B");
        assert!(pick_monitor(&[], "primary").is_none());
    }

    #[test]
    fn slide_starts_12px_toward_the_strip() {
        assert_eq!(slide_offset(Edge::Top, 0.0, 1.0), (0, -12));
        assert_eq!(slide_offset(Edge::Top, 1.0, 1.0), (0, 0));
        assert_eq!(slide_offset(Edge::Left, 0.0, 2.0), (-24, 0));
    }
}
