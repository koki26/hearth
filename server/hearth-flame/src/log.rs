//! The server's log, kept in memory so a window can show it.
//!
//! It is a `MakeWriter`, so `tracing_subscriber::fmt` formats lines exactly as
//! it does for a console and we store the result. The console server prints the
//! same lines; the window has nowhere else to print them.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};

/// Lines kept before the oldest is dropped. A busy evening is a few thousand.
pub const MAX_LINES: usize = 2000;

#[derive(Clone, Default)]
pub struct Log(Arc<Mutex<Inner>>);

#[derive(Default)]
struct Inner {
    lines: VecDeque<String>,
    /// A write may stop mid-line; it is held here until the newline arrives.
    pending: String,
    /// Counts everything ever written, so a reader can tell there was more.
    dropped: usize,
}

impl Log {
    pub fn new() -> Self {
        Self::default()
    }

    /// Oldest first.
    pub fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().lines.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.0.lock().unwrap().lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many lines fell off the end.
    pub fn dropped(&self) -> usize {
        self.0.lock().unwrap().dropped
    }

    /// For what a person typed and what it answered: those are part of the
    /// story of a session, so they belong in the same place as the events.
    pub fn push(&self, line: impl Into<String>) {
        let mut inner = self.0.lock().unwrap();
        for part in line.into().lines() {
            inner.add(part.to_owned());
        }
    }

    pub fn clear(&self) {
        let mut inner = self.0.lock().unwrap();
        inner.lines.clear();
        inner.pending.clear();
    }
}

impl Inner {
    fn add(&mut self, line: String) {
        if self.lines.len() >= MAX_LINES {
            self.lines.pop_front();
            self.dropped += 1;
        }
        self.lines.push_back(line);
    }
}

pub struct Writer(Log);

impl io::Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.0.0.lock().unwrap();
        inner.pending.push_str(&String::from_utf8_lossy(buf));
        while let Some(end) = inner.pending.find('\n') {
            let line: String = inner.pending.drain(..=end).collect();
            inner.add(line.trim_end().to_owned());
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Log {
    type Writer = Writer;

    fn make_writer(&'a self) -> Self::Writer {
        Writer(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn whole_lines_land_in_order_however_they_arrive() {
        let log = Log::new();
        let mut writer = Writer(log.clone());
        writer.write_all(b"first line\nsecond ").unwrap();
        assert_eq!(log.lines(), ["first line"], "a part line waits for its newline");
        writer.write_all(b"line\nthird\n").unwrap();
        assert_eq!(log.lines(), ["first line", "second line", "third"]);

        log.push("a typed command\nand its answer");
        assert_eq!(log.lines().len(), 5);
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn the_oldest_lines_fall_off_and_are_counted() {
        let log = Log::new();
        for i in 0..MAX_LINES + 10 {
            log.push(format!("line {i}"));
        }
        assert_eq!(log.len(), MAX_LINES);
        assert_eq!(log.dropped(), 10);
        assert_eq!(log.lines()[0], "line 10", "the first ten are gone");
    }
}
