//! Terminal output in whole frames, and how much each frame took.
//!
//! Every frame is wrapped in a synchronized update (DEC mode 2026): the
//! terminal shows it once it arrived completely, never half drawn, and
//! renders it only once. Terminals without that mode ignore it.
//!
//! The bytes of each frame are counted, so animations can draw fewer
//! frames when frames are big (large windows): a terminal can take only
//! so much per second, and more makes it lag behind the keys.

use std::cell::Cell;
use std::io::{self, Write};
use std::rc::Rc;

const BEGIN_SYNC: &[u8] = b"\x1b[?2026h";
const END_SYNC: &[u8] = b"\x1b[?2026l";

pub struct Output<W: Write> {
    inner: W,
    /// A synchronized update is open (something was written since the last flush).
    open: bool,
    written: Rc<Cell<usize>>,
}

impl<W: Write> Output<W> {
    pub fn new(inner: W) -> Output<W> {
        Output { inner, open: false, written: Rc::default() }
    }

    /// Bytes written since the counter was last reset (`Cell::take`).
    pub fn counter(&self) -> Rc<Cell<usize>> {
        self.written.clone()
    }
}

impl<W: Write> Write for Output<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if !self.open {
            self.inner.write_all(BEGIN_SYNC)?;
            self.open = true;
        }
        self.inner.write_all(data)?;
        self.written.set(self.written.get() + data.len());
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.open {
            self.inner.write_all(END_SYNC)?;
            self.open = false;
        }
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_synchronized_and_counted() {
        let mut out = Output::new(Vec::new());
        let count = out.counter();
        write!(out, "one").unwrap();
        write!(out, "two").unwrap();
        out.flush().unwrap();
        out.flush().unwrap(); // nothing written: no empty update
        write!(out, "three").unwrap();
        out.flush().unwrap();
        assert_eq!(out.inner, b"\x1b[?2026honetwo\x1b[?2026l\x1b[?2026hthree\x1b[?2026l");
        assert_eq!(count.take(), 11);
        assert_eq!(count.get(), 0);
    }
}
