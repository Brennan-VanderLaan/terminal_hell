//! Process-wide platform startup glue.
//!
//! Windows: the default scheduler tick is ~15.6 ms, so any
//! `thread::sleep(d)` with `d < 15.6 ms` rounds up to that tick. That
//! silently turns the gamepad worker's 2 ms poll into ~64 Hz and makes
//! the main loop's "sleep the rest of the frame" overshoot past the
//! 16.67 ms target. `timeBeginPeriod(1)` bumps the scheduler to 1 ms
//! resolution for the lifetime of the guard; dropping the guard calls
//! `timeEndPeriod(1)` so we release the request when the game exits.
//! The kernel refcounts these per-process since Windows 10 2004, so
//! nested guards stack cleanly.

#[cfg(windows)]
#[link(name = "winmm")]
unsafe extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

/// RAII handle for the process-wide scheduler-tick bump. No-op on
/// non-Windows so callers can hold it unconditionally.
pub struct TimerResolutionGuard(());

impl TimerResolutionGuard {
    /// Request 1 ms scheduler resolution (Windows) for the lifetime of
    /// the returned guard. Safe to call multiple times — kernel
    /// refcounts the begin/end pairs.
    pub fn millisecond() -> Self {
        #[cfg(windows)]
        unsafe {
            timeBeginPeriod(1);
        }
        Self(())
    }
}

impl Drop for TimerResolutionGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            timeEndPeriod(1);
        }
    }
}
