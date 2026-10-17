use deckforge_common::TrackAudio;

use crate::interp::read_cubic;
use crate::signalsmith::Voice;

const CROSSFADE: usize = 96;
const MAX_TEMPO: f64 = 4.0;

struct Lane {
    voice: Voice,
    read: f64,
    carry: f64,
}

impl Lane {
    fn latency(&self, tempo: f64) -> f64 {
        self.voice.input_latency() as f64 + self.voice.output_latency() as f64 * tempo
    }
}

/// Key-lock time stretcher on Signalsmith Stretch: two voices so jumps crossfade into audio that is already in place.
/// Positions are source frames; `step` is source frames per output frame at native pitch.
pub struct Stretcher {
    lanes: [Lane; 2],
    active: usize,
    pending: Option<f64>,
    idle: bool,
    fade_in: usize,
    fade_out: usize,
    releasing: usize,
    tempo: f64,
    step: f64,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    tail_l: Vec<f32>,
    tail_r: Vec<f32>,
}

impl Stretcher {
    pub fn new(out_rate: f64, max_block: usize) -> Self {
        let lane = || Lane { voice: Voice::new(out_rate), read: 0.0, carry: 0.0 };
        let lanes = [lane(), lane()];
        let input = lanes[0].voice.preroll_frames().max((max_block as f64 * MAX_TEMPO) as usize + 2);
        Self {
            lanes,
            active: 0,
            pending: None,
            idle: true,
            fade_in: 0,
            fade_out: 0,
            releasing: 0,
            tempo: 1.0,
            step: 1.0,
            in_l: vec![0.0; input],
            in_r: vec![0.0; input],
            tail_l: vec![0.0; max_block],
            tail_r: vec![0.0; max_block],
        }
    }

    pub const fn crossfade_frames() -> usize {
        CROSSFADE
    }

    /// The next rendered frame plays `position`; the current output crossfades into it.
    pub fn jump(&mut self, position: f64) {
        self.pending = Some(position);
    }

    /// Source position audible at the next output frame.
    pub fn audible_position(&self) -> f64 {
        match self.pending {
            Some(p) => p,
            None => {
                let lane = &self.lanes[self.active];
                lane.read - lane.latency(self.tempo) * self.step
            }
        }
    }

    pub fn is_idle(&self) -> bool {
        self.idle && self.pending.is_none()
    }

    /// Stops after a short fade; `mix_release` crossfades that tail into the caller's output.
    pub fn release(&mut self) {
        if !self.idle {
            self.releasing = CROSSFADE;
            self.fade_out = 0;
            self.fade_in = 0;
        }
        self.pending = None;
        self.idle = true;
    }

    pub fn mix_release(&mut self, track: &TrackAudio, step: f64, tempo: f64, out_l: &mut [f32], out_r: &mut [f32]) {
        if self.releasing == 0 {
            return;
        }
        let n = out_l.len().min(self.releasing);
        let active = self.active;
        self.process_lane(active, track, step, tempo, n);
        for i in 0..n {
            let g = (self.releasing - i) as f32 / CROSSFADE as f32;
            out_l[i] = out_l[i] * (1.0 - g) + self.tail_l[i] * g;
            out_r[i] = out_r[i] * (1.0 - g) + self.tail_r[i] * g;
        }
        self.releasing -= n;
    }

    pub fn render(&mut self, track: &TrackAudio, step: f64, tempo: f64, out_l: &mut [f32], out_r: &mut [f32]) {
        let tempo = tempo.clamp(0.0, MAX_TEMPO);
        let n = out_l.len().min(out_r.len());
        self.step = step;
        self.tempo = tempo;
        if let Some(target) = self.pending.take_if(|_| self.fade_out == 0 && self.fade_in == 0) {
            if self.idle {
                self.seek(self.active, track, step, tempo, target);
                self.fade_in = CROSSFADE;
                self.fade_out = 0;
            } else {
                let next = 1 - self.active;
                self.seek(next, track, step, tempo, target);
                self.active = next;
                self.fade_out = CROSSFADE;
                self.fade_in = 0;
            }
            self.idle = false;
            self.releasing = 0;
        }
        let active = self.active;
        self.process_lane(active, track, step, tempo, n);
        out_l[..n].copy_from_slice(&self.tail_l[..n]);
        out_r[..n].copy_from_slice(&self.tail_r[..n]);
        if self.fade_in > 0 {
            let m = n.min(self.fade_in);
            for i in 0..m {
                let g = 1.0 - (self.fade_in - i) as f32 / CROSSFADE as f32;
                out_l[i] *= g;
                out_r[i] *= g;
            }
            self.fade_in -= m;
        }
        if self.fade_out > 0 {
            let m = n.min(self.fade_out);
            self.process_lane(1 - active, track, step, tempo, m);
            for i in 0..m {
                let g = (self.fade_out - i) as f32 / CROSSFADE as f32;
                out_l[i] = out_l[i] * (1.0 - g) + self.tail_l[i] * g;
                out_r[i] = out_r[i] * (1.0 - g) + self.tail_r[i] * g;
            }
            self.fade_out -= m;
        }
    }

    fn seek(&mut self, lane: usize, track: &TrackAudio, step: f64, tempo: f64, target: f64) {
        let l = &mut self.lanes[lane];
        l.voice.reset();
        l.carry = 0.0;
        let preroll = l.voice.preroll_frames();
        l.read = target + l.voice.input_latency() as f64 * step;
        let start = l.read - preroll as f64 * step;
        for k in 0..preroll {
            let (a, b) = read_cubic(track, start + k as f64 * step);
            self.in_l[k] = a;
            self.in_r[k] = b;
        }
        l.voice.seek(&self.in_l[..preroll], &self.in_r[..preroll], tempo.max(1e-3));
        let mut warmup = l.voice.output_latency();
        while warmup > 0 {
            let n = warmup.min(self.tail_l.len());
            self.process_lane(lane, track, step, tempo, n);
            warmup -= n;
        }
    }

    fn process_lane(&mut self, lane: usize, track: &TrackAudio, step: f64, tempo: f64, n: usize) {
        let l = &mut self.lanes[lane];
        l.carry += n as f64 * tempo;
        let count = (l.carry.floor() as usize).min(self.in_l.len());
        l.carry -= count as f64;
        for k in 0..count {
            let (a, b) = read_cubic(track, l.read + k as f64 * step);
            self.in_l[k] = a;
            self.in_r[k] = b;
        }
        l.read += count as f64 * step;
        l.voice.process(&self.in_l[..count], &self.in_r[..count], &mut self.tail_l[..n], &mut self.tail_r[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc_guard::allocations_in;

    const OUT: f64 = 48_000.0;
    const SRC: u32 = 44_100;
    const BLOCK: usize = 512;

    fn track(secs: f64, sample: impl Fn(f64) -> f32) -> TrackAudio {
        let n = (secs * SRC as f64) as usize;
        TrackAudio::new(SRC, (0..n).flat_map(|i| { let s = sample(i as f64 / SRC as f64); [s, s] }).collect())
    }

    fn tones(freqs: &'static [f64]) -> impl Fn(f64) -> f32 {
        move |t| freqs.iter().map(|f| (std::f64::consts::TAU * f * t).sin()).sum::<f64>() as f32 * 0.2
    }

    /// Silence with a 1 kHz burst starting on every whole second.
    fn bursts(t: f64) -> f32 {
        let phase = t.fract();
        if phase < 0.03 { (std::f64::consts::TAU * 1000.0 * phase).sin() as f32 * 0.5 } else { 0.0 }
    }

    fn step() -> f64 {
        SRC as f64 / OUT
    }

    fn render(st: &mut Stretcher, track: &TrackAudio, tempo: f64, secs: f64) -> (Vec<f32>, Vec<f64>) {
        let blocks = (secs * OUT) as usize / BLOCK;
        let (mut out, mut audible) = (Vec::new(), Vec::new());
        let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
        for _ in 0..blocks {
            audible.push(st.audible_position());
            st.render(track, step(), tempo, &mut l, &mut r);
            out.extend_from_slice(&l);
        }
        (out, audible)
    }

    fn amplitude(x: &[f32], freq: f64) -> f64 {
        let w = std::f64::consts::TAU * freq / OUT;
        let (mut re, mut im, mut norm) = (0.0, 0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            let h = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
            re += *v as f64 * h * (w * i as f64).cos();
            im += *v as f64 * h * (w * i as f64).sin();
            norm += h;
        }
        2.0 * (re * re + im * im).sqrt() / norm
    }

    fn onsets(x: &[f32]) -> Vec<usize> {
        let win = 48;
        let env: Vec<f32> = x.windows(win).map(|w| w.iter().map(|v| v.abs()).fold(0.0, f32::max)).collect();
        let peak = env.iter().cloned().fold(0.0, f32::max);
        let mut found = Vec::new();
        let mut armed = true;
        for (i, e) in env.iter().enumerate() {
            if armed && *e > peak * 0.5 {
                found.push(i + win);
                armed = false;
            } else if *e < peak * 0.05 {
                armed = true;
            }
        }
        found
    }

    fn max_second_difference(x: &[f32]) -> f32 {
        x.windows(3).map(|w| (w[0] - 2.0 * w[1] + w[2]).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn keeps_pitch_and_level_at_any_tempo() {
        let t = track(6.0, tones(&[220.0, 1500.0, 4000.0, 9000.0]));
        for tempo in [0.84, 1.0, 1.16] {
            let mut st = Stretcher::new(OUT, BLOCK);
            st.jump(0.0);
            let (out, _) = render(&mut st, &t, tempo, 4.0);
            let tail = &out[out.len() / 2..];
            for f in [220.0, 1500.0, 4000.0, 9000.0] {
                let db = 20.0 * (amplitude(tail, f) / 0.2).log10();
                assert!(db.abs() < 1.0, "tempo {tempo} {f} Hz: {db:.2} dB");
                if (tempo - 1.0).abs() > 0.01 {
                    assert!(amplitude(tail, f * tempo) < 0.02, "tempo {tempo}: energy at shifted {f} Hz");
                }
            }
        }
    }

    #[test]
    fn audible_position_matches_what_is_heard() {
        let t = track(12.0, bursts);
        for tempo in [1.0, 1.12, 0.9, 0.8] {
            let mut st = Stretcher::new(OUT, BLOCK);
            st.jump(0.4 * SRC as f64);
            let (out, audible) = render(&mut st, &t, tempo, 6.0);
            let heard = onsets(&out);
            assert!(heard.len() >= 4, "tempo {tempo}: {heard:?}");
            for frame in heard {
                let block = frame / BLOCK;
                let reported = audible[block] + (frame % BLOCK) as f64 * step() * tempo;
                let burst = (reported / SRC as f64).round() * SRC as f64;
                let err_ms = (reported - burst) / SRC as f64 * 1000.0;
                assert!(err_ms.abs() < 3.0, "tempo {tempo}: heard at {:.4}s, reported {:.4}s ({err_ms:.2} ms)", burst / SRC as f64, reported / SRC as f64);
            }
        }
    }

    #[test]
    fn a_jump_is_heard_immediately() {
        let t = track(12.0, bursts);
        let mut st = Stretcher::new(OUT, BLOCK);
        st.jump(2.5 * SRC as f64);
        render(&mut st, &t, 1.0, 1.0);
        st.jump(5.0 * SRC as f64);
        assert!((st.audible_position() - 5.0 * SRC as f64).abs() < 1.0);
        let (out, _) = render(&mut st, &t, 1.0, 0.2);
        let first = onsets(&out)[0];
        assert!(first < (0.005 * OUT) as usize, "burst at the cue heard after {:.1} ms", first as f64 / OUT * 1000.0);
    }

    #[test]
    fn jumps_and_tempo_changes_do_not_click() {
        let t = track(30.0, tones(&[440.0]));
        let mut st = Stretcher::new(OUT, BLOCK);
        st.jump(1.0 * SRC as f64);
        let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
        let mut out = Vec::new();
        for b in 0..900 {
            if b % 37 == 36 {
                st.jump((2.0 + b as f64 * 0.013) * SRC as f64);
            }
            let tempo = 1.0 + 0.1 * (b as f64 * 0.05).sin();
            st.render(&t, step(), tempo, &mut l, &mut r);
            out.extend_from_slice(&l);
        }
        let worst = max_second_difference(&out);
        assert!(worst < 0.01, "second difference {worst} (a steady 440 Hz sine reaches {:.4})", 0.2 * (std::f64::consts::TAU * 440.0 / OUT).powi(2));
    }

    #[test]
    fn rapid_jumps_do_not_click() {
        let t = track(30.0, tones(&[440.0]));
        let mut st = Stretcher::new(OUT, BLOCK);
        st.jump(SRC as f64);
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        let mut out = Vec::new();
        for b in 0..400 {
            if b > 20 && b < 200 {
                st.jump((1.0 + b as f64 * 0.0137) * SRC as f64);
            }
            st.render(&t, step(), 1.0, &mut l, &mut r);
            out.extend_from_slice(&l);
        }
        assert!(max_second_difference(&out) < 0.01);
    }

    #[test]
    fn release_fades_out_without_a_click() {
        let t = track(10.0, tones(&[440.0]));
        let mut st = Stretcher::new(OUT, BLOCK);
        st.jump(SRC as f64);
        let (mut out, _) = render(&mut st, &t, 1.05, 1.0);
        st.release();
        let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
        st.mix_release(&t, step(), 1.05, &mut l, &mut r);
        assert!(l[Stretcher::crossfade_frames()..].iter().all(|v| *v == 0.0), "tail ends after the crossfade");
        out.extend_from_slice(&l);
        st.mix_release(&t, step(), 1.05, &mut l, &mut r);
        assert!(max_second_difference(&out) < 0.01);
    }

    #[test]
    fn reads_outside_the_track_are_silent() {
        let t = track(2.0, tones(&[440.0]));
        let mut st = Stretcher::new(OUT, BLOCK);
        st.jump(-0.5 * SRC as f64);
        let (out, _) = render(&mut st, &t, 1.0, 0.3);
        assert!(out.iter().all(|v| v.abs() < 0.05));
        st.jump(3.0 * SRC as f64);
        let (out, _) = render(&mut st, &t, 1.4, 0.5);
        assert!(out[out.len() / 2..].iter().all(|v| v.abs() < 1e-3));
    }

    #[test]
    fn rendering_does_not_allocate() {
        let t = track(10.0, tones(&[440.0, 3000.0]));
        let mut st = Stretcher::new(OUT, BLOCK);
        st.jump(0.0);
        render(&mut st, &t, 1.1, 0.5);
        let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
        let ((), allocations) = allocations_in(|| {
            for b in 0..200 {
                if b % 50 == 0 {
                    st.jump(b as f64 * 1000.0);
                }
                st.render(&t, step(), 1.0 + b as f64 * 0.001, &mut l, &mut r);
            }
        });
        assert_eq!(allocations, 0);
    }
}
