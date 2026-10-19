use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

pub const RECORD_RING_FRAMES: usize = 4 * 48_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecordFrame {
    Audio([f32; 2]),
    Gap(u32),
    Rate(u32),
}

#[derive(Default)]
pub struct RecordShared {
    started: AtomicBool,
    start_clock: AtomicU64,
    dropped: AtomicU64,
}

impl RecordShared {
    /// Engine clock, in seconds, of the first recorded frame.
    pub fn start_clock(&self) -> Option<f64> {
        self.started.load(Ordering::Acquire).then(|| f64::from_bits(self.start_clock.load(Ordering::Relaxed)))
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

pub struct RecordTap {
    tx: rtrb::Producer<RecordFrame>,
    shared: Arc<RecordShared>,
    started: bool,
    rate: u32,
    rate_pending: bool,
    gap: u32,
}

pub fn record_channel(capacity: usize) -> (RecordTap, rtrb::Consumer<RecordFrame>, Arc<RecordShared>) {
    let (tx, rx) = rtrb::RingBuffer::new(capacity.max(1));
    let shared = Arc::new(RecordShared::default());
    let tap = RecordTap { tx, shared: shared.clone(), started: false, rate: 0, rate_pending: false, gap: 0 };
    (tap, rx, shared)
}

impl RecordTap {
    pub(crate) fn begin_block(&mut self, clock_secs: f64, sample_rate: u32) {
        if !self.started {
            self.started = true;
            self.shared.start_clock.store(clock_secs.to_bits(), Ordering::Relaxed);
            self.shared.started.store(true, Ordering::Release);
        }
        if sample_rate != self.rate {
            if self.rate > 0 {
                self.gap = (self.gap as u64 * sample_rate as u64 / self.rate as u64) as u32;
            }
            self.rate = sample_rate;
            self.rate_pending = true;
        }
    }

    pub(crate) fn push(&mut self, frame: [f32; 2]) {
        if self.rate_pending {
            if self.tx.push(RecordFrame::Rate(self.rate)).is_err() {
                return self.drop_frame();
            }
            self.rate_pending = false;
        }
        if self.gap > 0 {
            if self.tx.push(RecordFrame::Gap(self.gap)).is_err() {
                return self.drop_frame();
            }
            self.gap = 0;
        }
        if self.tx.push(RecordFrame::Audio(frame)).is_err() {
            self.drop_frame();
        }
    }

    fn drop_frame(&mut self) {
        self.gap = self.gap.saturating_add(1);
        self.shared.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

/// Single-use carrier that lets a record tap travel through the cloneable command queue.
#[derive(Clone)]
pub struct RecordTapSlot(Arc<Mutex<Option<Option<Box<RecordTap>>>>>);

impl RecordTapSlot {
    pub fn new(tap: Option<RecordTap>) -> Self {
        Self(Arc::new(Mutex::new(Some(tap.map(Box::new)))))
    }

    pub(crate) fn take(&self) -> Option<Option<Box<RecordTap>>> {
        self.0.try_lock().and_then(|mut slot| slot.take())
    }
}

impl fmt::Debug for RecordTapSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecordTapSlot")
    }
}
