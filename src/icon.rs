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
    [
        ((rgb >> 16) & 0xFF) as f32,
        ((rgb >> 8) & 0xFF) as f32,
        (rgb & 0xFF) as f32,
    ]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn px(buf: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }
    fn bgra(rgb: u32) -> [u8; 4] {
        [
            (rgb & 0xFF) as u8,
            ((rgb >> 8) & 0xFF) as u8,
            ((rgb >> 16) & 0xFF) as u8,
            255,
        ]
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
        assert_eq!(
            px(&b, 16, 4, 4),
            bgra(EYE),
            "left eye at grid (3,1) + origin (1,3)"
        );
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
        let b = render_tray_icon(
            16,
            Some(Badge {
                rgb: GREEN,
                alpha: 1.0,
            }),
            false,
        );
        assert_eq!(px(&b, 16, 12, 12), bgra(GREEN), "badge centre");
        assert_eq!(
            px(&plain, 16, 10, 9)[3],
            255,
            "leg pixel exists without badge"
        );
        assert!(
            px(&b, 16, 10, 9)[3] < 128,
            "leg pixel inside the cut-out ring is mostly cleared"
        );
    }

    #[test]
    fn dim_badge_is_translucent() {
        let b = render_tray_icon(
            16,
            Some(Badge {
                rgb: GREEN,
                alpha: 0.55,
            }),
            false,
        );
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
