//! HTTPS GET over WinHTTP: system proxy, OS certificate store, no TLS crate.
//! Error strings never include header values (they carry the OAuth token).

use super::wide::wide;
use crate::collect::{HttpGet, HttpResponse};
use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts, INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_QUERY_CUSTOM, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};

const MAX_BODY: usize = 1024 * 1024;

pub struct WinHttp {
    session: *mut c_void,
}

// Created and used on the worker thread only; moved there once at startup.
unsafe impl Send for WinHttp {}

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn last_error(what: &str) -> String {
    format!("{what} failed: {}", windows::core::Error::from_win32())
}

impl WinHttp {
    pub fn new(user_agent: &str) -> Result<WinHttp, String> {
        let ua = wide(user_agent);
        let session = unsafe {
            WinHttpOpen(
                PCWSTR(ua.as_ptr()),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        };
        if session.is_null() {
            return Err(last_error("WinHttpOpen"));
        }
        unsafe {
            let _ = WinHttpSetTimeouts(session, 5_000, 10_000, 10_000, 20_000);
        }
        Ok(WinHttp { session })
    }
}

impl Drop for WinHttp {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.session);
        }
    }
}

unsafe fn query_status(req: *mut c_void) -> Result<u16, String> {
    let mut code = 0u32;
    let mut len = std::mem::size_of::<u32>() as u32;
    let mut index = 0u32;
    WinHttpQueryHeaders(
        req,
        WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
        PCWSTR::null(),
        Some(&mut code as *mut u32 as *mut c_void),
        &mut len,
        &mut index,
    )
    .map_err(|e| format!("status: {e}"))?;
    Ok(code as u16)
}

unsafe fn query_header(req: *mut c_void, name: &str) -> Option<String> {
    let name_w = wide(name);
    let mut buf = vec![0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    let mut index = 0u32;
    WinHttpQueryHeaders(
        req,
        WINHTTP_QUERY_CUSTOM,
        PCWSTR(name_w.as_ptr()),
        Some(buf.as_mut_ptr() as *mut c_void),
        &mut len,
        &mut index,
    )
    .ok()?;
    Some(String::from_utf16_lossy(
        &buf[..(len as usize / 2).min(buf.len())],
    ))
}

impl HttpGet for WinHttp {
    fn get(
        &self,
        host: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, String> {
        unsafe {
            let host_w = wide(host);
            let conn = Handle(WinHttpConnect(
                self.session,
                PCWSTR(host_w.as_ptr()),
                INTERNET_DEFAULT_HTTPS_PORT,
                0,
            ));
            if conn.0.is_null() {
                return Err(last_error("connect"));
            }
            let verb = wide("GET");
            let object = wide(path);
            let req = Handle(WinHttpOpenRequest(
                conn.0,
                PCWSTR(verb.as_ptr()),
                PCWSTR(object.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ));
            if req.0.is_null() {
                return Err(last_error("open request"));
            }
            let header_text: String = headers
                .iter()
                .map(|(k, v)| format!("{k}: {v}\r\n"))
                .collect();
            let header_w: Vec<u16> = header_text.encode_utf16().collect();
            WinHttpSendRequest(req.0, Some(&header_w), None, 0, 0, 0)
                .map_err(|e| format!("send: {e}"))?;
            WinHttpReceiveResponse(req.0, std::ptr::null_mut())
                .map_err(|e| format!("receive: {e}"))?;
            let status = query_status(req.0)?;
            let retry_after_s =
                query_header(req.0, "Retry-After").and_then(|v| v.trim().parse().ok());
            let mut body = Vec::new();
            loop {
                let mut avail = 0u32;
                WinHttpQueryDataAvailable(req.0, &mut avail).map_err(|e| format!("read: {e}"))?;
                if avail == 0 {
                    break;
                }
                let mut buf = vec![0u8; avail as usize];
                let mut read = 0u32;
                WinHttpReadData(req.0, buf.as_mut_ptr() as *mut c_void, avail, &mut read)
                    .map_err(|e| format!("read: {e}"))?;
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&buf[..read as usize]);
                if body.len() > MAX_BODY {
                    return Err("response too large".to_string());
                }
            }
            Ok(HttpResponse {
                status,
                body: String::from_utf8_lossy(&body).into_owned(),
                retry_after_s,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hits the real status page. Run manually: `cargo test --lib live_status_page -- --ignored`
    #[test]
    #[ignore]
    fn live_status_page() {
        let http = WinHttp::new(crate::collect::USER_AGENT).unwrap();
        let s = crate::collect::fetch_status(&http, 0).unwrap();
        assert!(!s.components.is_empty(), "{s:?}");
    }
}
