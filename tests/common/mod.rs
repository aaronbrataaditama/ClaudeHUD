#![allow(dead_code)]
//! Shared helpers for integration tests. No external crates allowed, so this
//! replaces `tempfile`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("claudehud-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create temp dir");
        TempDir(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn write(&self, rel: &str, contents: &str) -> PathBuf {
        self.write_bytes(rel, contents.as_bytes())
    }

    pub fn write_bytes(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(rel);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).expect("create parent dir");
        }
        std::fs::write(&p, bytes).expect("write test file");
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
