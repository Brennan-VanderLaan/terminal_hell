//! Linux direct-evdev force-feedback fallback.
//!
//! Context: `gilrs-core` 0.5's Linux FF probe (`test_ff`) requires
//! the pad to advertise FF_SQUARE + FF_TRIANGLE + FF_SINE + FF_GAIN
//! — the "full periodic" bundle. `xpadneo` (and many other modern
//! rumble-only drivers) only advertises FF_RUMBLE, so gilrs refuses
//! the pad even though a bare `EVIOCSFF` + `FF_RUMBLE` write would
//! work fine. This module does exactly that: finds the right
//! `/dev/input/event*` node for the active gamepad and talks to it
//! directly via libc ioctls.
//!
//! Windows + macOS routes still go through gilrs's native backends
//! (XInput / IOKit), which don't have this probe-mismatch issue —
//! this whole module is gated behind `cfg(target_os = "linux")`.

#![cfg(target_os = "linux")]

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};

const EV_FF: u16 = 0x15;
const FF_RUMBLE: u16 = 0x50;

/// `struct ff_replay` from `linux/input.h`. 4 bytes.
#[repr(C)]
#[derive(Copy, Clone, Default)]
struct FfReplay {
    length: u16,
    delay: u16,
}

/// `struct ff_trigger` from `linux/input.h`. 4 bytes.
#[repr(C)]
#[derive(Copy, Clone, Default)]
struct FfTrigger {
    button: u16,
    interval: u16,
}

/// `struct ff_effect` from `linux/input.h`, x86_64 layout. The
/// tagged union `u` inside the kernel struct is sized for the
/// largest variant (`ff_periodic_effect`, ~30 bytes with an 8-byte-
/// aligned `__user*` inside), which pushes total size to 48 bytes
/// including the 2 bytes of pad before the union to align it on 8.
///
/// We only ever write `FF_RUMBLE` effects, whose payload is the
/// first 4 bytes of the union (`strong_magnitude: u16` then
/// `weak_magnitude: u16`). The rest of `payload` stays zeroed.
#[repr(C)]
#[derive(Copy, Clone)]
struct FfEffect {
    kind: u16,
    id: i16,
    direction: u16,
    trigger: FfTrigger,
    replay: FfReplay,
    /// Padding so the union slot below lands on an 8-byte boundary,
    /// matching the kernel's layout of `ff_periodic_effect` (whose
    /// `__user* custom_data` needs 8-byte alignment on 64-bit).
    _align: [u8; 2],
    /// Union payload. Sized for `ff_periodic_effect` (the largest
    /// variant) so our `sizeof(ff_effect)` matches the kernel's and
    /// `EVIOCSFF` accepts the call.
    payload: [u8; 32],
}

impl Default for FfEffect {
    fn default() -> Self {
        Self {
            kind: 0,
            id: -1,
            direction: 0,
            trigger: FfTrigger::default(),
            replay: FfReplay::default(),
            _align: [0; 2],
            payload: [0; 32],
        }
    }
}

/// `struct input_event` on 64-bit Linux. Kernel writes `timeval`
/// with two `long` fields followed by `u16, u16, s32`. We only ever
/// WRITE events (to trigger/stop a loaded effect), so the timeval
/// can be zero — the kernel stamps it itself on read.
#[repr(C)]
struct InputEvent {
    tv_sec: libc::c_long,
    tv_usec: libc::c_long,
    kind: u16,
    code: u16,
    value: i32,
}

// ----- ioctl request-number construction (asm-generic/ioctl.h) -----

const IOC_WRITE: u64 = 1;
const IOC_READ: u64 = 2;
const IOC_NRBITS: u64 = 8;
const IOC_TYPEBITS: u64 = 8;
const IOC_SIZEBITS: u64 = 14;
const IOC_DIRSHIFT: u64 = IOC_NRBITS + IOC_TYPEBITS + IOC_SIZEBITS;
const IOC_SIZESHIFT: u64 = IOC_NRBITS + IOC_TYPEBITS;
const IOC_TYPESHIFT: u64 = IOC_NRBITS;

const EV_TYPE: u64 = b'E' as u64;

fn ioc(dir: u64, ty: u64, nr: u64, size: usize) -> u64 {
    (dir << IOC_DIRSHIFT)
        | ((size as u64) << IOC_SIZESHIFT)
        | (ty << IOC_TYPESHIFT)
        | nr
}

fn eviocsff() -> u64 {
    ioc(IOC_WRITE, EV_TYPE, 0x80, std::mem::size_of::<FfEffect>())
}

fn eviocgname(len: usize) -> u64 {
    ioc(IOC_READ, EV_TYPE, 0x06, len)
}

fn eviocgbit_ff(len: usize) -> u64 {
    // `EVIOCGBIT(ev, len)` expands to `_IOC(IOC_READ, 'E', 0x20 + ev, len)`.
    ioc(IOC_READ, EV_TYPE, 0x20 + EV_FF as u64, len)
}

fn eviocrmff() -> u64 {
    ioc(IOC_WRITE, EV_TYPE, 0x81, std::mem::size_of::<i32>())
}

// ----- public rumble device ---------------------------------------

/// Opened `/dev/input/event*` handle that can accept `FF_RUMBLE`
/// effects. Owned by the gamepad worker; dropped on disconnect.
pub struct RumbleDevice {
    file: File,
    /// Path kept for diagnostics only.
    pub path: String,
    /// Effects we've started but not yet explicitly stopped. Each
    /// entry is `(id, deadline)` where `deadline` is when we should
    /// write a `value=0` stop event. The kernel's FF core is
    /// *supposed* to auto-stop after `replay.length` but xpadneo and
    /// similar HID-layer drivers don't honour it — they run until
    /// the host writes a stop. So we schedule the stop ourselves;
    /// [`tick`] runs from the worker thread every few ms and
    /// commits stops that have come due.
    pending: Vec<(i16, Instant)>,
}

/// Maximum effects we'll hold upload slots for. The kernel default
/// is 16 per FD; leaving a little headroom.
const MAX_EFFECT_SLOTS: usize = 12;

impl RumbleDevice {
    /// Walk `/dev/input/event*`, pick the first node that (a) we can
    /// open `O_RDWR`, (b) has `FF_RUMBLE` in its capability bitmap,
    /// and (c) optionally matches `name_hint` as a case-insensitive
    /// substring in its `EVIOCGNAME` name.
    ///
    /// If `name_hint` is empty, any FF-capable evdev node wins —
    /// appropriate when exactly one gamepad is attached.
    pub fn find(name_hint: &str) -> Option<Self> {
        let entries = std::fs::read_dir("/dev/input").ok()?;
        let mut candidates: Vec<(String, String)> = Vec::new(); // (path, name)
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !fname.starts_with("event") {
                continue;
            }
            let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else {
                continue;
            };
            let name = match read_name(&file) {
                Some(n) => n,
                None => continue,
            };
            if !has_ff_rumble(&file) {
                continue;
            }
            let path_str = path.to_string_lossy().into_owned();
            candidates.push((path_str, name));
        }

        let hint_lower = name_hint.to_lowercase();
        // Prefer a name-substring match when a hint is provided.
        if !hint_lower.is_empty() {
            if let Some((p, _n)) = candidates
                .iter()
                .find(|(_, n)| n.to_lowercase().contains(&hint_lower))
            {
                if let Ok(file) = OpenOptions::new().read(true).write(true).open(p) {
                    return Some(Self {
                        file,
                        path: p.clone(),
                        pending: Vec::new(),
                    });
                }
            }
        }
        // Otherwise: first FF-capable candidate.
        if let Some((p, _n)) = candidates.first() {
            if let Ok(file) = OpenOptions::new().read(true).write(true).open(p) {
                return Some(Self {
                    file,
                    path: p.clone(),
                    pending: Vec::new(),
                });
            }
        }
        None
    }

    /// Upload and play a single rumble effect with the given motor
    /// magnitudes (0..=0xFFFF) for `dur_ms` milliseconds. Returns
    /// the kernel-assigned effect id on success. The caller must
    /// also call [`tick`] periodically so the effect gets stopped
    /// when its deadline passes.
    pub fn play_rumble(
        &mut self,
        strong: u16,
        weak: u16,
        dur_ms: u16,
    ) -> std::io::Result<i16> {
        // Make room if we're at the slot limit — stop + erase the
        // oldest before uploading a new one.
        if self.pending.len() >= MAX_EFFECT_SLOTS {
            let (oldest_id, _) = self.pending.remove(0);
            let _ = self.write_ff_event(oldest_id, 0);
            let _ = self.erase_raw(oldest_id);
        }

        let mut eff = FfEffect {
            kind: FF_RUMBLE,
            id: -1,
            replay: FfReplay { length: dur_ms, delay: 0 },
            ..Default::default()
        };
        // Rumble payload = (strong_magnitude: u16, weak_magnitude: u16)
        // starting at the union's offset 0.
        eff.payload[0..2].copy_from_slice(&strong.to_ne_bytes());
        eff.payload[2..4].copy_from_slice(&weak.to_ne_bytes());

        let ret = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                eviocsff() as libc::c_ulong,
                &mut eff as *mut FfEffect,
            )
        };
        if ret < 0 {
            return Err(std::io::Error::last_os_error());
        }

        self.write_ff_event(eff.id, 1)?;
        let deadline = Instant::now() + Duration::from_millis(dur_ms as u64);
        self.pending.push((eff.id, deadline));
        Ok(eff.id)
    }

    /// Drive the stop-and-erase schedule forward. Call this
    /// regularly from the worker thread (every poll tick is fine —
    /// it's a cheap `Instant::now()` comparison when nothing is
    /// pending). Emits `value=0` EV_FF events for any effect whose
    /// deadline has elapsed, then frees the kernel slot so the
    /// per-fd effect budget doesn't drift up under auto-fire.
    pub fn tick(&mut self) {
        let now = Instant::now();
        // Snapshot the pending list, then stop+erase anything due.
        // Split into "due" and "keep" vectors first so the mutable
        // borrow on `self.pending` is released before we call the
        // `&mut self` / `&self` helpers.
        let mut due: Vec<i16> = Vec::new();
        let mut keep: Vec<(i16, Instant)> = Vec::with_capacity(self.pending.len());
        for (id, deadline) in self.pending.drain(..) {
            if now >= deadline {
                due.push(id);
            } else {
                keep.push((id, deadline));
            }
        }
        self.pending = keep;
        for id in due {
            let _ = self.write_ff_event(id, 0);
            let _ = self.erase_raw(id);
        }
    }

    /// Write a bare EV_FF input event to start (`value=1`) or stop
    /// (`value=0`) the given effect. The kernel dispatches on `code`
    /// (our effect id); `tv_sec`/`tv_usec` are ignored for writes.
    fn write_ff_event(&mut self, id: i16, value: i32) -> std::io::Result<()> {
        let ev = InputEvent {
            tv_sec: 0,
            tv_usec: 0,
            kind: EV_FF,
            code: id as u16,
            value,
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&ev as *const InputEvent) as *const u8,
                std::mem::size_of::<InputEvent>(),
            )
        };
        self.file.write_all(bytes)
    }

    fn erase_raw(&self, id: i16) -> std::io::Result<()> {
        // `EVIOCRMFF` is `_IOW('E', 0x81, int)` — the id is passed
        // by value (not pointer), truncated to int by the kernel.
        let id_as_long: libc::c_long = id.into();
        let ret = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                eviocrmff() as libc::c_ulong,
                id_as_long,
            )
        };
        if ret < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for RumbleDevice {
    fn drop(&mut self) {
        // Silence + free every still-loaded effect before the fd
        // closes. Without the explicit stop, a crash/exit mid-rumble
        // would leave the pad buzzing until the controller times out
        // on its own (can be several seconds on Bluetooth pads).
        let ids: Vec<i16> = self.pending.iter().map(|(id, _)| *id).collect();
        self.pending.clear();
        for id in ids {
            let _ = self.write_ff_event(id, 0);
            let _ = self.erase_raw(id);
        }
    }
}

fn read_name(file: &File) -> Option<String> {
    let mut buf = [0u8; 256];
    let ret = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            eviocgname(buf.len()) as libc::c_ulong,
            buf.as_mut_ptr(),
        )
    };
    if ret < 0 {
        return None;
    }
    let len = (ret as usize).min(buf.len());
    let slice = &buf[..len];
    // EVIOCGNAME returns a NUL-terminated string; trim the terminator.
    let end = slice.iter().position(|b| *b == 0).unwrap_or(slice.len());
    std::str::from_utf8(&slice[..end]).ok().map(str::to_string)
}

fn has_ff_rumble(file: &File) -> bool {
    // FF_MAX is 0x7F in the kernel; bitmap = (FF_MAX / 8) + 1 = 16 bytes.
    let mut bits = [0u8; 16];
    let ret = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            eviocgbit_ff(bits.len()) as libc::c_ulong,
            bits.as_mut_ptr(),
        )
    };
    if ret < 0 {
        return false;
    }
    let bit = FF_RUMBLE as usize;
    (bits[bit / 8] & (1 << (bit % 8))) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ff_effect_size_matches_linux_uapi() {
        // On x86_64 Linux, sizeof(struct ff_effect) = 48 (union sized
        // for ff_periodic_effect, 8-byte aligned). Our Rust layout
        // must match exactly or `EVIOCSFF` will reject the call.
        assert_eq!(std::mem::size_of::<FfEffect>(), 48);
    }

    #[test]
    fn ioctl_numbers_match_linux_uapi() {
        // Verified values from `uapi/linux/input.h`:
        //   EVIOCSFF  = _IOC(IOC_WRITE, 'E', 0x80, sizeof(ff_effect=48))
        //             = (1<<30) | (48<<16) | (0x45<<8) | 0x80
        //             = 0x4030_4580
        //   EVIOCRMFF = _IOC(IOC_WRITE, 'E', 0x81, 4)
        //             = 0x4004_4581
        assert_eq!(eviocsff(), 0x4030_4580);
        assert_eq!(eviocrmff(), 0x4004_4581);
    }
}
