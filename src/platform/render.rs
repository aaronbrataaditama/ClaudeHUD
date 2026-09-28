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
use windows::Win32::Foundation::{HINSTANCE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, DIB_RGB_COLORS,
    HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut,
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

// `windows_numerics::Vector2` (the field type of `D2D1_ELLIPSE.point` and the
// parameter type of `DrawLine`) isn't reachable through any public path in the
// `windows` 0.61 crate (`windows::Foundation::Numerics` doesn't re-export it, and
// `windows_numerics` itself isn't a direct dependency of this crate). A macro
// sidesteps that: it never spells the type's name, it only ever holds a value of
// it (via `D2D1_ELLIPSE::default().point`, which the compiler infers), so no new
// dependency is needed.
macro_rules! pt {
    ($x:expr, $y:expr) => {{
        let mut v = D2D1_ELLIPSE::default().point;
        v.X = $x + SHADOW;
        v.Y = $y + SHADOW;
        v
    }};
}

fn d2r(r: RectF) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x + SHADOW,
        top: r.y + SHADOW,
        right: r.right() + SHADOW,
        bottom: r.bottom() + SHADOW,
    }
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
            match CreateDIBSection(
                Some(dc),
                &dib_info(w, h),
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            ) {
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

fn make_target(
    f: &ID2D1Factory,
    kind: D2D1_RENDER_TARGET_TYPE,
) -> windows::core::Result<ID2D1DCRenderTarget> {
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: kind,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 0.0,
        dpiY: 0.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    unsafe { f.CreateDCRenderTarget(&props) }
}

fn make_format(
    dw: &IDWriteFactory,
    family: PCWSTR,
    weight: DWRITE_FONT_WEIGHT,
    size: f32,
) -> Result<IDWriteTextFormat, String> {
    unsafe {
        let f = dw
            .CreateTextFormat(
                family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-us"),
            )
            .map_err(|e| format!("CreateTextFormat: {e}"))?;
        let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        if let Ok(sign) = dw.CreateEllipsisTrimmingSign(&f) {
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            let _ = f.SetTrimming(&trimming, &sign);
        }
        Ok(f)
    }
}

impl Renderer {
    pub fn new(hinst: HINSTANCE) -> Result<Renderer, String> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                .map_err(|e| format!("D2D1CreateFactory: {e}"))?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
                .map_err(|e| format!("DWriteCreateFactory: {e}"))?;
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("WIC: {e}"))?;
            let (target, software) = match make_target(&factory, D2D1_RENDER_TARGET_TYPE_DEFAULT) {
                Ok(t) => (t, false),
                Err(_) => (
                    make_target(&factory, D2D1_RENDER_TARGET_TYPE_SOFTWARE)
                        .map_err(|e| format!("CreateDCRenderTarget: {e}"))?,
                    true,
                ),
            };
            let brush = target
                .CreateSolidColorBrush(&colour(0xFFFFFF, 1.0), None)
                .map_err(|e| format!("brush: {e}"))?;
            let ui = w!("Segoe UI Variable Text");
            let mut formats = HashMap::new();
            for (font, family, weight, size) in [
                (Font::Title, ui, DWRITE_FONT_WEIGHT_SEMI_BOLD, 14.0),
                (Font::Body, ui, DWRITE_FONT_WEIGHT_NORMAL, 13.0),
                (Font::BodyStrong, ui, DWRITE_FONT_WEIGHT_SEMI_BOLD, 13.0),
                (Font::Small, ui, DWRITE_FONT_WEIGHT_NORMAL, 12.0),
                (Font::SmallStrong, ui, DWRITE_FONT_WEIGHT_SEMI_BOLD, 12.0),
                (Font::Caption, ui, DWRITE_FONT_WEIGHT_NORMAL, 11.0),
                (
                    Font::Mono,
                    w!("Cascadia Mono"),
                    DWRITE_FONT_WEIGHT_NORMAL,
                    12.0,
                ),
            ] {
                formats.insert(font, make_format(&dwrite, family, weight, size)?);
            }
            Ok(Renderer {
                factory,
                dwrite,
                wic,
                target,
                brush,
                formats,
                surface: None,
                icon: None,
                hinst,
                software,
            })
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
                .BindDC(
                    dc,
                    &RECT {
                        left: 0,
                        top: 0,
                        right: window.w,
                        bottom: window.h,
                    },
                )
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
        let kind = if self.software {
            D2D1_RENDER_TARGET_TYPE_SOFTWARE
        } else {
            D2D1_RENDER_TARGET_TYPE_DEFAULT
        };
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
        let rr = D2D1_ROUNDED_RECT {
            rect: d2r(r),
            radiusX: radius,
            radiusY: radius,
        };
        self.target.FillRoundedRectangle(&rr, &self.brush);
    }

    unsafe fn stroke_round(&self, r: RectF, radius: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        let rr = D2D1_ROUNDED_RECT {
            rect: d2r(inflate(r, -0.5)),
            radiusX: radius - 0.5,
            radiusY: radius - 0.5,
        };
        self.target
            .DrawRoundedRectangle(&rr, &self.brush, 1.0, None);
    }

    unsafe fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, rgb: u32, width: f32) {
        self.set(rgb, 1.0);
        self.target
            .DrawLine(pt!(x0, y0), pt!(x1, y1), &self.brush, width, None);
    }

    unsafe fn dot(&self, cx: f32, cy: f32, r: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        let e = D2D1_ELLIPSE {
            point: pt!(cx, cy),
            radiusX: r,
            radiusY: r,
        };
        self.target.FillEllipse(&e, &self.brush);
    }

    unsafe fn text(&self, t: &TextOp) {
        let Some(f) = self.formats.get(&t.font) else {
            return;
        };
        let align = if t.align == Align::Right {
            DWRITE_TEXT_ALIGNMENT_TRAILING
        } else {
            DWRITE_TEXT_ALIGNMENT_LEADING
        };
        let _ = f.SetTextAlignment(align);
        self.set(ink_rgb(t.ink), 1.0);
        let s: Vec<u16> = t.text.encode_utf16().collect();
        self.target.DrawText(
            &s,
            f,
            &d2r(t.rect),
            &self.brush,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            DWRITE_MEASURING_MODE_NATURAL,
        );
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
            // resource id 1 = assets/claudehud.ico (Task 14); MAKEINTRESOURCEW-style integer
            // resource id encoded as a pointer value, not a real dangling pointer.
            #[allow(clippy::manual_dangling_ptr)]
            let hicon = LoadIconWithScaleDown(
                Some(self.hinst),
                PCWSTR(1usize as *const u16),
                px as i32,
                px as i32,
            )
            .ok()?;
            let wic_bmp = self.wic.CreateBitmapFromHICON(hicon);
            let _ = DestroyIcon(hicon);
            let conv = self.wic.CreateFormatConverter().ok()?;
            conv.Initialize(
                &wic_bmp.ok()?,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )
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
            Op::Dot {
                cx,
                cy,
                r,
                rgb,
                alpha,
            } => self.dot(*cx, *cy, *r, *rgb, *alpha),
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
                    self.target.DrawBitmap(
                        &bmp,
                        Some(&d2r(*rect)),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    );
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
                let head = D2D1_ELLIPSE {
                    point: pt!(cx, cy - 3.0),
                    radiusX: 3.0,
                    radiusY: 3.0,
                };
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
            Op::ClipPush(r) => self
                .target
                .PushAxisAlignedClip(&d2r(*r), D2D1_ANTIALIAS_MODE_ALIASED),
            Op::ClipPop => self.target.PopAxisAlignedClip(),
            Op::Tooltip { rect, title, body } => {
                self.fill_round(*rect, 6.0, 0x2E3036, 1.0);
                self.stroke_round(*rect, 6.0, 0xFFFFFF, 0.12);
                let t = TextOp {
                    rect: RectF::new(rect.x + 8.0, rect.y + 4.0, rect.w - 16.0, 14.0),
                    text: title.clone(),
                    font: Font::Caption,
                    ink: Ink::Muted,
                    align: Align::Left,
                };
                self.text(&t);
                let b = TextOp {
                    rect: RectF::new(rect.x + 8.0, rect.y + 19.0, rect.w - 16.0, 16.0),
                    text: body.clone(),
                    font: Font::Mono,
                    ink: Ink::Primary,
                    align: Align::Left,
                };
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
        let Some(f) = self.r.formats.get(&font) else {
            return text.chars().count() as f32 * 7.0;
        };
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
