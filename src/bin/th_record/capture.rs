//! cpal input capture — owns the input stream, writes samples into a
//! shared ring buffer keyed by channel. Converts every supported
//! sample format into f32 at the callback boundary so the UI layer
//! only ever sees f32.

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

/// Shared between the cpal callback thread and the UI thread. Holds
/// the currently-recording buffer (per channel), running peak meters,
/// and a per-channel "this take clipped" flag. UI reads all of them.
pub struct CaptureState {
    /// Per-channel sample buffer. Resized on stream start.
    pub samples: Vec<Vec<f32>>,
    /// Per-channel peak from the last callback burst. UI reads, UI
    /// decays. Capture thread only ever takes `max(current, abs)`.
    pub peak: Vec<f32>,
    /// Per-channel mean-square accumulator for the most recent UI
    /// window. The callback adds `sum(x²)` into `rms_sum[c]` and frame
    /// counts into `rms_count` (shared across channels since frames
    /// interleave). UI reads both to compute RMS, then resets. This
    /// gives a smoother "is the signal live?" read than the peak meter
    /// — peak can fire on a single wild sample while RMS will sit
    /// steady on real audio.
    pub rms_sum: Vec<f32>,
    pub rms_count: u32,
    /// Per-channel clip flag. Set by the callback whenever a sample
    /// abs value crosses 0.99 while recording; cleared on take start.
    pub clipped: Vec<bool>,
    /// True while the user's holding REC. Capture callback drops
    /// frames when this is false so idle time doesn't fill memory.
    pub recording: bool,
    /// Sample rate the stream was opened at.
    pub rate: u32,
    /// Channel count the stream was opened at.
    pub channels: u16,
}

impl CaptureState {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
            peak: Vec::new(),
            rms_sum: Vec::new(),
            rms_count: 0,
            clipped: Vec::new(),
            recording: false,
            rate: 48_000,
            channels: 0,
        }
    }

    pub fn reset_buffers(&mut self, channels: u16, rate: u32) {
        self.samples = (0..channels).map(|_| Vec::new()).collect();
        self.peak = vec![0.0; channels as usize];
        self.rms_sum = vec![0.0; channels as usize];
        self.rms_count = 0;
        self.clipped = vec![false; channels as usize];
        self.rate = rate;
        self.channels = channels;
    }

    /// Drain the RMS accumulators into a per-channel RMS value and
    /// zero the running sum. Called from the UI thread each frame so
    /// the "live level" reading covers roughly one frame worth of
    /// audio (~16-33 ms at 60 Hz). Returns zeros if no frames arrived
    /// during the window.
    pub fn drain_rms(&mut self) -> Vec<f32> {
        let ch = self.channels as usize;
        let count = self.rms_count.max(1) as f32;
        let mut out = vec![0.0f32; ch];
        for (i, slot) in out.iter_mut().enumerate() {
            let sum = self.rms_sum.get(i).copied().unwrap_or(0.0);
            *slot = if self.rms_count == 0 { 0.0 } else { (sum / count).sqrt() };
        }
        for s in &mut self.rms_sum {
            *s = 0.0;
        }
        self.rms_count = 0;
        out
    }

    /// Wipe the per-channel clip flags. Called when a new take is
    /// armed so stale clip warnings don't linger.
    pub fn clear_clip_flags(&mut self) {
        for c in &mut self.clipped {
            *c = false;
        }
    }

    /// Drain the captured buffers into owned Vecs, leaving the state
    /// ready for the next take. Used by the "stop → keep take" path.
    pub fn take_samples(&mut self) -> Vec<Vec<f32>> {
        let ch = self.channels as usize;
        std::mem::replace(&mut self.samples, (0..ch).map(|_| Vec::new()).collect())
    }

    /// Decay peaks toward zero. Called from the UI thread each frame
    /// so meter falloff is time-based rather than sample-count-based.
    pub fn decay_peaks(&mut self, factor: f32) {
        for p in &mut self.peak {
            *p *= factor;
        }
    }
}

/// Every input device + its preferred default config. Used to
/// populate the device dropdown.
pub struct DeviceInfo {
    pub name: String,
    pub channels: u16,
    pub default_rate: u32,
    pub sample_format: String,
}

pub fn list_input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let mut out = Vec::new();
    if let Ok(iter) = host.input_devices() {
        for d in iter {
            let name = d.name().unwrap_or_else(|_| "<unnamed>".into());
            let Ok(config) = d.default_input_config() else {
                out.push(DeviceInfo {
                    name,
                    channels: 0,
                    default_rate: 0,
                    sample_format: "unknown".into(),
                });
                continue;
            };
            out.push(DeviceInfo {
                name,
                channels: config.channels(),
                default_rate: config.sample_rate().0,
                sample_format: format!("{:?}", config.sample_format()),
            });
        }
    }
    out
}

/// Spin up an input stream on the named device and wire it to the
/// shared `CaptureState`. Returns the stream handle; dropping it
/// stops capture.
pub fn start_stream(
    device_name: &str,
    state: Arc<Mutex<CaptureState>>,
) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = host
        .input_devices()
        .context("enumerate input devices")?
        .find(|d| d.name().ok().as_deref() == Some(device_name))
        .ok_or_else(|| anyhow!("input device `{device_name}` not found"))?;

    let supported = device
        .default_input_config()
        .context("read default input config")?;
    let channels = supported.channels();
    let rate = supported.sample_rate().0;
    let format = supported.sample_format();

    {
        let mut s = state.lock().unwrap();
        s.reset_buffers(channels, rate);
    }

    let err_fn = |err| tracing::error!(?err, "cpal: input stream error");
    let stream = match format {
        cpal::SampleFormat::F32 => {
            let state = state.clone();
            device.build_input_stream(
                &supported.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    callback_f32(data, channels, &state);
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let state = state.clone();
            device.build_input_stream(
                &supported.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    callback_convert(data, channels, &state, |s| s as f32 / i16::MAX as f32);
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let state = state.clone();
            device.build_input_stream(
                &supported.into(),
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    callback_convert(data, channels, &state, |s| {
                        (s as f32 - u16::MAX as f32 * 0.5) / (u16::MAX as f32 * 0.5)
                    });
                },
                err_fn,
                None,
            )
        }
        other => {
            return Err(anyhow!(
                "unsupported sample format {other:?} on device `{device_name}`"
            ));
        }
    }
    .context("build input stream")?;

    stream.play().context("start input stream")?;
    tracing::info!(device = %device_name, channels, rate, "capture stream armed");
    Ok(stream)
}

fn callback_f32(data: &[f32], channels: u16, state: &Arc<Mutex<CaptureState>>) {
    let Ok(mut s) = state.lock() else { return };
    // Meters run whether or not we're recording — the whole point of
    // pre-record monitoring is to *see* signal before committing.
    update_peaks(data, channels, &mut s.peak);
    let CaptureState { rms_sum, rms_count, .. } = &mut *s;
    accumulate_rms(data, channels, rms_sum, rms_count);
    if !s.recording {
        return;
    }
    append_interleaved(data, channels, &mut s.samples);
    update_clips(data, channels, &mut s.clipped);
}

fn callback_convert<T: Copy>(
    data: &[T],
    channels: u16,
    state: &Arc<Mutex<CaptureState>>,
    cvt: impl Fn(T) -> f32,
) {
    let Ok(mut s) = state.lock() else { return };
    // Always convert — meters need f32 either way, and doing it once
    // lets us share the buffer with the record path below.
    let mut tmp = Vec::with_capacity(data.len());
    for &v in data {
        tmp.push(cvt(v));
    }
    update_peaks(&tmp, channels, &mut s.peak);
    let CaptureState { rms_sum, rms_count, .. } = &mut *s;
    accumulate_rms(&tmp, channels, rms_sum, rms_count);
    if !s.recording {
        return;
    }
    append_interleaved(&tmp, channels, &mut s.samples);
    update_clips(&tmp, channels, &mut s.clipped);
}

/// Sum x² per channel for the UI-thread RMS calculation. Frame count
/// is shared across channels since every frame contributes one sample
/// per channel.
fn accumulate_rms(data: &[f32], channels: u16, sum: &mut Vec<f32>, count: &mut u32) {
    let ch = channels as usize;
    if ch == 0 {
        return;
    }
    if sum.len() < ch {
        sum.resize(ch, 0.0);
    }
    let mut frames = 0u32;
    for frame in data.chunks(ch) {
        if frame.len() < ch {
            break; // trailing partial frame — safer to skip
        }
        for (c, &s) in frame.iter().enumerate() {
            sum[c] += s * s;
        }
        frames += 1;
    }
    *count = count.saturating_add(frames);
}

fn append_interleaved(data: &[f32], channels: u16, buffers: &mut Vec<Vec<f32>>) {
    let ch = channels as usize;
    if buffers.len() < ch {
        buffers.resize_with(ch, Vec::new);
    }
    for frame in data.chunks(ch) {
        for (c, &s) in frame.iter().enumerate() {
            buffers[c].push(s);
        }
    }
}

fn update_peaks(data: &[f32], channels: u16, peaks: &mut Vec<f32>) {
    let ch = channels as usize;
    if peaks.len() < ch {
        peaks.resize(ch, 0.0);
    }
    for frame in data.chunks(ch) {
        for (c, &s) in frame.iter().enumerate() {
            let a = s.abs();
            if a > peaks[c] {
                peaks[c] = a;
            }
        }
    }
}

/// Scan the just-captured frames for clips; latch the flag on any
/// sample >= 0.99 abs. Only called while recording so idle input
/// noise doesn't falsely latch the flag.
fn update_clips(data: &[f32], channels: u16, clipped: &mut Vec<bool>) {
    let ch = channels as usize;
    if clipped.len() < ch {
        clipped.resize(ch, false);
    }
    for frame in data.chunks(ch) {
        for (c, &s) in frame.iter().enumerate() {
            if s.abs() >= 0.99 {
                clipped[c] = true;
            }
        }
    }
}
