//! Pushes premultiplied BGRA pixels to a WS_EX_LAYERED window with UpdateLayeredWindow.

use crate::geometry::Rect;
use std::ffi::c_void;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HDC,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

fn blend(alpha: u8) -> BLENDFUNCTION {
    BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    }
}

pub fn dib_info(w: i32, h: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Shows `bgra` (w*h*4 premultiplied bytes) at `r`, with overall opacity `alpha`.
pub fn present(hwnd: HWND, r: Rect, bgra: &[u8], alpha: u8) -> bool {
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bmi = dib_info(r.w, r.h);
        let ok = match CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bmp) => {
                let n = bgra.len().min((r.w * r.h * 4).max(0) as usize);
                std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, n);
                let old = SelectObject(mem, bmp.into());
                let ok = present_dc(hwnd, r, mem, alpha);
                SelectObject(mem, old);
                let _ = DeleteObject(bmp.into());
                ok
            }
            Err(_) => false,
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        ok
    }
}

/// Same, from a memory DC that already has a 32-bpp DIB selected (the panel renderer's).
pub fn present_dc(hwnd: HWND, r: Rect, dc: HDC, alpha: u8) -> bool {
    unsafe {
        UpdateLayeredWindow(
            hwnd,
            None,
            Some(&POINT { x: r.x, y: r.y }),
            Some(&SIZE { cx: r.w, cy: r.h }),
            Some(dc),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&blend(alpha)),
            ULW_ALPHA,
        )
        .is_ok()
    }
}

/// Moves and fades an already-presented window without re-sending pixels (animation frames).
pub fn move_and_fade(hwnd: HWND, x: i32, y: i32, alpha: u8) -> bool {
    unsafe {
        UpdateLayeredWindow(
            hwnd,
            None,
            Some(&POINT { x, y }),
            None,
            None,
            None,
            COLORREF(0),
            Some(&blend(alpha)),
            ULW_ALPHA,
        )
        .is_ok()
    }
}
