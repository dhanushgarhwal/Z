use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;

/// Shared between a worker thread and the UI. Lock-free except for the file name.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
    current: Mutex<String>,
}

impl Progress {
    pub fn new() -> Self { Self::default() }
    pub fn set_total(&self, n: u64) { self.total.store(n, Relaxed); }
    pub fn add(&self, n: u64) { self.done.fetch_add(n, Relaxed); }
    pub fn cancelled(&self) -> bool { self.cancel.load(Relaxed) }
    pub fn cancel(&self) { self.cancel.store(true, Relaxed); }
    pub fn set_current(&self, s: &str) { if let Ok(mut g) = self.current.lock() { g.clear(); g.push_str(s); } }
    pub fn current(&self) -> String { self.current.lock().map(|g| g.clone()).unwrap_or_default() }
    pub fn fraction(&self) -> f32 {
        let t = self.total.load(Relaxed);
        if t == 0 { 0.0 } else { (self.done.load(Relaxed) as f64 / t as f64).min(1.0) as f32 }
    }
}

/// Counts bytes read through it and aborts when cancelled.
pub struct CountingReader<'a, R> { pub inner: R, pub p: &'a Progress }

impl<'a, R: Read> Read for CountingReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.p.cancelled() { return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled")); }
        let n = self.inner.read(buf)?;
        self.p.add(n as u64);
        Ok(n)
    }
}
