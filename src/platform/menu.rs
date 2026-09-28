//! The native tray context menu (§5). Returns the chosen command id (0 = dismissed).

use super::wide::wide;
use crate::geometry::MonitorInfo;
use crate::settings::{Edge, Settings};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW,
    SetForegroundWindow, SetMenuDefaultItem, TrackPopupMenu, HMENU, MENU_ITEM_FLAGS, MF_BYCOMMAND,
    MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_NULL,
};

pub const CMD_TOGGLE: u32 = 100;
pub const CMD_EDGE_TOP: u32 = 110;
pub const CMD_EDGE_LEFT: u32 = 111;
pub const CMD_MONITOR_PRIMARY: u32 = 120;
/// 121..=129: one per monitor in enumeration order
pub const CMD_MONITOR_FIRST: u32 = 121;
pub const MAX_MONITORS: usize = 9;
pub const CMD_WARN_80: u32 = 130;
pub const CMD_WARN_85: u32 = 131;
pub const CMD_WARN_90: u32 = 132;
pub const CMD_AUTOSTART: u32 = 140;
pub const CMD_STATUS_PAGE: u32 = 150;
pub const CMD_EXIT: u32 = 199;

pub struct MenuState<'a> {
    pub panel_visible: bool,
    pub settings: &'a Settings,
    pub monitors: &'a [MonitorInfo],
}

unsafe fn item(menu: HMENU, flags: MENU_ITEM_FLAGS, id: u32, text: &str) {
    let t = wide(text);
    let _ = AppendMenuW(menu, flags, id as usize, PCWSTR(t.as_ptr()));
}

unsafe fn submenu(menu: HMENU, sub: HMENU, text: &str) {
    let t = wide(text);
    let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(t.as_ptr()));
}

pub fn popup(owner: HWND, st: &MenuState) -> u32 {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return 0;
        };
        item(
            menu,
            MF_STRING,
            CMD_TOGGLE,
            if st.panel_visible {
                "Hide panel"
            } else {
                "Show panel"
            },
        );
        let _ = SetMenuDefaultItem(menu, CMD_TOGGLE, 0);
        item(menu, MF_SEPARATOR, 0, "");

        if let Ok(edge) = CreatePopupMenu() {
            item(edge, MF_STRING, CMD_EDGE_TOP, "Top");
            item(edge, MF_STRING, CMD_EDGE_LEFT, "Left");
            let sel = if st.settings.edge == Edge::Top {
                CMD_EDGE_TOP
            } else {
                CMD_EDGE_LEFT
            };
            let _ = CheckMenuRadioItem(edge, CMD_EDGE_TOP, CMD_EDGE_LEFT, sel, MF_BYCOMMAND.0);
            submenu(menu, edge, "Edge");
        }

        if let Ok(mons) = CreatePopupMenu() {
            item(mons, MF_STRING, CMD_MONITOR_PRIMARY, "Primary display");
            let shown = st.monitors.iter().take(MAX_MONITORS).enumerate();
            let mut sel = CMD_MONITOR_PRIMARY;
            let mut last = CMD_MONITOR_PRIMARY;
            for (i, m) in shown {
                let id = CMD_MONITOR_FIRST + i as u32;
                item(mons, MF_STRING, id, &m.name);
                if st.settings.monitor == m.id {
                    sel = id;
                }
                last = id;
            }
            let _ = CheckMenuRadioItem(mons, CMD_MONITOR_PRIMARY, last, sel, MF_BYCOMMAND.0);
            submenu(menu, mons, "Monitor");
        }

        if let Ok(warn) = CreatePopupMenu() {
            item(warn, MF_STRING, CMD_WARN_80, "80%");
            item(warn, MF_STRING, CMD_WARN_85, "85%");
            item(warn, MF_STRING, CMD_WARN_90, "90%");
            let sel = match st.settings.warn_percent {
                80 => Some(CMD_WARN_80),
                85 => Some(CMD_WARN_85),
                90 => Some(CMD_WARN_90),
                _ => None,
            };
            if let Some(sel) = sel {
                let _ = CheckMenuRadioItem(warn, CMD_WARN_80, CMD_WARN_90, sel, MF_BYCOMMAND.0);
            }
            // Text after a tab is drawn right-aligned by Windows.
            submenu(
                menu,
                warn,
                &format!("Warn at\t{}%", st.settings.warn_percent),
            );
        }

        item(menu, MF_SEPARATOR, 0, "");
        let check = if st.settings.autostart {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        item(menu, MF_STRING | check, CMD_AUTOSTART, "Run on startup");
        item(menu, MF_STRING, CMD_STATUS_PAGE, "Open status page\t↗");
        item(menu, MF_SEPARATOR, 0, "");
        item(menu, MF_STRING, CMD_EXIT, "Exit");

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Required so the menu closes when the user clicks elsewhere (documented tray quirk).
        let _ = SetForegroundWindow(owner);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            pt.x,
            pt.y,
            Some(0),
            owner,
            None,
        );
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu); // destroys the submenus too
        cmd.0 as u32
    }
}
