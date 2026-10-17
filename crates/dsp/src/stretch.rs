use deckforge_common::TrackAudio;

use crate::interp::read_cubic;
use crate::signalsmith::Voice;

const CROSSFADE: usize = 96;
const MAX_TEMPO: f64 = 4.0;
const HISTORY: usize = 512;
const LANES: usize = 3;
/// Below this tempo the stretched output fades to silence instead of freezing on one spectrum.
const GATE_TEMPO: f64 = 0.02;

struct Lane {
    voice: Voice,
    read: f64,
    carry: f64,
    clock: u64,
    history: [(u64, f64); HISTORY],
    head: usize,
    filled: usize,
    weight: f32,
    target: f32,
    delta: f32,
}

impl Lane {
    fn new(out_rate: f64) -> Self {
        Self {
            voice: Voice::new(out_rate),
            read: 0.0,
            carry: 0.0,
            clock: 0,
            history: [(0, 0.0); HISTORY],
            head: 0,
            filled: 0,
            weight: 0.0,
            target: 0.0,
            delta: 0.0,
        }
    }

    fn busy(&self) -> bool {
        self.weight > 0.0 || self.target > 0.0
    }

    fn fade_to(&mut self, target: f32) {
        self.target = target;
        self.delta = ((target - self.weight).abs() / CROSSFADE as f32).max(1e-6);
    }

    /// Records which source position the stretcher is processing at the current output clock.
    fn record(&mut self, processing: f64) {
        self.head = (self.head + 1) % HISTORY;
        self.history[self.head] = (self.clock, processing);
        self.filled = (self.filled + 1).min(HISTORY);
    }

    /// Source position heard now: what was being processed `output_latency` frames ago.
    fn audible(&self, step: f64, tempo: f64) -> f64 {
        let at = self.clock as f64 - self.voice.output_latency() as f64;
        let mut newer = self.history[self.head];
        for k in 1..self.filled {
            let older = self.history[(self.head + HISTORY - k) % HISTORY];
            if (older.0 as f64) <= at {
                let span = (newer.0 - older.0) as f64;
                let t = if span > 0.0 { (at - older.0 as f64) / span } else { 1.0 };
                return older.1 + (newer.1 - older.1) * t;
            }
            newer = older;
        }
        newer.1 - (newer.0 as f64 - at) * tempo * step
    }
}

/// Key-lock time stretcher on Signalsmith Stretch. Every voice has its own gain ramp, so jumps, releases and
/// their overlaps always crossfade. Positions are source frames; `step` is source frames per output frame at native pitch.
pub struct Stretcher {
    lanes: [Lane; LANES],
    main: Option<usize>,
    pending: Option<f64>,
    last_audible: f64,
    gate: f32,
    tempo: f64,
    step: f64,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    tail_l: Vec<f32>,
    tail_r: Vec<f32>,
    mix_l: Vec<f32>,
    mix_r: Vec<f32>,
    mix_w: Vec<f32>,
}

impl Stretcher {
    pub fn new(out_rate: f64, max_block: usize) -> Self {
        let lanes = std::array::from_fn(|_| Lane::new(out_rate));
        let input = Voice::new(out_rate).preroll_frames().max((max_block as f64 * MAX_TEMPO) as usize + 2);
        Self {
            lanes,
            main: None,
            pending: None,
            last_audible: 0.0,
            gate: 1.0,
            tempo: 1.0,
            step: 1.0,
            in_l: vec![0.0; input],
            in_r: vec![0.0; input],
            tail_l: vec![0.0; max_block],
            tail_r: vec![0.0; max_block],
            mix_l: vec![0.0; max_block],
            mix_r: vec![0.0; max_block],
            mix_w: vec![0.0; max_block],
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
        match (self.pending, self.main) {
            (Some(p), _) => p,
            (None, Some(m)) => self.lanes[m].audible(self.step, self.tempo),
            (None, None) => self.last_audible,
        }
    }

    /// Fades everything out; `mix_release` crossfades that tail into the caller's output.
    pub fn release(&mut self) {
        self.last_audible = self.audible_position();
        self.pending = None;
        self.main = None;
        for lane in self.lanes.iter_mut().filter(|l| l.busy()) {
            lane.fade_to(0.0);
        }
    }

    /// Drops all voices at once; only for when the output is already silent.
    pub fn silence(&mut self) {
        self.release();
        for lane in &mut self.lanes {
            lane.weight = 0.0;
            lane.target = 0.0;
        }
    }

    pub fn mix_release(&mut self, track: &TrackAudio, step: f64, tempo: f64, out_l: &mut [f32], out_r: &mut [f32]) {
        if self.main.is_some() || !self.lanes.iter().any(Lane::busy) {
            return;
        }
        let n = out_l.len().min(out_r.len());
        self.compose(track, step, tempo, n);
        for i in 0..n {
            out_l[i] = out_l[i] * (1.0 - self.mix_w[i]) + self.mix_l[i];
            out_r[i] = out_r[i] * (1.0 - self.mix_w[i]) + self.mix_r[i];
        }
    }

    pub fn render(&mut self, track: &TrackAudio, step: f64, tempo: f64, out_l: &mut [f32], out_r: &mut [f32]) {
        let tempo = tempo.clamp(0.0, MAX_TEMPO);
        let n = out_l.len().min(out_r.len());
        self.step = step;
        self.tempo = tempo;
        if let Some(target) = self.pending
            && let Some(free) = self.lanes.iter().position(|l| !l.busy())
        {
            self.pending = None;
            for lane in self.lanes.iter_mut().filter(|l| l.busy()) {
                lane.fade_to(0.0);
            }
            self.seek(free, track, step, tempo, target);
            self.lanes[free].weight = 0.0;
            self.lanes[free].fade_to(1.0);
            self.main = Some(free);
        }
        self.compose(track, step, tempo, n);
        let gate_target = (tempo / GATE_TEMPO).min(1.0) as f32;
        for i in 0..n {
            self.gate += (gate_target - self.gate).clamp(-1.0 / CROSSFADE as f32, 1.0 / CROSSFADE as f32);
            out_l[i] = self.mix_l[i] * self.gate;
            out_r[i] = self.mix_r[i] * self.gate;
        }
        if let Some(m) = self.main {
            self.last_audible = self.lanes[m].audible(step, tempo);
        }
    }

    fn compose(&mut self, track: &TrackAudio, step: f64, tempo: f64, n: usize) {
        self.mix_l[..n].fill(0.0);
        self.mix_r[..n].fill(0.0);
        self.mix_w[..n].fill(0.0);
        for k in 0..LANES {
            if !self.lanes[k].busy() {
                continue;
            }
            self.process_lane(k, track, step, tempo, n);
            let lane = &mut self.lanes[k];
            for i in 0..n {
                let w = lane.weight;
                self.mix_l[i] += self.tail_l[i] * w;
                self.mix_r[i] += self.tail_r[i] * w;
                self.mix_w[i] += w;
                lane.weight += (lane.target - lane.weight).clamp(-lane.delta, lane.delta);
            }
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
        l.filled = 0;
        l.record(target);
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
        l.clock += n as u64;
        let processing = l.read - l.voice.input_latency() as f64 * step;
        l.record(processing);
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
    fn audible_position_moves_smoothly_through_tempo_changes() {
        let t = track(20.0, tones(&[440.0]));
        let mut st = Stretcher::new(OUT, BLOCK);
        st.jump(SRC as f64);
        let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
        let mut tempo = 1.0;
        for b in 0..600 {
            if b % 40 == 0 {
                tempo = [1.0, 0.85, 1.12, 1.4, 0.9][(b / 40) % 5];
            }
            let before = st.audible_position();
            st.render(&t, step(), tempo, &mut l, &mut r);
            let moved = st.audible_position() - before;
            if b > 2 {
                let lo = BLOCK as f64 * step() * 0.8;
                let hi = BLOCK as f64 * step() * 1.45;
                assert!(moved > lo && moved < hi, "block {b}: audible moved {moved:.1} frames at tempo {tempo}");
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
    fn releasing_in_the_middle_of_a_crossfade_does_not_click() {
        let t = track(10.0, tones(&[440.0]));
        for offset in [0usize, 16, 48, 80] {
            let mut st = Stretcher::new(OUT, BLOCK);
            st.jump(SRC as f64);
            let (mut out, _) = render(&mut st, &t, 1.0, 0.5);
            st.jump(4.37 * SRC as f64);
            let (mut l, mut r) = (vec![0.0; 32], vec![0.0; 32]);
            let mut done = 0;
            while done < offset {
                st.render(&t, step(), 1.0, &mut l, &mut r);
                out.extend_from_slice(&l);
                done += 32;
            }
            st.release();
            for _ in 0..8 {
                l.fill(0.0);
                r.fill(0.0);
                st.mix_release(&t, step(), 1.0, &mut l, &mut r);
                out.extend_from_slice(&l);
            }
            assert!(max_second_difference(&out) < 0.01, "release {offset} frames into a crossfade");
        }
    }

    #[test]
    fn zero_tempo_fades_to_silence_and_back() {
        let t = track(10.0, tones(&[440.0]));
        let mut st = Stretcher::new(OUT, BLOCK);
        st.jump(SRC as f64);
        let (mut out, _) = render(&mut st, &t, 1.0, 0.5);
        let (stopped, _) = render(&mut st, &t, 0.0, 1.0);
        assert!(stopped[stopped.len() / 2..].iter().all(|v| v.abs() < 1e-4), "silent at tempo 0");
        out.extend_from_slice(&stopped);
        let (back, _) = render(&mut st, &t, 1.0, 0.5);
        assert!(amplitude(&back[back.len() / 2..], 440.0) > 0.15, "plays again");
        out.extend_from_slice(&back);
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
    fn rendering_does_not_allocate_on_the_rust_side() {
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
