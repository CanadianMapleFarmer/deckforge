pub mod curves;
pub mod filter;
pub mod fx;
pub mod interp;
pub mod isolator;
pub mod limiter;
pub mod meter;
pub mod signalsmith;
pub mod smooth;
pub mod stretch;
pub mod svf;

pub use curves::{channel_fader_gain, crossfader_gains};
pub use filter::DjFilter;
pub use fx::{reverb_rt60_for_percent, AscendLanding, AscendRequest, BackwashArm, BackwashRequest, BeatFx, ColorFx, ReleaseFx, SmartCfx, Transport};
pub use isolator::{ChannelEq, Isolator};
pub use limiter::{soft_limit, PeakLimiter};
pub use meter::PeakMeter;
pub use smooth::Smoothed;
pub use stretch::Stretcher;
pub use svf::Svf;

#[cfg(test)]
pub(crate) mod alloc_guard {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    struct Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            COUNT.with(|c| c.set(c.get() + 1));
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            COUNT.with(|c| c.set(c.get() + 1));
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    /// Runs `f` and returns how many heap allocations this thread made meanwhile.
    pub(crate) fn allocations_in<R>(f: impl FnOnce() -> R) -> (R, usize) {
        let before = COUNT.with(Cell::get);
        let out = f();
        (out, COUNT.with(Cell::get) - before)
    }
}
