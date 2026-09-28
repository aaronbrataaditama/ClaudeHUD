# Task 18: The hover panel (Direct2D renderer + window + hover wiring)

**Goal:** Hovering the strip for 250 ms slides out the rounded panel drawn from Task 12's layout. Leaving closes it after 300 ms; clicking the strip or the pin glyph pins it; the tray left-click toggles it; hovering a session name shows the folder tooltip; clicking a session toggles its sub-agents; the wheel scrolls the session list; the status link opens the browser. The panel never takes focus.

**Spec:** §2.2 (panel window: `UpdateLayeredWindow` from a Direct2D-drawn premultiplied bitmap, 12 px corners, shadow, created at startup, redraw only on change + 1 Hz while visible, `MA_NOACTIVATE`), §2.3 (reveal/hide/pin, animation 180/140 ms), §4 (content), §7 (WARP fallback). Visual reference: `claudehud-mockup.html` sections 1–3; match its colours and spacing.

Read the README's **Notes for implementers** first. Direct2D/DirectWrite signatures vary most between `windows` versions. In 0.61, points are `windows::Foundation::Numerics::Vector2 { X, Y }` (feature `Foundation_Numerics`, added in Task 1) and `D2D1_ELLIPSE.point` uses the same type. `GetMetrics` may return the struct instead of filling an out-param. Adjust at the call site only.

**Files:**
- Create: `src/platform/render.rs`
- Replace: `src/platform/app.rs` (full new version below)
- Modify: `src/platform/mod.rs` (add `pub mod render;`)

**Interfaces:**
- Consumes: `panel::layout::{layout, is_expanded, Ctx, Layout, Op, TextOp, Font, Ink, Align, RectF, Hit, ViewState, Measure, W, RADIUS}`, `hover::{Hover, Event, Action}`, `geometry::{panel_rect, max_content_h, slide_offset, edge_borders_other_monitor, SHADOW}`, `platform::layered::{dib_info, present_dc, move_and_fade}`, Task 17's app.
- Produces: `platform::render::{Renderer, DwMeasure}`: `Renderer::new(HINSTANCE) -> Result<Renderer, String>`, `measure(&self) -> DwMeasure<'_>` (implements `Measure`), `render(&mut self, &Layout, window: Rect, scale: f32) -> Result<(), String>`, `present(&self, HWND, Rect, u8) -> bool`, field `software: bool`.

---

- [x] **Step 1: The renderer**

`src/platform/render.rs`:

```rust
//! Draws `panel::layout` ops with Direct2D/DirectWrite into a 32-bpp DIB that the
//! panel window shows with UpdateLayeredWindow (§2.2). Makes no layout decisions.
//! Coordinates: layout ops are logical px from the content's top-left; the bitmap
//! has a SHADOW margin on every side and the target's DPI is set to 96 × scale.

use super::layered::{dib_info, present_dc};
use crate::geometry::{Rect, SHADOW};
use crate::panel::layout::{Align, Font, Ink, Layout, Measure, Op, RectF, TextOp, RADIUS, W};
use std::collections::HashMap;
use std::ffi::c_void;
use windows::core::{w, PCWSTR};
use windows::Foundation::Numerics::Vector2;
use windows::Win32::Foundation::{HINSTANCE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_ALIASED,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapPaletteTypeMedianCut,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Controls::LoadIconWithScaleDown;
use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;

const SURFACE: u32 = 0x1C1D21;
const RED: u32 = 0xE0444E;

fn ink_rgb(i: Ink) -> u32 {
    match i {
        Ink::Primary => 0xECECEE,
        Ink::Secondary => 0xA4A6AD,
        Ink::Muted => 0x6E7078,
    }
}

fn colour(rgb: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
        g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
        b: (rgb & 0xFF) as f32 / 255.0,
        a,
    }
}

fn pt(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x + SHADOW, Y: y + SHADOW }
}

fn d2r(r: RectF) -> D2D_RECT_F {
    D2D_RECT_F { left: r.x + SHADOW, top: r.y + SHADOW, right: r.right() + SHADOW, bottom: r.bottom() + SHADOW }
}

fn inflate(r: RectF, d: f32) -> RectF {
    RectF::new(r.x - d, r.y - d, r.w + 2.0 * d, r.h + 2.0 * d)
}

/// A memory DC with a top-down 32-bpp DIB selected into it.
struct Surface {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    w: i32,
    h: i32,
}

impl Surface {
    fn new(w: i32, h: i32) -> Option<Surface> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits: *mut c_void = std::ptr::null_mut();
            match CreateDIBSection(Some(dc), &dib_info(w, h), DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bmp) => {
                    let old = SelectObject(dc, bmp.into());
                    Some(Surface { dc, bmp, old, w, h })
                }
                Err(_) => {
                    let _ = DeleteDC(dc);
                    None
                }
            }
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

pub struct Renderer {
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    wic: IWICImagingFactory,
    target: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    formats: HashMap<Font, IDWriteTextFormat>,
    surface: Option<Surface>,
    icon: Option<(u32, ID2D1Bitmap)>,
    hinst: HINSTANCE,
    /// True when running on the WARP software rasteriser.
    pub software: bool,
}

fn make_target(f: &ID2D1Factory, kind: D2D1_RENDER_TARGET_TYPE) -> windows::core::Result<ID2D1DCRenderTarget> {
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: kind,
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
        dpiX: 0.0,
        dpiY: 0.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    unsafe { f.CreateDCRenderTarget(&props) }
}

fn make_format(dw: &IDWriteFactory, family: PCWSTR, weight: DWRITE_FONT_WEIGHT, size: f32) -> Result<IDWriteTextFormat, String> {
    unsafe {
        let f = dw
            .CreateTextFormat(family, None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us"))
            .map_err(|e| format!("CreateTextFormat: {e}"))?;
        let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        if let Ok(sign) = dw.CreateEllipsisTrimmingSign(&f) {
            let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
            let _ = f.SetTrimming(&trimming, &sign);
        }
        Ok(f)
    }
}

impl Renderer {
    pub fn new(hinst: HINSTANCE) -> Result<Renderer, String> {
        unsafe {
            let factory: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).map_err(|e| format!("D2D1CreateFactory: {e}"))?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).map_err(|e| format!("DWriteCreateFactory: {e}"))?;
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("WIC: {e}"))?;
            let (target, software) = match make_target(&factory, D2D1_RENDER_TARGET_TYPE_DEFAULT) {
                Ok(t) => (t, false),
                Err(_) => (make_target(&factory, D2D1_RENDER_TARGET_TYPE_SOFTWARE).map_err(|e| format!("CreateDCRenderTarget: {e}"))?, true),
            };
            let brush = target.CreateSolidColorBrush(&colour(0xFFFFFF, 1.0), None).map_err(|e| format!("brush: {e}"))?;
            let ui = w!("Segoe UI Variable Text");
            let mut formats = HashMap::new();
            for (font, family, weight, size) in [
                (Font::Title, ui, DWRITE_FONT_WEIGHT_SEMI_BOLD, 14.0),
                (Font::Body, ui, DWRITE_FONT_WEIGHT_NORMAL, 13.0),
                (Font::BodyStrong, ui, DWRITE_FONT_WEIGHT_SEMI_BOLD, 13.0),
                (Font::Small, ui, DWRITE_FONT_WEIGHT_NORMAL, 12.0),
                (Font::SmallStrong, ui, DWRITE_FONT_WEIGHT_SEMI_BOLD, 12.0),
                (Font::Caption, ui, DWRITE_FONT_WEIGHT_NORMAL, 11.0),
                (Font::Mono, w!("Cascadia Mono"), DWRITE_FONT_WEIGHT_NORMAL, 12.0),
            ] {
                formats.insert(font, make_format(&dwrite, family, weight, size)?);
            }
            Ok(Renderer { factory, dwrite, wic, target, brush, formats, surface: None, icon: None, hinst, software })
        }
    }

    pub fn measure(&self) -> DwMeasure<'_> {
        DwMeasure { r: self }
    }

    /// Draws `layout` into a bitmap sized for `window` (physical px, shadow margin included).
    pub fn render(&mut self, layout: &Layout, window: Rect, scale: f32) -> Result<(), String> {
        let needs_new = match &self.surface {
            Some(s) => s.w != window.w || s.h != window.h,
            None => true,
        };
        if needs_new {
            self.surface = None;
            self.surface = Some(Surface::new(window.w, window.h).ok_or("CreateDIBSection failed")?);
        }
        let dc = match &self.surface {
            Some(s) => s.dc,
            None => return Err("no surface".to_string()),
        };
        unsafe {
            self.target
                .BindDC(dc, &RECT { left: 0, top: 0, right: window.w, bottom: window.h })
                .map_err(|e| format!("BindDC: {e}"))?;
            self.target.SetDpi(96.0 * scale, 96.0 * scale);
            self.target.BeginDraw();
            self.target.Clear(Some(&colour(0, 0.0)));
            self.shadow(layout.height);
            for op in &layout.ops {
                self.draw(op, scale);
            }
            if let Err(e) = self.target.EndDraw(None, None) {
                self.recover();
                return Err(format!("EndDraw: {e}"));
            }
        }
        Ok(())
    }

    pub fn present(&self, hwnd: HWND, at: Rect, alpha: u8) -> bool {
        match &self.surface {
            Some(s) => present_dc(hwnd, at, s.dc, alpha),
            None => false,
        }
    }

    /// Device loss (D2DERR_RECREATE_TARGET) or a driver reset: rebuild the target.
    fn recover(&mut self) {
        let kind = if self.software { D2D1_RENDER_TARGET_TYPE_SOFTWARE } else { D2D1_RENDER_TARGET_TYPE_DEFAULT };
        if let Ok(t) = make_target(&self.factory, kind) {
            if let Ok(b) = unsafe { t.CreateSolidColorBrush(&colour(0xFFFFFF, 1.0), None) } {
                self.target = t;
                self.brush = b;
            }
        }
        self.icon = None;
    }

    unsafe fn set(&self, rgb: u32, a: f32) {
        self.brush.SetColor(&colour(rgb, a));
    }

    unsafe fn fill_rect(&self, r: RectF, rgb: u32, a: f32) {
        self.set(rgb, a);
        self.target.FillRectangle(&d2r(r), &self.brush);
    }

    unsafe fn fill_round(&self, r: RectF, radius: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        let rr = D2D1_ROUNDED_RECT { rect: d2r(r), radiusX: radius, radiusY: radius };
        self.target.FillRoundedRectangle(&rr, &self.brush);
    }

    unsafe fn stroke_round(&self, r: RectF, radius: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        let rr = D2D1_ROUNDED_RECT { rect: d2r(inflate(r, -0.5)), radiusX: radius - 0.5, radiusY: radius - 0.5 };
        self.target.DrawRoundedRectangle(&rr, &self.brush, 1.0, None);
    }

    unsafe fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, rgb: u32, width: f32) {
        self.set(rgb, 1.0);
        self.target.DrawLine(pt(x0, y0), pt(x1, y1), &self.brush, width, None);
    }

    unsafe fn dot(&self, cx: f32, cy: f32, r: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        let e = D2D1_ELLIPSE { point: pt(cx, cy), radiusX: r, radiusY: r };
        self.target.FillEllipse(&e, &self.brush);
    }

    unsafe fn text(&self, t: &TextOp) {
        let Some(f) = self.formats.get(&t.font) else { return };
        let align = if t.align == Align::Right { DWRITE_TEXT_ALIGNMENT_TRAILING } else { DWRITE_TEXT_ALIGNMENT_LEADING };
        let _ = f.SetTextAlignment(align);
        self.set(ink_rgb(t.ink), 1.0);
        let s: Vec<u16> = t.text.encode_utf16().collect();
        self.target.DrawText(&s, f, &d2r(t.rect), &self.brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
    }

    /// Soft drop shadow: stacked, expanding translucent rounded rects.
    unsafe fn shadow(&self, height: f32) {
        let content = RectF::new(0.0, 3.0, W, height);
        for i in 1..=8 {
            let d = i as f32 * 1.5;
            self.fill_round(inflate(content, d), RADIUS + d, 0x000000, 0.022);
        }
    }

    fn app_icon(&mut self, px: u32) -> Option<ID2D1Bitmap> {
        if let Some((size, bmp)) = &self.icon {
            if *size == px {
                return Some(bmp.clone());
            }
        }
        unsafe {
            // resource id 1 = assets/claudehud.ico (Task 14)
            let hicon = LoadIconWithScaleDown(Some(self.hinst), PCWSTR(1usize as *const u16), px as i32, px as i32).ok()?;
            let wic_bmp = self.wic.CreateBitmapFromHICON(hicon);
            let _ = DestroyIcon(hicon);
            let conv = self.wic.CreateFormatConverter().ok()?;
            conv.Initialize(&wic_bmp.ok()?, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)
                .ok()?;
            let bmp = self.target.CreateBitmapFromWicBitmap(&conv, None).ok()?;
            self.icon = Some((px, bmp.clone()));
            Some(bmp)
        }
    }

    unsafe fn draw(&mut self, op: &Op, scale: f32) {
        match op {
            Op::Background { rect, radius } => {
                self.fill_round(*rect, *radius, SURFACE, 0.96);
                self.stroke_round(*rect, *radius, 0xFFFFFF, 0.08);
            }
            Op::Divider { y } => self.fill_rect(RectF::new(0.0, *y, W, 1.0), 0xFFFFFF, 0.06),
            Op::Highlight { rect } => self.fill_rect(*rect, 0xFFFFFF, 0.025),
            Op::Text(t) => self.text(t),
            Op::Dot { cx, cy, r, rgb, alpha } => self.dot(*cx, *cy, *r, *rgb, *alpha),
            Op::Meter { rect, frac, rgb } => {
                let radius = rect.h / 2.0;
                self.fill_round(*rect, radius, *rgb, 0.18);
                if *frac > 0.0 {
                    let w = (rect.w * frac).max(rect.h).min(rect.w);
                    self.fill_round(RectF { w, ..*rect }, radius, *rgb, 1.0);
                }
            }
            Op::AppIcon { rect } => {
                let px = (rect.w * scale).round().max(16.0) as u32;
                if let Some(bmp) = self.app_icon(px) {
                    self.target.DrawBitmap(&bmp, Some(&d2r(*rect)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                }
            }
            Op::Pin { rect, on } => {
                if *on {
                    self.fill_round(*rect, 6.0, 0x24262B, 1.0);
                }
                let c = ink_rgb(if *on { Ink::Primary } else { Ink::Muted });
                let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
                self.line(cx - 4.0, cy + 1.0, cx + 4.0, cy + 1.0, c, 1.4); // crossbar
                self.line(cx, cy + 1.0, cx, cy + 7.0, c, 1.4); // needle
                self.set(c, 1.0);
                let head = D2D1_ELLIPSE { point: pt(cx, cy - 3.0), radiusX: 3.0, radiusY: 3.0 };
                self.target.DrawEllipse(&head, &self.brush, 1.4, None);
            }
            Op::Chevron { cx, cy, open } => {
                let c = ink_rgb(Ink::Muted);
                if *open {
                    self.line(cx - 3.0, cy - 1.5, *cx, cy + 1.5, c, 1.3);
                    self.line(*cx, cy + 1.5, cx + 3.0, cy - 1.5, c, 1.3);
                } else {
                    self.line(cx - 1.5, cy - 3.0, cx + 1.5, *cy, c, 1.3);
                    self.line(cx + 1.5, *cy, cx - 1.5, cy + 3.0, c, 1.3);
                }
            }
            Op::Banner { rect } => {
                self.fill_round(*rect, 8.0, RED, 0.12);
                self.stroke_round(*rect, 8.0, RED, 0.25);
            }
            Op::ClipPush(r) => self.target.PushAxisAlignedClip(&d2r(*r), D2D1_ANTIALIAS_MODE_ALIASED),
            Op::ClipPop => self.target.PopAxisAlignedClip(),
            Op::Tooltip { rect, title, body } => {
                self.fill_round(*rect, 6.0, 0x2E3036, 1.0);
                self.stroke_round(*rect, 6.0, 0xFFFFFF, 0.12);
                let t = TextOp { rect: RectF::new(rect.x + 8.0, rect.y + 4.0, rect.w - 16.0, 14.0), text: title.clone(), font: Font::Caption, ink: Ink::Muted, align: Align::Left };
                self.text(&t);
                let b = TextOp { rect: RectF::new(rect.x + 8.0, rect.y + 19.0, rect.w - 16.0, 16.0), text: body.clone(), font: Font::Mono, ink: Ink::Primary, align: Align::Left };
                self.text(&b);
            }
        }
    }
}

/// Text measurement for `panel::layout`, in logical px.
pub struct DwMeasure<'a> {
    r: &'a Renderer,
}

impl Measure for DwMeasure<'_> {
    fn width(&self, text: &str, font: Font) -> f32 {
        let Some(f) = self.r.formats.get(&font) else { return text.chars().count() as f32 * 7.0 };
        let s: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            match self.r.dwrite.CreateTextLayout(&s, f, 10_000.0, 100.0) {
                Ok(l) => {
                    let mut m = DWRITE_TEXT_METRICS::default();
                    if l.GetMetrics(&mut m).is_ok() {
                        m.widthIncludingTrailingWhitespace
                    } else {
                        0.0
                    }
                }
                Err(_) => 0.0,
            }
        }
    }
}
```

Add `pub mod render;` to `src/platform/mod.rs`.

- [x] **Step 2: Replace `src/platform/app.rs`**

New compared with Task 17: `hinst`, the `panel` window and its class, `renderer`, `hover`/`view`/`layout`/`anim` state, `COM` initialisation, strip and panel mouse handling, `dispatch()`, the close and animation timers, tray left-click, and a panel redraw on every refresh while it is visible.

```rust
//! Message loop and wiring. Task 18 version: adds the hover panel.

use super::layered;
use super::localtime::local_parts;
use super::monitors;
use super::process::WinProbe;
use super::render::Renderer;
use super::system;
use super::tray::{Tray, WM_TRAY};
use super::win;
use super::worker::{Worker, WorkerMsg, WM_WORKER};
use crate::collect::Collector;
use crate::geometry::{self, MonitorInfo, Rect};
use crate::hover::{Action, Event, Hover};
use crate::icon::{self, Badge};
use crate::model::{Colour, Light, Snapshot, DIM_ALPHA};
use crate::panel::layout::{is_expanded, layout, Ctx, Hit, Layout, ViewState};
use crate::settings::{self, Settings};
use crate::{fixture, log, state, timefmt, tooltip};
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Instant;
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_HOVER, TME_LEAVE, TRACKMOUSEEVENT, TRACKMOUSEEVENT_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetMessageW, KillTimer, PostMessageW, PostQuitMessage, RegisterWindowMessageW, SetForegroundWindow, SetTimer,
    TrackPopupMenu, TranslateMessage, MA_NOACTIVATE, MF_STRING, MSG, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WM_DESTROY, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEHOVER, WM_MOUSELEAVE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NULL,
    WM_RBUTTONUP, WM_TIMER,
};

const TIMER_TICK: usize = 1;
const TIMER_CLOSE: usize = 2;
const TIMER_ANIM: usize = 3;
const CMD_EXIT: u32 = 199;
const OPEN_MS: f32 = 180.0;
const CLOSE_MS: f32 = 140.0;
const HOVER_MS: u32 = 250;
const CLOSE_DELAY_MS: u32 = 300;
const STALE_USAGE_MS: i64 = 60_000;
const WHEEL_STEP: f32 = 44.0;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard.as_mut().map(f),
        Err(_) => None,
    })
}

#[derive(Clone, Copy)]
struct Anim {
    start: Instant,
    opening: bool,
}

struct App {
    controller: HWND,
    strip: HWND,
    panel: HWND,
    tray: Tray,
    taskbar_created: u32,
    settings: Settings,
    monitors: Vec<MonitorInfo>,
    fixture: Option<PathBuf>,
    collector: Collector,
    probe: WinProbe,
    worker: Option<Worker>,
    snapshot: Snapshot,
    light: Light,
    shown_strip: Option<(Rect, Colour, bool)>,
    renderer: Option<Renderer>,
    hover: Hover,
    view: ViewState,
    layout: Option<Layout>,
    panel_rect: Rect,
    panel_scale: f32,
    anim: Option<Anim>,
    strip_tracking: bool,
    panel_tracking: bool,
}

fn track(hwnd: HWND, flags: TRACKMOUSEEVENT_FLAGS, hover_ms: u32) {
    let mut t = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: flags,
        hwndTrack: hwnd,
        dwHoverTime: hover_ms,
    };
    unsafe {
        let _ = TrackMouseEvent(&mut t);
    }
}

fn lparam_xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as u16 as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32)
}

pub fn run() {
    let Some(_instance) = system::single_instance() else { return };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let settings_path = settings::settings_path(&system::exe_dir(), system::appdata_dir().as_deref());
    log::init(settings_path.with_file_name("claudehud.log"));
    let settings = settings::load(&settings_path);
    let hinst: HINSTANCE = match unsafe { GetModuleHandleW(None) } {
        Ok(m) => m.into(),
        Err(e) => {
            log::warn(&format!("GetModuleHandleW: {e}"));
            return;
        }
    };
    if !win::register_class(hinst, w!("ClaudeHUDController"), Some(controller_proc))
        || !win::register_class(hinst, w!("ClaudeHUDStrip"), Some(strip_proc))
        || !win::register_class(hinst, w!("ClaudeHUDPanel"), Some(panel_proc))
    {
        log::warn("RegisterClassExW failed");
        return;
    }
    let (Ok(controller), Ok(strip), Ok(panel)) = (
        win::create_controller(hinst, w!("ClaudeHUDController")),
        win::create_layered(hinst, w!("ClaudeHUDStrip")),
        win::create_layered(hinst, w!("ClaudeHUDPanel")),
    ) else {
        log::warn("CreateWindowExW failed");
        return;
    };
    let renderer = match Renderer::new(hinst) {
        Ok(r) => Some(r),
        Err(e) => {
            log::warn(&format!("panel disabled: {e}"));
            None
        }
    };
    let fixture = std::env::var_os("CLAUDEHUD_FIXTURE").map(PathBuf::from);
    let claude_dir = system::claude_dir();
    let worker = fixture.is_none().then(|| {
        Worker::spawn(controller, claude_dir.clone(), settings.usage_poll_s as u64, settings.status_poll_s as u64)
    });
    let app = App {
        controller,
        strip,
        panel,
        tray: Tray::new(controller),
        taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        settings,
        monitors: monitors::enumerate(),
        fixture,
        collector: Collector::new(claude_dir),
        probe: WinProbe::new(),
        worker,
        snapshot: Snapshot::default(),
        light: Light::off(),
        shown_strip: None,
        renderer,
        hover: Hover::default(),
        view: ViewState::default(),
        layout: None,
        panel_rect: Rect::default(),
        panel_scale: 1.0,
        anim: None,
        strip_tracking: false,
        panel_tracking: false,
    };
    APP.with(|cell| *cell.borrow_mut() = Some(app));
    with_app(App::tick);
    unsafe {
        SetTimer(Some(controller), TIMER_TICK, 1000, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    with_app(|a| a.tray.remove());
}

impl App {
    // ------------------------------------------------------------ data

    fn tick(&mut self) {
        if self.fixture.is_none() {
            self.collector.tick(timefmt::now_ms(), &self.probe, self.hover.visible);
        }
        self.refresh();
    }

    fn drain_worker(&mut self) {
        let now = timefmt::now_ms();
        if let Some(w) = &self.worker {
            while let Ok(m) = w.results.try_recv() {
                match m {
                    WorkerMsg::Usage(o) => self.collector.apply_usage(o, now),
                    WorkerMsg::Status(r) => self.collector.apply_status(r),
                }
            }
        }
    }

    fn refresh(&mut self) {
        let now = timefmt::now_ms();
        self.snapshot = match &self.fixture {
            Some(p) => match fixture::load_snapshot(p) {
                Ok(mut s) => {
                    fixture::anchor(&mut s, now);
                    s
                }
                Err(e) => {
                    log::warn(&e);
                    Snapshot { now_ms: now, ..Snapshot::default() }
                }
            },
            None => self.collector.snapshot(now, self.settings.warn_percent),
        };
        self.light = state::fold(&self.snapshot, Some(&self.light));
        self.update_strip();
        self.update_tray();
        if self.hover.visible {
            self.render_panel();
        }
    }

    fn monitor(&self) -> Option<MonitorInfo> {
        geometry::pick_monitor(&self.monitors, &self.settings.monitor).cloned()
    }

    fn update_strip(&mut self) {
        let Some(m) = self.monitor() else { return };
        self.hover.reveal_suppressed = geometry::edge_borders_other_monitor(&m, &self.monitors, self.settings.edge);
        if self.light.colour == Colour::Off {
            if self.shown_strip.take().is_some() {
                win::hide(self.strip);
            }
            return;
        }
        let r = geometry::strip_rect(&m, self.settings.edge);
        let key = (r, self.light.colour, self.light.dim);
        if self.shown_strip == Some(key) {
            win::set_topmost(self.strip);
            return;
        }
        let alpha = if self.light.dim { DIM_ALPHA } else { 1.0 };
        let px = icon::render_strip(r.w as u32, r.h as u32, self.light.colour.rgb(), alpha);
        if layered::present(self.strip, r, &px, 255) {
            win::show_noactivate(self.strip);
            self.shown_strip = Some(key);
        }
    }

    fn update_tray(&mut self) {
        let size = win::small_icon_size();
        let off = self.light.colour == Colour::Off;
        let alpha = if self.light.dim { DIM_ALPHA } else { 1.0 };
        let badge = (!off).then_some(Badge { rgb: self.light.colour.rgb(), alpha });
        let px = icon::render_tray_icon(size, badge, off);
        let tip = tooltip::tooltip(&self.light, &self.snapshot, &local_parts);
        self.tray.update(size, &px, &tip);
    }

    // ------------------------------------------------------------ panel

    /// Position and opacity for the current animation frame.
    fn frame(&self) -> (i32, i32, u8, bool) {
        let r = self.panel_rect;
        let Some(a) = self.anim else { return (r.x, r.y, 255, true) };
        let dur = if a.opening { OPEN_MS } else { CLOSE_MS };
        let t = (a.start.elapsed().as_secs_f32() * 1000.0 / dur).clamp(0.0, 1.0);
        let p = if a.opening { 1.0 - (1.0 - t).powi(3) } else { 1.0 - t };
        let (dx, dy) = geometry::slide_offset(self.settings.edge, p, self.panel_scale);
        (r.x + dx, r.y + dy, (p * 255.0).round() as u8, t >= 1.0)
    }

    /// Lays out and draws the panel, then shows it at the current frame.
    fn render_panel(&mut self) -> bool {
        let Some(m) = self.monitor() else { return false };
        let Some(r) = self.renderer.as_ref() else { return false };
        let strip = geometry::strip_rect(&m, self.settings.edge);
        let lay = {
            let measure = r.measure();
            let ctx = Ctx {
                snap: &self.snapshot,
                light: &self.light,
                view: &self.view,
                max_h: geometry::max_content_h(&m),
                measure: &measure,
                local: &local_parts,
            };
            layout(&ctx)
        };
        self.view.scroll = self.view.scroll.clamp(0.0, lay.scroll_max);
        self.panel_rect = geometry::panel_rect(&m, self.settings.edge, strip, lay.height);
        self.panel_scale = m.scale;
        let (x, y, alpha, _) = self.frame();
        let at = Rect { x, y, ..self.panel_rect };
        let panel = self.panel;
        let Some(r) = self.renderer.as_mut() else { return false };
        if let Err(e) = r.render(&lay, self.panel_rect, m.scale) {
            log::warn(&e);
            return false;
        }
        r.present(panel, at, alpha);
        self.layout = Some(lay);
        true
    }

    fn show_panel(&mut self) {
        let now = timefmt::now_ms();
        if self.fixture.is_none() {
            self.collector.tick(now, &self.probe, true); // pull sub-agents right away
            self.snapshot = self.collector.snapshot(now, self.settings.warn_percent);
            if self.collector.usage_age_ms(now).is_none_or(|age| age > STALE_USAGE_MS) {
                if let Some(w) = &self.worker {
                    w.refresh();
                }
            }
        }
        self.anim = Some(Anim { start: Instant::now(), opening: true });
        if !self.render_panel() {
            self.anim = None;
            self.hover.visible = false;
            return;
        }
        win::show_noactivate(self.panel);
        unsafe {
            SetTimer(Some(self.controller), TIMER_ANIM, 16, None);
        }
    }

    fn hide_panel(&mut self) {
        self.view.hovered = None;
        self.anim = Some(Anim { start: Instant::now(), opening: false });
        unsafe {
            SetTimer(Some(self.controller), TIMER_ANIM, 16, None);
        }
    }

    fn step_anim(&mut self) {
        let Some(a) = self.anim else {
            unsafe {
                let _ = KillTimer(Some(self.controller), TIMER_ANIM);
            }
            return;
        };
        let (x, y, alpha, done) = self.frame();
        layered::move_and_fade(self.panel, x, y, alpha);
        if done {
            self.anim = None;
            unsafe {
                let _ = KillTimer(Some(self.controller), TIMER_ANIM);
            }
            if !a.opening {
                win::hide(self.panel);
                self.layout = None;
                self.view.scroll = 0.0;
            }
        }
    }

    fn dispatch(&mut self, ev: Event) {
        for action in self.hover.step(ev) {
            match action {
                Action::Show => self.show_panel(),
                Action::Hide => self.hide_panel(),
                Action::StartCloseTimer => unsafe {
                    SetTimer(Some(self.controller), TIMER_CLOSE, CLOSE_DELAY_MS, None);
                },
                Action::CancelCloseTimer => unsafe {
                    let _ = KillTimer(Some(self.controller), TIMER_CLOSE);
                },
                Action::Acknowledge => {
                    self.collector.acknowledge();
                    self.refresh();
                }
                Action::PinChanged(p) => {
                    self.view.pinned = p;
                    if self.hover.visible {
                        self.render_panel();
                    }
                }
            }
        }
    }

    fn hit(&self, lp: LPARAM) -> Option<Hit> {
        let (px, py) = lparam_xy(lp);
        let s = self.panel_scale.max(0.1);
        let (x, y) = (px as f32 / s - geometry::SHADOW, py as f32 / s - geometry::SHADOW);
        self.layout.as_ref().and_then(|l| l.hit_at(x, y).cloned())
    }

    // ------------------------------------------------------------ window procs

    fn on_strip(&mut self, msg: u32, _wp: WPARAM, _lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_MOUSEMOVE => {
                if !self.strip_tracking {
                    track(self.strip, TME_HOVER | TME_LEAVE, HOVER_MS);
                    self.strip_tracking = true;
                    self.dispatch(Event::StripEnter);
                }
            }
            WM_MOUSEHOVER => self.dispatch(Event::StripHover),
            WM_MOUSELEAVE => {
                self.strip_tracking = false;
                self.dispatch(Event::StripLeave);
            }
            WM_LBUTTONUP => self.dispatch(Event::StripClick),
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn on_panel(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_MOUSEMOVE => {
                if !self.panel_tracking {
                    track(self.panel, TME_LEAVE, 0);
                    self.panel_tracking = true;
                    self.dispatch(Event::PanelEnter);
                }
                let h = self.hit(lp);
                if h != self.view.hovered {
                    self.view.hovered = h;
                    self.render_panel();
                }
            }
            WM_MOUSELEAVE => {
                self.panel_tracking = false;
                if self.view.hovered.take().is_some() && self.hover.visible {
                    self.render_panel();
                }
                self.dispatch(Event::PanelLeave);
            }
            WM_LBUTTONUP => match self.hit(lp) {
                Some(Hit::Pin) => self.dispatch(Event::PinClick),
                Some(Hit::Session(id)) | Some(Hit::SessionName(id)) => {
                    let current = self.snapshot.sessions.iter().find(|s| s.session_id == id).map(|s| is_expanded(&self.view, s));
                    if let Some(open) = current {
                        self.view.expanded.insert(id, !open);
                        self.render_panel();
                    }
                }
                Some(Hit::StatusLink) => system::open_url("https://status.claude.com"),
                None => {}
            },
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as u16 as i16;
                let max = self.layout.as_ref().map_or(0.0, |l| l.scroll_max);
                self.view.scroll = (self.view.scroll - delta as f32 / 120.0 * WHEEL_STEP).clamp(0.0, max);
                self.render_panel();
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn on_controller(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_TIMER if wp.0 == TIMER_TICK => self.tick(),
            WM_TIMER if wp.0 == TIMER_CLOSE => {
                unsafe {
                    let _ = KillTimer(Some(self.controller), TIMER_CLOSE);
                }
                self.dispatch(Event::CloseTimer);
            }
            WM_TIMER if wp.0 == TIMER_ANIM => self.step_anim(),
            WM_WORKER => {
                self.drain_worker();
                self.refresh();
            }
            WM_TRAY => match (lp.0 as u32) & 0xFFFF {
                WM_LBUTTONUP => self.dispatch(Event::TrayClick),
                WM_RBUTTONUP => self.show_menu(),
                _ => {}
            },
            m if self.taskbar_created != 0 && m == self.taskbar_created => {
                self.tray.reset();
                self.update_tray();
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn show_menu(&mut self) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT as usize, w!("Exit"));
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(self.controller);
            let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, Some(0), self.controller, None);
            let _ = PostMessageW(Some(self.controller), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            self.on_command(cmd.0 as u32);
        }
    }

    fn on_command(&mut self, cmd: u32) {
        if cmd == CMD_EXIT {
            unsafe {
                let _ = DestroyWindow(self.controller);
            }
        }
    }
}

extern "system" fn controller_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if let Some(Some(r)) = with_app(|a| a.on_controller(msg, wp, lp)) {
        return r;
    }
    if msg == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

extern "system" fn strip_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match with_app(|a| a.on_strip(msg, wp, lp)) {
        Some(Some(r)) => r,
        _ if msg == WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

extern "system" fn panel_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match with_app(|a| a.on_panel(msg, wp, lp)) {
        Some(Some(r)) => r,
        _ if msg == WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
```

`Option::is_none_or` needs Rust 1.82+. On an older toolchain write `self.collector.usage_age_ms(now).map_or(true, |age| age > STALE_USAGE_MS)`.

- [x] **Step 3: Build**

Run: `cargo build --release` and `cargo clippy --all-targets -- -D warnings`. Fix signature-level errors as described at the top.

- [x] **Step 4: Verify with a fixture, against the mockup**

Create `fixtures/manual/team_mockup.json`, a fixture that matches mockup section 1 (three sessions, three sub-agents, three limits, status operational):

```json
{"plan":"Team · Max 5x","warn_percent":85,
 "sessions":[
  {"pid":1,"session_id":"portal","name":"portal-service","cwd":"C:\\Projects\\Work\\portal-service","status":"waiting","waiting_for":"approve the permission prompt","started_at_ms":-2400000,"status_updated_at_ms":-20000,"transcript":{"model":"claude-opus-5","last_turn_input_tokens":84200}},
  {"pid":2,"session_id":"hud","name":"claudehud-a4","cwd":"C:\\Projects\\Personal\\ClaudeHUD","status":"busy","started_at_ms":-720000,"status_updated_at_ms":-60000,
   "transcript":{"model":"claude-opus-5","last_turn_input_tokens":132600},
   "subagents":[
     {"agent_id":"a","agent_type":"general-purpose","description":"Draft hover state machine tests","depth":1,"state":"running","started_ms":-185000,"context_tokens":41900},
     {"agent_id":"b","agent_type":"Explore","description":"Locate WinHTTP proxy handling","depth":1,"state":"running","started_ms":-72000,"context_tokens":18400},
     {"agent_id":"c","agent_type":"Explore","description":"Find per-monitor DPI examples","depth":1,"state":"done","started_ms":-300000,"ended_ms":-252000,"context_tokens":9100}]},
  {"pid":3,"session_id":"docs","name":"docs-site","cwd":"C:\\Projects\\Personal\\docs-site\\packages\\website","status":"idle","started_at_ms":-5400000,"status_updated_at_ms":-180000,"transcript":{"model":"claude-sonnet-5","last_turn_input_tokens":22100,"compacted":true}}],
 "usage":{"value":{"limits":[
   {"key":"five_hour","label":"Current session · 5h","pct":61,"resets_at":16320},
   {"key":"seven_day","label":"Weekly · all models","pct":88,"resets_at":257000},
   {"key":"seven_day_opus","label":"Weekly · Opus","pct":42,"resets_at":257000}]}},
 "status":{"value":{"description":"All Systems Operational","checked_at_ms":-120000,"components":[
   {"name":"Claude Code","state":"operational"},{"name":"Claude API (api.anthropic.com)","state":"operational"},{"name":"claude.ai","state":"operational"}]}}}
```

```powershell
cargo build --release
$env:CLAUDEHUD_FIXTURE = (Resolve-Path fixtures\manual\team_mockup.json).Path
Start-Process target\release\claudehud.exe
```

Ask the user to check each point:

1. Hover the yellow strip at the top. After ¼ s the panel slides down about 12 px while fading in, anchored under the strip, with rounded corners and a soft shadow. It has the app icon, "Team · Max 5x", "3 sessions · 1 needs you" and the pin.
2. Move the pointer away. The panel fades out about ⅓ s later. Move quickly across the strip without stopping: nothing opens.
3. Hover again, move down into the panel, hover "claudehud-a4". A dark tooltip "Working folder / C:\Projects\Personal\ClaudeHUD" appears below the name.
4. Click the "docs-site" row and it does not expand (no sub-agents). Click the "claudehud-a4" row: its three sub-agent rows collapse; click again and they return.
5. Click the pin, then move away: the panel stays. Click the pin again and move away: it closes.
6. Click the strip: the panel opens pinned. Click the strip again: it closes.
7. Left-click the tray icon: it toggles the pinned panel.
8. While the panel is open, the editor keeps keyboard focus: type in it.
9. Click "status.claude.com ↗": the browser opens the status page.

Capture it for your own check: `scripts\screenshot.ps1 -Region panel` (with the panel pinned), then view the PNG next to the mockup's section 1. Differences in font rendering are fine; layout, colours and ordering should match.

Also run once with `$env:CLAUDEHUD_FIXTURE = (Resolve-Path fixtures\snapshots\red_quota_spent.json).Path` and pin the panel. It should show the red banner "Weekly limit spent. New turns will fail until …".

- [x] **Step 5: Verify live** (partial — safe part only)

Clear the fixture, start the exe, and hover the strip while this Claude Code session works. This session should be listed as "Working · Nm" with its model and last-turn tokens. If this task spawned sub-agents they appear under it. Done: the live session (`claudehud-7e`) rendered as "Working · 3h 05m", Sonnet 5, last-turn tokens, with this very Task 18 sub-agent listed underneath as a running sub-agent.

Crash acknowledgement: kill a busy session (red strip), hover to open the panel, then close it. The crash row was visible while the panel was open and the strip returns to its normal colour afterwards. **Deliberately not done by this implementer** — killing another session's process is out of scope for a sub-agent and was deferred to the coordinator and user to do together; already tracked as item 7 in `docs/manual-qa-pending.md` ("From Task 18 (hover panel)"). Not duplicated here.

- [x] **Step 6: Commit**

```powershell
cargo fmt
git add src/platform fixtures/manual
git commit -m "feat(panel): Direct2D hover panel with pin, tooltip, expand, scroll"
```
