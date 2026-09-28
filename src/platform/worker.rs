//! Polls usage and service status on the Schedule, off the UI thread (§2).
//! Results go through a channel; a posted WM_WORKER wakes the UI thread.

use super::http::WinHttp;
use crate::collect::{fetch_status, fetch_usage, Schedule, UsageOutcome, USER_AGENT};
use crate::model::ServiceStatus;
use crate::{log, timefmt};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const WM_WORKER: u32 = WM_APP + 2;

pub enum WorkerMsg {
    Usage(UsageOutcome),
    Status(Result<ServiceStatus, String>),
}

enum WorkerCmd {
    Refresh,
    Polls { usage_s: u64, status_s: u64 },
}

pub struct Worker {
    cmds: Sender<WorkerCmd>,
    pub results: Receiver<WorkerMsg>,
}

impl Worker {
    pub fn spawn(notify: HWND, claude_dir: PathBuf, usage_s: u64, status_s: u64) -> Worker {
        let (cmd_tx, cmd_rx) = channel();
        let (res_tx, res_rx) = channel();
        // HWND is not Send; pass the raw value and rebuild it on the worker.
        let target = notify.0 as isize;
        let spawned = std::thread::Builder::new()
            .name("claudehud-worker".to_string())
            .spawn(move || worker_loop(target, claude_dir, usage_s, status_s, cmd_rx, res_tx));
        if let Err(e) = spawned {
            log::warn(&format!("could not start worker thread: {e}"));
        }
        Worker {
            cmds: cmd_tx,
            results: res_rx,
        }
    }

    /// Resume, unlock, or panel opened with stale data. Respects rate-limit backoff.
    pub fn refresh(&self) {
        let _ = self.cmds.send(WorkerCmd::Refresh);
    }

    pub fn set_polls(&self, usage_s: u64, status_s: u64) {
        let _ = self.cmds.send(WorkerCmd::Polls { usage_s, status_s });
    }
}

fn post(target: isize) {
    unsafe {
        let _ = PostMessageW(
            Some(HWND(target as *mut c_void)),
            WM_WORKER,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

fn worker_loop(
    target: isize,
    claude_dir: PathBuf,
    mut usage_s: u64,
    mut status_s: u64,
    cmds: Receiver<WorkerCmd>,
    out: Sender<WorkerMsg>,
) {
    let http = match WinHttp::new(USER_AGENT) {
        Ok(h) => h,
        Err(e) => {
            log::warn(&format!("worker: {e}"));
            return;
        }
    };
    let mut sched = Schedule::new(timefmt::now_ms());
    loop {
        let now = timefmt::now_ms();
        if sched.usage_due(now) {
            let o = fetch_usage(&claude_dir, &http, now);
            sched.after_usage(now, &o.result, usage_s);
            if out.send(WorkerMsg::Usage(o)).is_err() {
                return;
            }
            post(target);
        }
        let now = timefmt::now_ms();
        if sched.status_due(now) {
            let r = fetch_status(&http, now);
            sched.after_status(now, r.is_ok(), status_s);
            if out.send(WorkerMsg::Status(r)).is_err() {
                return;
            }
            post(target);
        }
        let wait_ms = (sched.next_wake_ms() - timefmt::now_ms()).clamp(250, 3_600_000) as u64;
        match cmds.recv_timeout(Duration::from_millis(wait_ms)) {
            Ok(WorkerCmd::Refresh) => sched.refresh_now(timefmt::now_ms()),
            Ok(WorkerCmd::Polls {
                usage_s: u,
                status_s: s,
            }) => {
                usage_s = u;
                status_s = s;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}
