use crate::geometry::{MonitorInfo, Rect};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

// `BOOL` lives in windows::Win32::Foundation in some 0.6x releases and in windows::core in others.
use windows::core::BOOL;

fn rect(r: RECT) -> Rect {
    Rect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    }
}

unsafe extern "system" fn collect(
    hmon: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let out = &mut *(data.0 as *mut Vec<MonitorInfo>);
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmon, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let end = mi
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(mi.szDevice.len());
        let bounds = rect(mi.monitorInfo.rcMonitor);
        out.push(MonitorInfo {
            id: String::from_utf16_lossy(&mi.szDevice[..end]),
            name: format!("Display {} · {}×{}", out.len() + 1, bounds.w, bounds.h),
            primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            bounds,
            work: rect(mi.monitorInfo.rcWork),
            scale: dx as f32 / 96.0,
        });
    }
    true.into()
}

/// All monitors in physical pixels (the process is PerMonitorV2-aware via its manifest).
pub fn enumerate() -> Vec<MonitorInfo> {
    let mut out: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut out as *mut Vec<MonitorInfo> as isize),
        );
    }
    out
}
