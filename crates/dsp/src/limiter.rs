pub const LIMIT_KNEE: f32 = 0.891;

/// Stateless soft limiter: transparent below the knee, asymptotically approaches 1.0 above it.
#[inline]
pub fn soft_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= LIMIT_KNEE {
        x
    } else {
        let range = 1.0 - LIMIT_KNEE;
        let y = LIMIT_KNEE + range * ((a - LIMIT_KNEE) / range).tanh();
        y.copysign(x)
    }
}

const LOOKAHEAD_SECS: f32 = 0.0015;
const HOLD_SECS: f32 = 0.02;
const RELEASE_SECS: f32 = 0.1;

/// Stereo-linked look-ahead peak limiter: turns the gain down ahead of each peak instead of reshaping the wave.
pub struct PeakLimiter {
    lookahead: usize,
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    write: usize,
    hold: usize,
    min_values: Vec<f32>,
    min_times: Vec<u64>,
    min_head: usize,
    min_len: usize,
    time: u64,
    window: Vec<f32>,
    window_sum: f64,
    window_pos: usize,
    envelope: f32,
    release: f32,
    gain: f32,
}

impl PeakLimiter {
    pub const CEILING: f32 = 0.944;

    pub fn new(sample_rate: f32) -> Self {
        let lookahead = ((LOOKAHEAD_SECS * sample_rate).round() as usize).max(1);
        let hold = lookahead + (HOLD_SECS * sample_rate).round() as usize;
        Self {
            lookahead,
            delay_l: vec![0.0; lookahead],
            delay_r: vec![0.0; lookahead],
            write: 0,
            hold,
            min_values: vec![1.0; hold],
            min_times: vec![0; hold],
            min_head: 0,
            min_len: 0,
            time: 0,
            window: vec![1.0; lookahead],
            window_sum: lookahead as f64,
            window_pos: 0,
            envelope: 1.0,
            release: 1.0 - (-1.0 / (RELEASE_SECS * sample_rate)).exp(),
            gain: 1.0,
        }
    }

    /// Frames between a sample going in and coming out.
    pub fn latency(&self) -> usize {
        self.lookahead - 1
    }

    pub fn gain(&self) -> f32 {
        self.gain
    }

    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let peak = l.abs().max(r.abs());
        let wanted = if peak > Self::CEILING { Self::CEILING / peak } else { 1.0 };
        self.envelope = if wanted < self.envelope { wanted } else { self.envelope + (wanted - self.envelope) * self.release };

        let held = self.hold_min(self.envelope);
        self.window_sum += f64::from(held) - f64::from(self.window[self.window_pos]);
        self.window[self.window_pos] = held;
        self.window_pos = (self.window_pos + 1) % self.lookahead;
        self.gain = (self.window_sum / self.lookahead as f64).min(1.0) as f32;

        self.delay_l[self.write] = l;
        self.delay_r[self.write] = r;
        self.write = (self.write + 1) % self.lookahead;
        (self.delay_l[self.write] * self.gain, self.delay_r[self.write] * self.gain)
    }

    /// Minimum of the last `hold` envelope values, kept as a monotonic queue in fixed storage.
    fn hold_min(&mut self, value: f32) -> f32 {
        let cap = self.hold;
        while self.min_len > 0 && self.min_values[(self.min_head + self.min_len - 1) % cap] >= value {
            self.min_len -= 1;
        }
        let tail = (self.min_head + self.min_len) % cap;
        self.min_values[tail] = value;
        self.min_times[tail] = self.time;
        self.min_len += 1;
        while self.min_times[self.min_head] + cap as u64 <= self.time {
            self.min_head = (self.min_head + 1) % cap;
            self.min_len -= 1;
        }
        self.time += 1;
        self.min_values[self.min_head]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_exceeds_full_scale() {
        for i in 0..2000 {
            let x = i as f32 / 100.0 - 10.0;
            assert!(soft_limit(x).abs() <= 1.0);
        }
        assert_eq!(soft_limit(0.5), 0.5);
    }

    const SR: f32 = 48_000.0;

    fn limit(input: &[f32]) -> (PeakLimiter, Vec<f32>) {
        let mut lim = PeakLimiter::new(SR);
        let out = input.iter().map(|&x| lim.process(x, x).0).collect();
        (lim, out)
    }

    fn sine(freq: f32, amplitude: f32, secs: f32) -> Vec<f32> {
        (0..(secs * SR) as usize).map(|i| amplitude * (std::f32::consts::TAU * freq * i as f32 / SR).sin()).collect()
    }

    /// Total harmonic distortion over whole cycles of `freq`, skipping the first half second.
    fn thd(signal: &[f32], freq: f32) -> f64 {
        let period = (SR / freq) as usize;
        let start = SR as usize / 2;
        let len = (signal.len() - start) / period * period;
        let tail = &signal[start..start + len];
        let bin = |h: f64| {
            let w = std::f64::consts::TAU * h * freq as f64 / SR as f64;
            let (re, im) = tail.iter().enumerate().fold((0.0, 0.0), |(re, im), (i, &x)| (re + x as f64 * (w * i as f64).cos(), im + x as f64 * (w * i as f64).sin()));
            (re * re + im * im).sqrt()
        };
        let harmonics: f64 = (2..12).map(|h| bin(h as f64).powi(2)).sum();
        harmonics.sqrt() / bin(1.0)
    }

    #[test]
    fn a_full_scale_track_pushed_twelve_db_stays_clean() {
        for freq in [60.0, 1_000.0] {
            let (_, out) = limit(&sine(freq, 4.0, 2.0));
            let distortion = thd(&out, freq);
            assert!(distortion < 0.01, "{freq} Hz at +12 dB: THD {:.2} %", distortion * 100.0);
            let peak = out.iter().fold(0f32, |m, x| m.max(x.abs()));
            assert!(peak <= PeakLimiter::CEILING + 1e-6, "{freq} Hz peaks at {peak}");
            assert!(peak > PeakLimiter::CEILING - 0.02, "{freq} Hz is limited to {peak}, not louder");
        }
    }

    #[test]
    fn peaks_never_pass_the_ceiling() {
        let mut seed = 0x2545_f491_u32;
        let noise: Vec<f32> = (0..SR as usize)
            .map(|i| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let burst = if (i / 4_800) % 3 == 0 { 8.0 } else { 0.3 };
                (seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * burst
            })
            .collect();
        let (_, out) = limit(&noise);
        assert!(out.iter().all(|x| x.abs() <= PeakLimiter::CEILING + 1e-6));
    }

    #[test]
    fn quiet_audio_passes_unchanged_after_the_lookahead() {
        let input = sine(440.0, 0.5, 0.5);
        let (lim, out) = limit(&input);
        let delay = lim.latency();
        assert!(delay > 0 && delay < (SR * 0.003) as usize, "lookahead {delay} frames");
        for i in delay..input.len() {
            assert!((out[i] - input[i - delay]).abs() < 1e-6, "frame {i}");
        }
    }

    #[test]
    fn processing_does_not_allocate() {
        let mut lim = PeakLimiter::new(SR);
        let (_, allocations) = crate::alloc_guard::allocations_in(|| {
            for i in 0..10_000 {
                lim.process((i as f32 * 0.01).sin() * 3.0, 0.0);
            }
        });
        assert_eq!(allocations, 0);
    }
}
