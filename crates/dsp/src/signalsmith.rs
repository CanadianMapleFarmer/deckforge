//! Safe stereo wrapper around the vendored Signalsmith Stretch (see `vendor/README.md`).

use std::ffi::{c_double, c_float, c_int};

#[repr(C)]
struct RawStretch {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn df_stretch_new(sample_rate: c_float) -> *mut RawStretch;
    fn df_stretch_free(stretch: *mut RawStretch);
    fn df_stretch_input_latency(stretch: *const RawStretch) -> c_int;
    fn df_stretch_output_latency(stretch: *const RawStretch) -> c_int;
    fn df_stretch_preroll(stretch: *const RawStretch) -> c_int;
    fn df_stretch_reset(stretch: *mut RawStretch);
    fn df_stretch_seek(stretch: *mut RawStretch, left: *const c_float, right: *const c_float, frames: c_int, rate: c_double);
    fn df_stretch_process(
        stretch: *mut RawStretch,
        in_left: *const c_float,
        in_right: *const c_float,
        in_frames: c_int,
        out_left: *mut c_float,
        out_right: *mut c_float,
        out_frames: c_int,
    );
}

/// One stereo Signalsmith Stretch instance; `process` and `seek` do not allocate.
pub struct Voice {
    raw: *mut RawStretch,
}

// SAFETY: the instance is only ever used through `&mut self`, so one thread touches it at a time.
unsafe impl Send for Voice {}

impl Voice {
    pub fn new(sample_rate: f64) -> Self {
        let raw = unsafe { df_stretch_new(sample_rate as c_float) };
        assert!(!raw.is_null());
        Self { raw }
    }

    pub fn input_latency(&self) -> usize {
        unsafe { df_stretch_input_latency(self.raw) as usize }
    }

    pub fn output_latency(&self) -> usize {
        unsafe { df_stretch_output_latency(self.raw) as usize }
    }

    /// Input frames `seek` wants before the new position.
    pub fn preroll_frames(&self) -> usize {
        unsafe { df_stretch_preroll(self.raw) as usize }
    }

    pub fn reset(&mut self) {
        unsafe { df_stretch_reset(self.raw) }
    }

    pub fn seek(&mut self, l: &[f32], r: &[f32], rate: f64) {
        let frames = l.len().min(r.len());
        unsafe { df_stretch_seek(self.raw, l.as_ptr(), r.as_ptr(), frames as c_int, rate) }
    }

    pub fn process(&mut self, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        let in_frames = in_l.len().min(in_r.len());
        let out_frames = out_l.len().min(out_r.len());
        unsafe {
            df_stretch_process(
                self.raw,
                in_l.as_ptr(),
                in_r.as_ptr(),
                in_frames as c_int,
                out_l.as_mut_ptr(),
                out_r.as_mut_ptr(),
                out_frames as c_int,
            )
        }
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        unsafe { df_stretch_free(self.raw) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freqs: &[f64], sr: f64, frames: usize) -> Vec<f32> {
        (0..frames).map(|i| freqs.iter().map(|f| (std::f64::consts::TAU * f * i as f64 / sr).sin()).sum::<f64>() as f32 * 0.25).collect()
    }

    fn amplitude(x: &[f32], freq: f64, sr: f64) -> f64 {
        let w = std::f64::consts::TAU * freq / sr;
        let (mut re, mut im, mut norm) = (0.0, 0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            let h = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
            re += *v as f64 * h * (w * i as f64).cos();
            im += *v as f64 * h * (w * i as f64).sin();
            norm += h;
        }
        2.0 * (re * re + im * im).sqrt() / norm
    }

    #[test]
    fn voice_stretches_without_changing_pitch() {
        let sr = 48_000.0;
        let input = tone(&[220.0, 9_000.0], sr, sr as usize * 4);
        let mut voice = Voice::new(sr);
        let tempo = 1.06;
        let block = 512;
        let (mut out_l, mut out_r) = (Vec::new(), Vec::new());
        let (mut ol, mut or) = (vec![0.0; block], vec![0.0; block]);
        let mut read = 0.0f64;
        while (read + block as f64 * tempo) as usize + 1 < input.len() {
            let (a, b) = (read as usize, (read + block as f64 * tempo) as usize);
            voice.process(&input[a..b], &input[a..b], &mut ol, &mut or);
            out_l.extend_from_slice(&ol);
            out_r.extend_from_slice(&or);
            read += block as f64 * tempo;
        }
        let tail = &out_l[sr as usize..];
        for f in [220.0, 9_000.0] {
            let a = amplitude(tail, f, sr);
            assert!((20.0 * (a / 0.25).log10()).abs() < 1.0, "{f} Hz amplitude {a}");
        }
        assert!(amplitude(tail, 9_000.0 * tempo, sr) < 0.01, "no pitch shift");
        let channel_gap = out_l.iter().zip(&out_r).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(channel_gap < 1e-3, "identical channels stay identical: {channel_gap}");
        assert!(voice.input_latency() > 0 && voice.output_latency() > 0 && voice.preroll_frames() > voice.input_latency());
    }
}
