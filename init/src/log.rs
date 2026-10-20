//! fern-log: the structured log store.
//!
//! Records live in an in-memory ring for the current boot and are appended
//! as JSON lines to `/var/log/fern/log.jsonl` (rotated to `.1` at 16 MiB).

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const LOG_FILE: &str = "var/log/fern/log.jsonl";
pub const RING: usize = 20_000;
pub const ROTATE_AT: u64 = 16 << 20;
/// Longer lines are split.
pub const MAX_LINE: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Unix time, milliseconds.
    pub t: u64,
    pub unit: String,
    /// `out`, `err`, or `init` (messages from fern-init itself).
    pub stream: String,
    pub msg: String,
}

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[derive(Debug)]
pub struct LogStore {
    ring: VecDeque<Record>,
    path: Option<PathBuf>,
    file: Option<File>,
    written: u64,
    /// Echo every record to stderr (the console) as well.
    pub echo: bool,
}

impl LogStore {
    pub fn memory_only() -> Self {
        LogStore {
            ring: VecDeque::new(),
            path: None,
            file: None,
            written: 0,
            echo: false,
        }
    }

    /// Starts persisting to `path` (creating its directory). Records already
    /// in the ring are written first, so early-boot messages are kept.
    ///
    /// # Errors
    /// I/O errors opening the file.
    pub fn persist_to(&mut self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            fs::create_dir_all(d)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        self.written = file.metadata().map_or(0, |m| m.len());
        self.path = Some(path.to_path_buf());
        self.file = Some(file);
        let early: Vec<Record> = self.ring.iter().cloned().collect();
        for r in &early {
            self.write_line(r);
        }
        Ok(())
    }

    fn write_line(&mut self, r: &Record) {
        let Some(f) = self.file.as_mut() else { return };
        let mut line = serde_json::to_string(r).unwrap_or_default();
        line.push('\n');
        if f.write_all(line.as_bytes()).is_ok() {
            self.written += line.len() as u64;
        }
        if self.written >= ROTATE_AT
            && let Some(p) = self.path.clone()
        {
            let old = p.with_extension("jsonl.1");
            let _ = fs::rename(&p, &old);
            self.file = OpenOptions::new().create(true).append(true).open(&p).ok();
            self.written = 0;
        }
    }

    pub fn push(&mut self, unit: &str, stream: &str, msg: &str) {
        for chunk in split_utf8(msg, MAX_LINE) {
            let r = Record {
                t: unix_ms(),
                unit: unit.to_string(),
                stream: stream.to_string(),
                msg: chunk.to_string(),
            };
            if self.echo {
                eprintln!("[{}] {}", r.unit, r.msg);
            }
            self.write_line(&r);
            if self.ring.len() >= RING {
                self.ring.pop_front();
            }
            self.ring.push_back(r);
        }
    }

    /// The last `n` records, optionally for one unit, oldest first.
    pub fn tail(&self, unit: Option<&str>, n: usize) -> Vec<Record> {
        let mut v: Vec<Record> = self
            .ring
            .iter()
            .rev()
            .filter(|r| unit.is_none_or(|u| r.unit == u))
            .take(n)
            .cloned()
            .collect();
        v.reverse();
        v
    }

    pub fn flush(&mut self) {
        if let Some(f) = self.file.as_mut() {
            let _ = f.sync_data();
        }
    }
}

fn split_utf8(s: &str, max: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while rest.len() > max {
        let mut cut = max;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        out.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    out.push(rest);
    out
}

/// Splits a byte stream from a service into lines.
#[derive(Debug, Default)]
pub struct LineBuffer {
    buf: Vec<u8>,
}

impl LineBuffer {
    /// Feeds bytes; returns the complete lines (lossy UTF-8, no newline).
    pub fn feed(&mut self, data: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        while let Some(i) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=i).collect();
            out.push(String::from_utf8_lossy(&line[..line.len() - 1]).into_owned());
        }
        if self.buf.len() > MAX_LINE {
            let line = std::mem::take(&mut self.buf);
            out.push(String::from_utf8_lossy(&line).into_owned());
        }
        out
    }

    /// Whatever is left when the stream closes.
    pub fn finish(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            None
        } else {
            let line = std::mem::take(&mut self.buf);
            Some(String::from_utf8_lossy(&line).into_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_and_tail() {
        let mut s = LogStore::memory_only();
        s.push("a", "out", "one");
        s.push("b", "err", "two");
        s.push("a", "out", "three");
        let a: Vec<String> = s.tail(Some("a"), 10).into_iter().map(|r| r.msg).collect();
        assert_eq!(a, ["one", "three"]);
        assert_eq!(s.tail(None, 1)[0].msg, "three");
    }

    #[test]
    fn persists_early_records_and_rotates() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("log/fern.jsonl");
        let mut s = LogStore::memory_only();
        s.push("init", "init", "before /var");
        s.persist_to(&p).unwrap();
        s.push("a", "out", "after");
        let text = fs::read_to_string(&p).unwrap();
        let recs: Vec<Record> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].msg, "before /var");

        s.written = ROTATE_AT;
        s.push("a", "out", "trigger");
        assert!(p.with_extension("jsonl.1").exists());
    }

    #[test]
    fn long_lines_split_on_char_boundaries() {
        let mut s = LogStore::memory_only();
        let long = "é".repeat(MAX_LINE); // 2 bytes each
        s.push("a", "out", &long);
        let recs = s.tail(None, 100);
        assert_eq!(recs.len(), 2);
        assert!(recs.iter().all(|r| r.msg.len() <= MAX_LINE));
    }

    #[test]
    fn line_buffer() {
        let mut b = LineBuffer::default();
        assert!(b.feed(b"hel").is_empty());
        assert_eq!(b.feed(b"lo\nwor"), ["hello"]);
        assert_eq!(b.feed(b"ld\n\n"), ["world", ""]);
        assert_eq!(b.feed(b"tail"), Vec::<String>::new());
        assert_eq!(b.finish().as_deref(), Some("tail"));
        assert_eq!(b.feed(&vec![b'x'; MAX_LINE + 1]).len(), 1);
    }
}
