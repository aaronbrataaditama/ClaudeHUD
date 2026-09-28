//! Shell_NotifyIcon wrapper (legacy callback mode: lParam carries the mouse message).

use super::layered::dib_info;
use std::ffi::c_void;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{CreateBitmap, CreateDIBSection, DeleteObject, DIB_RGB_COLORS};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, HICON, ICONINFO, WM_APP,
};

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;

/// Icons want straight (non-premultiplied) alpha.
fn unpremultiply(src: &[u8]) -> Vec<u8> {
    let mut out = src.to_vec();
    for px in out.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    out
}

pub fn icon_from_bgra(size: u32, premultiplied: &[u8]) -> Option<HICON> {
    let straight = unpremultiply(premultiplied);
    let s = size as i32;
    unsafe {
        let mut bits: *mut c_void = std::ptr::null_mut();
        let color =
            CreateDIBSection(None, &dib_info(s, s), DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        std::ptr::copy_nonoverlapping(
            straight.as_ptr(),
            bits as *mut u8,
            straight.len().min((s * s * 4) as usize),
        );
        let mask_bytes = vec![0u8; (((s + 15) / 16) * 2 * s) as usize];
        let mask = CreateBitmap(s, s, 1, 1, Some(mask_bytes.as_ptr() as *const c_void));
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

fn copy_wide(dst: &mut [u16], s: &str) {
    let max = dst.len() - 1;
    for (i, c) in s.encode_utf16().take(max).enumerate() {
        dst[i] = c;
    }
}

pub struct Tray {
    hwnd: HWND,
    icon: Option<HICON>,
    added: bool,
    last: Option<(Vec<u8>, String)>,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Tray {
        Tray {
            hwnd,
            icon: None,
            added: false,
            last: None,
        }
    }

    fn base(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            ..Default::default()
        }
    }

    /// Adds or updates the icon. Cheap when nothing changed.
    pub fn update(&mut self, size: u32, bgra: &[u8], tip: &str) {
        if self.added
            && self
                .last
                .as_ref()
                .is_some_and(|(p, t)| p == bgra && t == tip)
        {
            return;
        }
        let Some(icon) = icon_from_bgra(size, bgra) else {
            return;
        };
        let mut nid = self.base();
        nid.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = icon;
        copy_wide(&mut nid.szTip, tip);
        let ok = unsafe { Shell_NotifyIconW(if self.added { NIM_MODIFY } else { NIM_ADD }, &nid) }
            .as_bool();
        if ok {
            self.added = true;
            self.last = Some((bgra.to_vec(), tip.to_string()));
        }
        if let Some(old) = self.icon.replace(icon) {
            unsafe {
                let _ = DestroyIcon(old);
            }
        }
    }

    /// A one-off balloon notification (first run, panel unavailable).
    pub fn notify(&mut self, title: &str, text: &str) {
        if !self.added {
            return;
        }
        let mut nid = self.base();
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        copy_wide(&mut nid.szInfoTitle, title);
        copy_wide(&mut nid.szInfo, text);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    /// Explorer restarted (TaskbarCreated): the icon must be added again.
    pub fn reset(&mut self) {
        self.added = false;
        self.last = None;
    }

    pub fn remove(&mut self) {
        if self.added {
            let nid = self.base();
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            }
            self.added = false;
        }
        if let Some(i) = self.icon.take() {
            unsafe {
                let _ = DestroyIcon(i);
            }
        }
    }
}
