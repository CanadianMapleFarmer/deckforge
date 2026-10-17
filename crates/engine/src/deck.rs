use deckforge_common::{Beatgrid, HotCue, SharedTrack, TempoRange, HOT_CUE_PALETTE, HOT_CUE_SLOTS};
use deckforge_dsp::interp::read_cubic;
use deckforge_dsp::{Smoothed, Stretcher};

use crate::command::{JogMode, LoadRequest};
use crate::snapshot::DeckSnapshot;

pub const MAX_BLOCK: usize = 2048;
const DECLICK_FRAMES: usize = 96;
const SCRATCH_SECS_PER_REV: f64 = 1.8;
const SEARCH_SECS_PER_REV: f64 = 1.0;
const FAST_SEARCH_SECS_PER_REV: f64 = 16.0;
const BEND_PER_REV: f64 = 1.2;
const BEND_LIMIT: f64 = 0.5;
const BEND_DECAY_SECS: f64 = 0.12;
const SCRATCH_SMOOTHING: f64 = 0.35;
const CUE_EPSILON_SECS: f64 = 0.005;
/// A loop pressed this close before a beat starts on that beat, so a slightly early press still catches it.
const LOOP_LEAD_BEATS: f64 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewKind {
    Cue,
    HotCue(usize),
}

#[derive(Debug, Clone, Copy)]
struct Preview {
    kind: PreviewKind,
    return_secs: f64,
    latched: bool,
}

#[derive(Debug, Clone, Copy)]
struct Declick {
    old_pos: f64,
    remaining: usize,
}

pub struct Deck {
    track: Option<SharedTrack>,
    token: u64,
    out_rate: f64,
    position: f64,
    playing: bool,
    at_end: bool,
    fader_ratio: f64,
    tempo_range: TempoRange,
    sync_ratio: f64,
    sync_nudge: f64,
    sync: bool,
    key_lock: bool,
    vinyl: bool,
    bend: f64,
    grid: Option<Beatgrid>,
    phrase_starts: Box<[f64]>,
    cue_secs: f64,
    hot_cues: [Option<HotCue>; HOT_CUE_SLOTS],
    preview: Option<Preview>,
    loop_in: Option<f64>,
    loop_out: Option<f64>,
    loop_active: bool,
    jog_touched: bool,
    scratch_revs: f64,
    scratch_rate: f64,
    fade: Smoothed,
    pause_return: Option<f64>,
    declick: Option<Declick>,
    stretcher: Stretcher,
    stretching: bool,
    pub buf_l: Vec<f32>,
    pub buf_r: Vec<f32>,
}

impl Deck {
    pub fn new(out_rate: f64) -> Self {
        Self {
            track: None,
            token: 0,
            out_rate,
            position: 0.0,
            playing: false,
            at_end: false,
            fader_ratio: 1.0,
            tempo_range: TempoRange::default(),
            sync_ratio: 1.0,
            sync_nudge: 0.0,
            sync: false,
            key_lock: false,
            vinyl: true,
            bend: 0.0,
            grid: None,
            phrase_starts: Box::default(),
            cue_secs: 0.0,
            hot_cues: [None; HOT_CUE_SLOTS],
            preview: None,
            loop_in: None,
            loop_out: None,
            loop_active: false,
            jog_touched: false,
            scratch_revs: 0.0,
            scratch_rate: 0.0,
            fade: Smoothed::new(0.0, out_rate as f32, 2.0),
            pause_return: None,
            declick: None,
            stretcher: Stretcher::new(out_rate, MAX_BLOCK),
            stretching: false,
            buf_l: vec![0.0; MAX_BLOCK],
            buf_r: vec![0.0; MAX_BLOCK],
        }
    }

    pub fn set_output_rate(&mut self, out_rate: f64) {
        let audible = self.to_frames(self.audible_secs());
        self.out_rate = out_rate;
        self.fade.set_time(out_rate as f32, 2.0);
        self.stretcher = Stretcher::new(out_rate, MAX_BLOCK);
        if self.stretching {
            self.position = audible;
            self.stretcher.jump(audible);
        }
    }

    fn src_rate(&self) -> f64 {
        self.track.as_ref().map_or(44_100.0, |t| t.sample_rate as f64)
    }

    fn frames(&self) -> f64 {
        self.track.as_ref().map_or(0.0, |t| t.frames() as f64)
    }

    fn to_secs(&self, frames: f64) -> f64 {
        frames / self.src_rate()
    }

    fn to_frames(&self, secs: f64) -> f64 {
        secs * self.src_rate()
    }

    pub fn is_loaded(&self) -> bool {
        self.track.is_some()
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn grid(&self) -> Option<Beatgrid> {
        self.grid
    }

    pub fn track(&self) -> Option<&SharedTrack> {
        self.track.as_ref()
    }

    pub fn phrase_starts(&self) -> &[f64] {
        &self.phrase_starts
    }

    /// Swaps in new phrase starts and returns the old ones so the caller can drop them off the audio thread.
    pub fn replace_phrase_starts(&mut self, starts: Box<[f64]>) -> Box<[f64]> {
        std::mem::replace(&mut self.phrase_starts, starts)
    }

    pub fn source_rate(&self) -> f64 {
        self.src_rate()
    }

    pub fn out_rate(&self) -> f64 {
        self.out_rate
    }

    /// Track seconds that pass per output second right now, including nudges.
    pub fn play_rate(&self) -> f64 {
        self.tempo_ratio() * (1.0 + self.bend) * (1.0 + self.sync_nudge)
    }

    pub fn sync_enabled(&self) -> bool {
        self.sync
    }

    pub fn tempo_ratio(&self) -> f64 {
        if self.sync { self.sync_ratio } else { self.fader_ratio }
    }

    pub fn effective_bpm(&self) -> Option<f64> {
        self.grid.map(|g| g.bpm * self.tempo_ratio())
    }

    /// Track time currently audible, compensating for time-stretch latency.
    pub fn audible_secs(&self) -> f64 {
        let frames = if self.stretching {
            self.stretcher.audible_position()
        } else {
            self.position
        };
        self.to_secs(frames)
    }

    pub fn beat_position(&self) -> Option<f64> {
        self.grid.map(|g| g.beat_at(self.audible_secs()))
    }

    pub fn beat_phase(&self) -> Option<f64> {
        self.grid.map(|g| g.phase_at(self.audible_secs()))
    }

    pub fn set_sync_state(&mut self, ratio: f64, nudge: f64) {
        self.sync_ratio = ratio;
        self.sync_nudge = nudge;
    }

    /// Returns the previously loaded track so the caller can release it off the audio thread.
    pub fn load(&mut self, req: &LoadRequest) -> Option<SharedTrack> {
        let old = self.track.replace(req.track.clone());
        self.token = req.token;
        self.grid = req.grid;
        self.cue_secs = req.cue_secs.unwrap_or_else(|| req.grid.map_or(0.0, |g| g.first_beat_secs.max(0.0)));
        self.hot_cues = req.hot_cues;
        self.playing = false;
        self.at_end = false;
        self.preview = None;
        self.loop_in = None;
        self.loop_out = None;
        self.loop_active = false;
        self.bend = 0.0;
        self.fade.reset(0.0);
        self.pause_return = None;
        self.declick = None;
        self.position = self.to_frames(self.cue_secs);
        self.stretcher.jump(self.position);
        old
    }

    pub fn eject(&mut self) -> Option<SharedTrack> {
        if self.playing {
            return None;
        }
        self.preview = None;
        self.grid = None;
        self.hot_cues = [None; HOT_CUE_SLOTS];
        self.sync = false;
        self.token = 0;
        self.track.take()
    }

    fn jump_frames(&mut self, target: f64) {
        let target = target.clamp(0.0, self.frames());
        if self.fade.value() > 0.0 && !self.stretching {
            self.declick = Some(Declick { old_pos: self.position, remaining: DECLICK_FRAMES });
        }
        self.position = target;
        self.stretcher.jump(target);
        self.at_end = false;
    }

    fn jump_secs(&mut self, secs: f64) {
        let f = self.to_frames(secs);
        self.jump_frames(f);
    }

    fn snap_to_beat(&self, secs: f64, quantize: bool) -> f64 {
        match (quantize, self.grid) {
            (true, Some(g)) => g.nearest_beat_secs(secs),
            _ => secs,
        }
    }

    /// Jump target that keeps the current beat phase when quantize is on and the deck is playing.
    fn phase_locked_target(&self, target_secs: f64, quantize: bool) -> f64 {
        match (quantize && self.playing, self.grid) {
            (true, Some(g)) => {
                let mut diff = g.phase_at(self.audible_secs()) - g.phase_at(target_secs);
                diff -= diff.round();
                target_secs + diff * g.beat_secs()
            }
            _ => target_secs,
        }
    }

    pub fn play(&mut self) {
        if !self.is_loaded() {
            return;
        }
        if let Some(p) = &mut self.preview {
            p.latched = true;
            return;
        }
        if self.at_end {
            return;
        }
        self.playing = true;
        self.pause_return = None;
        self.fade.set(1.0);
    }

    pub fn pause(&mut self) {
        if let Some(p) = self.preview.take() {
            if p.latched {
                self.stop_at(self.position);
            }
            return;
        }
        if self.playing {
            self.stop_at(self.position);
        }
    }

    fn stop_at(&mut self, frames: f64) {
        self.playing = false;
        self.fade.set(0.0);
        self.pause_return = Some(frames);
        self.bend = 0.0;
    }

    pub fn toggle_play(&mut self) {
        let latched = self.preview.is_some_and(|p| p.latched);
        if self.playing && (self.preview.is_none() || latched) {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn cue_down(&mut self, quantize: bool) {
        if !self.is_loaded() {
            return;
        }
        if self.playing && self.preview.is_none() {
            let cue = self.cue_secs;
            self.jump_secs(cue);
            self.stop_at(self.position);
            return;
        }
        if self.preview.is_some() {
            return;
        }
        let here = self.audible_secs();
        if (here - self.cue_secs).abs() > CUE_EPSILON_SECS {
            self.cue_secs = self.snap_to_beat(here, quantize);
            let cue = self.cue_secs;
            self.jump_secs(cue);
        }
        self.preview = Some(Preview { kind: PreviewKind::Cue, return_secs: self.cue_secs, latched: false });
        self.start_preview_playback();
    }

    pub fn cue_up(&mut self) {
        self.end_preview(PreviewKind::Cue);
    }

    fn start_preview_playback(&mut self) {
        self.playing = true;
        self.at_end = false;
        self.pause_return = None;
        self.fade.set(1.0);
    }

    fn end_preview(&mut self, kind: PreviewKind) {
        let Some(p) = self.preview else { return };
        if p.kind != kind {
            return;
        }
        self.preview = None;
        if !p.latched {
            let target = self.to_frames(p.return_secs);
            self.playing = false;
            self.fade.set(0.0);
            self.pause_return = Some(target);
        }
    }

    pub fn hot_cue(&mut self, slot: usize, pressed: bool, quantize: bool) {
        if slot >= HOT_CUE_SLOTS || !self.is_loaded() {
            return;
        }
        if !pressed {
            self.end_preview(PreviewKind::HotCue(slot));
            return;
        }
        match self.hot_cues[slot] {
            None => {
                let secs = self.snap_to_beat(self.audible_secs(), quantize);
                self.hot_cues[slot] = Some(HotCue { position_secs: secs, color: HOT_CUE_PALETTE[slot] });
            }
            Some(cue) if self.playing && self.preview.is_none() => {
                let target = self.phase_locked_target(cue.position_secs, quantize);
                self.jump_secs(target);
            }
            Some(cue) => {
                if self.preview.is_some() {
                    return;
                }
                self.jump_secs(cue.position_secs);
                self.preview = Some(Preview {
                    kind: PreviewKind::HotCue(slot),
                    return_secs: cue.position_secs,
                    latched: false,
                });
                self.start_preview_playback();
            }
        }
    }

    pub fn delete_hot_cue(&mut self, slot: usize) {
        if slot < HOT_CUE_SLOTS {
            self.hot_cues[slot] = None;
        }
    }

    pub fn set_hot_cue_color(&mut self, slot: usize, color: u32) {
        if let Some(Some(c)) = self.hot_cues.get_mut(slot) {
            c.color = color;
        }
    }

    pub fn seek_secs(&mut self, secs: f64) {
        if self.is_loaded() {
            self.jump_secs(secs.max(0.0));
            if !self.playing {
                self.pause_return = None;
            }
        }
    }

    pub fn set_tempo(&mut self, ratio: f64) {
        let r = self.tempo_range.fraction();
        self.fader_ratio = ratio.clamp(1.0 - r, 1.0 + r);
    }

    pub fn set_pitch_fader(&mut self, position: f64) {
        if !self.sync {
            self.fader_ratio = 1.0 + position.clamp(-1.0, 1.0) * self.tempo_range.fraction();
        }
    }

    pub fn set_tempo_range(&mut self, range: TempoRange) {
        self.tempo_range = range;
        self.set_tempo(self.fader_ratio);
    }

    pub fn set_key_lock(&mut self, enabled: bool) {
        self.key_lock = enabled;
    }

    pub fn set_vinyl(&mut self, enabled: bool) {
        self.vinyl = enabled;
    }

    pub fn set_sync(&mut self, enabled: bool) {
        if self.sync && !enabled {
            self.fader_ratio = self.sync_ratio;
            self.sync_nudge = 0.0;
        }
        self.sync = enabled;
    }

    pub fn jog_touch(&mut self, touched: bool) {
        self.jog_touched = touched && self.vinyl;
        if self.jog_touched {
            let step = self.src_rate() / self.out_rate;
            self.scratch_rate = if self.playing { step * self.tempo_ratio() } else { 0.0 };
            self.scratch_revs = 0.0;
            if self.stretching {
                self.position = self.stretcher.audible_position();
                self.stretcher.release();
                self.stretching = false;
            }
        }
    }

    pub fn jog(&mut self, revolutions: f64, mode: JogMode) {
        if !self.is_loaded() {
            return;
        }
        match mode {
            JogMode::Scratch if self.jog_touched => self.scratch_revs += revolutions,
            JogMode::Scratch | JogMode::Bend if self.playing => {
                self.bend = (self.bend + revolutions * BEND_PER_REV).clamp(-BEND_LIMIT, BEND_LIMIT);
            }
            JogMode::FastSearch => {
                let delta = revolutions * FAST_SEARCH_SECS_PER_REV * self.src_rate();
                self.jump_frames(self.position + delta);
            }
            _ => {
                let delta = revolutions * SEARCH_SECS_PER_REV * self.src_rate();
                self.jump_frames(self.position + delta);
                if !self.playing {
                    self.pause_return = None;
                }
            }
        }
    }

    pub fn loop_in(&mut self, quantize: bool) {
        if !self.is_loaded() {
            return;
        }
        self.loop_in = Some(self.snap_to_beat(self.audible_secs(), quantize));
        self.loop_out = None;
        self.loop_active = false;
    }

    pub fn loop_out(&mut self, quantize: bool) {
        let Some(start) = self.loop_in else { return };
        let mut end = self.snap_to_beat(self.audible_secs(), quantize);
        if let Some(g) = self.grid.filter(|_| quantize)
            && end <= start + 1e-3
        {
            end = start + g.beat_secs();
        }
        if end > start + 0.01 {
            self.loop_out = Some(end);
            self.loop_active = true;
        }
    }

    pub fn reloop_exit(&mut self) {
        if self.loop_active {
            self.loop_active = false;
        } else if let (Some(start), Some(_)) = (self.loop_in, self.loop_out) {
            self.loop_active = true;
            self.jump_secs(start);
        }
    }

    pub fn beat_loop(&mut self, beats: f64, quantize: bool) {
        if !self.is_loaded() {
            return;
        }
        let beat = self.grid.map_or(0.5, |g| g.beat_secs());
        let len = beats * beat;
        if self.loop_active
            && let (Some(a), Some(b)) = (self.loop_in, self.loop_out)
            && ((b - a) - len).abs() < 1e-3
        {
            self.loop_active = false;
            return;
        }
        let here = self.audible_secs();
        let start = match (quantize, self.grid) {
            (true, Some(g)) => g.secs_of_beat((g.beat_at(here) + LOOP_LEAD_BEATS).floor()),
            _ => here,
        };
        self.loop_in = Some(start);
        self.loop_out = Some(start + len);
        self.loop_active = true;
    }

    pub fn loop_scale(&mut self, factor: f64) {
        if let (Some(a), Some(b)) = (self.loop_in, self.loop_out) {
            let len = ((b - a) * factor).max(0.01);
            self.loop_out = Some(a + len);
            let here = self.audible_secs();
            if self.loop_active && here >= a + len {
                self.jump_secs(a + (here - a) % len);
            }
        }
    }

    pub fn beat_jump(&mut self, beats: f64) {
        if !self.is_loaded() {
            return;
        }
        let delta = beats * self.grid.map_or(0.5, |g| g.beat_secs());
        if self.loop_active
            && let (Some(a), Some(b)) = (self.loop_in, self.loop_out)
        {
            self.loop_in = Some(a + delta);
            self.loop_out = Some(b + delta);
        }
        let target = self.audible_secs() + delta;
        self.jump_secs(target.max(0.0));
        if !self.playing {
            self.pause_return = None;
        }
    }

    pub fn set_grid(&mut self, grid: Option<Beatgrid>) {
        self.grid = grid;
    }

    fn loop_frames(&self) -> Option<(f64, f64)> {
        match (self.loop_active, self.loop_in, self.loop_out) {
            (true, Some(a), Some(b)) => Some((self.to_frames(a), self.to_frames(b))),
            _ => None,
        }
    }

    /// Renders `n` frames into `buf_l`/`buf_r`. Returns true if any audio was produced.
    pub fn render(&mut self, n: usize) -> bool {
        debug_assert!(n <= MAX_BLOCK);
        let Some(track) = self.track.clone() else {
            self.buf_l[..n].fill(0.0);
            self.buf_r[..n].fill(0.0);
            return false;
        };

        let decay = (-(n as f64) / (BEND_DECAY_SECS * self.out_rate)).exp();
        self.bend *= decay;
        if self.bend.abs() < 1e-4 {
            self.bend = 0.0;
        }

        let step = track.sample_rate as f64 / self.out_rate;
        let tempo = self.tempo_ratio() * (1.0 + self.bend) * (1.0 + self.sync_nudge);
        let audible = self.playing || self.jog_touched || self.fade.value() > 0.0;
        if !audible {
            self.buf_l[..n].fill(0.0);
            self.buf_r[..n].fill(0.0);
            return false;
        }
        if self.jog_touched {
            self.fade.set(1.0);
            let measured = self.scratch_revs * SCRATCH_SECS_PER_REV * track.sample_rate as f64 / n as f64;
            self.scratch_rate += (measured - self.scratch_rate) * SCRATCH_SMOOTHING;
            self.scratch_revs = 0.0;
        }

        let use_stretch = self.key_lock && !self.jog_touched;
        if use_stretch != self.stretching {
            if use_stretch {
                self.declick = Some(Declick { old_pos: self.position, remaining: DECLICK_FRAMES });
                self.stretcher.jump(self.position);
            } else {
                self.position = self.stretcher.audible_position();
                self.stretcher.release();
            }
            self.stretching = use_stretch;
        }

        let frames = track.frames() as f64;
        let loop_range = self.loop_frames();
        let out_l = &mut self.buf_l[..n];
        let out_r = &mut self.buf_r[..n];
        if self.stretching {
            let rate = step * tempo;
            let mut done = 0;
            while done < n {
                let mut chunk = n - done;
                if let Some((a, b)) = loop_range {
                    let audible = self.stretcher.audible_position();
                    if audible >= b {
                        let len = (b - a).max(1.0);
                        self.stretcher.jump(a + (audible - b) % len);
                    } else if rate > 0.0 {
                        chunk = chunk.min(((b - audible) / rate).ceil().max(1.0) as usize);
                    }
                }
                self.stretcher.render(&track, step, tempo, &mut out_l[done..done + chunk], &mut out_r[done..done + chunk]);
                done += chunk;
            }
            if let Some(d) = &mut self.declick {
                for i in 0..n.min(d.remaining) {
                    let (ol, or) = read_cubic(&track, d.old_pos);
                    let g = (d.remaining - i) as f32 / DECLICK_FRAMES as f32;
                    out_l[i] += ol * g;
                    out_r[i] += or * g;
                    d.old_pos += rate;
                }
                self.declick = None;
            }
            self.position = self.stretcher.audible_position();
        } else {
            let rate = if self.jog_touched { self.scratch_rate } else { step * tempo };
            for i in 0..n {
                let (mut l, mut r) = read_cubic(&track, self.position);
                if let Some(d) = &mut self.declick {
                    let (ol, or) = read_cubic(&track, d.old_pos);
                    let g = 1.0 - d.remaining as f32 / DECLICK_FRAMES as f32;
                    l = l * g + ol * (1.0 - g);
                    r = r * g + or * (1.0 - g);
                    d.old_pos += rate;
                    d.remaining -= 1;
                    if d.remaining == 0 {
                        self.declick = None;
                    }
                }
                out_l[i] = l;
                out_r[i] = r;
                self.position += rate;
                if let Some((a, b)) = loop_range
                    && rate > 0.0
                    && self.position >= b
                {
                    let len = (b - a).max(1.0);
                    let wrapped = a + (self.position - b) % len;
                    self.declick = Some(Declick { old_pos: self.position, remaining: DECLICK_FRAMES });
                    self.position = wrapped;
                }
                if self.position < 0.0 {
                    self.position = 0.0;
                }
            }
            self.stretcher.mix_release(&track, step, tempo, out_l, out_r);
        }

        for i in 0..n {
            let g = self.fade.next();
            out_l[i] *= g;
            out_r[i] *= g;
        }
        if !self.playing && self.fade.is_settled() && self.fade.value() == 0.0 {
            if let Some(ret) = self.pause_return.take() {
                self.position = ret;
                self.stretcher.release();
                self.stretcher.jump(ret);
            }
            self.declick = None;
        }
        if self.position >= frames && self.playing && !self.jog_touched {
            self.position = frames;
            self.playing = false;
            self.at_end = true;
            self.preview = None;
            self.fade.set(0.0);
        }
        true
    }

    pub fn snapshot(&self, is_master: bool) -> DeckSnapshot {
        let grid = self.grid;
        DeckSnapshot {
            loaded: self.is_loaded(),
            track_token: self.token,
            playing: self.playing,
            position_secs: self.audible_secs(),
            duration_secs: self.to_secs(self.frames()),
            tempo_ratio: self.tempo_ratio(),
            fader_tempo_ratio: self.fader_ratio,
            tempo_range: self.tempo_range,
            key_lock: self.key_lock,
            vinyl_mode: self.vinyl,
            has_grid: grid.is_some(),
            grid_bpm: grid.map_or(0.0, |g| g.bpm),
            first_beat_secs: grid.map_or(0.0, |g| g.first_beat_secs),
            bpm: self.effective_bpm().unwrap_or(0.0),
            beat_phase: self.beat_phase().unwrap_or(0.0),
            cue_secs: self.cue_secs,
            hot_cues: self.hot_cues,
            loop_in_secs: self.loop_in,
            loop_out_secs: self.loop_out,
            loop_active: self.loop_active,
            sync_enabled: self.sync,
            is_master,
            jog_touched: self.jog_touched,
            previewing: self.preview.is_some(),
            at_end: self.at_end,
        }
    }
}
