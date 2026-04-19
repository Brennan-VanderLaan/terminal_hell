//! In-memory log buffer + tracing writer.
//!
//! `tracing_subscriber::fmt::Layer` is configured in `main.rs` to use this
//! buffer as its writer instead of stderr. Events land in a bounded ring;
//! the in-game log console reads from it via `global().recent(n)`.
//!
//! Without this, `println!` / `eprintln!` / `tracing::info!` output would
//! be clobbered by the TUI render loop — users can't see connect errors,
//! STUN failures, etc. while the game is running.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

const DEFAULT_CAPACITY: usize = 1024;

#[derive(Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<VecDeque<String>>>,
    max_lines: usize,
}

static GLOBAL: OnceLock<LogBuffer> = OnceLock::new();

impl LogBuffer {
    pub fn install(max_lines: usize) -> Self {
        let buf = Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(max_lines.min(4096)))),
            max_lines,
        };
        let _ = GLOBAL.set(buf.clone());
        buf
    }

    pub fn global() -> Option<&'static LogBuffer> {
        GLOBAL.get()
    }

    pub fn push_line(&self, line: &str) {
        let mut buf = match self.inner.lock() {
            Ok(b) => b,
            Err(p) => p.into_inner(),
        };
        buf.push_back(line.to_string());
        while buf.len() > self.max_lines {
            buf.pop_front();
        }
    }

    /// Return the last `n` lines in chronological order (oldest first).
    pub fn recent(&self, n: usize) -> Vec<String> {
        let buf = match self.inner.lock() {
            Ok(b) => b,
            Err(p) => p.into_inner(),
        };
        let skip = buf.len().saturating_sub(n);
        buf.iter().skip(skip).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|b| b.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::install(DEFAULT_CAPACITY)
    }
}

/// Writer returned from `MakeWriter::make_writer`. Accumulates bytes from
/// a single tracing event and commits them as complete lines on drop.
pub struct LogWriter {
    buffer: Arc<Mutex<VecDeque<String>>>,
    max_lines: usize,
    pending: Vec<u8>,
}

impl std::io::Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for LogWriter {
    fn drop(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&self.pending).into_owned();
        let mut buf = match self.buffer.lock() {
            Ok(b) => b,
            Err(p) => p.into_inner(),
        };
        for raw_line in text.split('\n') {
            let trimmed = raw_line.trim_end_matches('\r').trim_end();
            if trimmed.is_empty() {
                continue;
            }
            buf.push_back(trimmed.to_string());
            while buf.len() > self.max_lines {
                buf.pop_front();
            }
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
    type Writer = LogWriter;
    fn make_writer(&'a self) -> Self::Writer {
        LogWriter {
            buffer: Arc::clone(&self.inner),
            max_lines: self.max_lines,
            pending: Vec::new(),
        }
    }
}
