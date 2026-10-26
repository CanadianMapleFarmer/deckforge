use deckforge_common::{
    beat_fx_params, color_fx_params, db_to_gain, snap_beat_fx_beats_for, snap_release_fx_beats, Band, BeatFxKind, ColorFxKind, CrossfaderCurve, EqCurve, FxBands,
    FxTarget, FxTrigger, PadVoice, ReleaseFxKind, SmartCfxPreset, BEAT_FX_PERCENT_DEFAULT, COLOR_FX_MAX_PARAMS, DECK_COUNT, RELEASE_FX_DEFAULT_BEATS,
};
use deckforge_dsp::{
    channel_fader_gain, crossfader_gains, BeatFx, ChannelEq, ColorFx, DjFilter, PeakLimiter, PeakMeter, ReleaseFx, SmartCfx, Smoothed, Transport,
};

use crate::command::MixerParam;
use crate::deck::MAX_BLOCK;
use crate::snapshot::{BeatFxSnapshot, ChannelSnapshot, MasterSnapshot, PadFxSnapshot, ReleaseFxSnapshot, SmartCfxSnapshot};

const TRIM_RANGE_DB: f32 = 12.0;
const SMOOTH_MS: f32 = 15.0;
const CLIP_HOLD_SECS: f32 = 1.0;
const COLOR_FILTER_PAIR_SCALE: f32 = 0.5;
const XF_PICKUP_WINDOW: f32 = 0.02;
const LIMITING_GAIN: f32 = 0.999;

fn backstop(x: f32) -> f32 {
    x.clamp(-1.0, 1.0)
}

/// Knob 0..1 → gain: -inf..0 dB below centre is not wanted for trim, so trim spans ±12 dB.
pub fn trim_gain(value: f32) -> f32 {
    db_to_gain((value.clamp(0.0, 1.0) - 0.5) * 2.0 * TRIM_RANGE_DB)
}

/// Master knob 0..1 → gain with unity at 0.8 and +3 dB at the top.
pub fn master_gain(value: f32) -> f32 {
    let v = value.clamp(0.0, 1.0);
    if v <= 0.0 { 0.0 } else if v >= 0.8 { db_to_gain((v - 0.8) / 0.2 * 3.0) } else { (v / 0.8).powi(2) }
}

pub struct Channel {
    pub trim: f32,
    pub eq: [f32; 3],
    pub filter_value: f32,
    pub fader: f32,
    pub pfl: bool,
    trim_gain: Smoothed,
    fader_gain: Smoothed,
    channel_eq: ChannelEq,
    filter: DjFilter,
    color: ColorFx,
    color_kind: ColorFxKind,
    smart: SmartCfx,
    smart_on: bool,
    pad: BeatFx,
    pad_voice: Option<PadVoice>,
    release: ReleaseFx,
    bpm: f64,
    meter: [PeakMeter; 2],
}

impl Channel {
    fn new(sr: f32, seed: u32) -> Self {
        Self {
            trim: 0.5,
            eq: [0.5; 3],
            filter_value: 0.0,
            fader: 0.0,
            pfl: false,
            trim_gain: Smoothed::new(1.0, sr, SMOOTH_MS),
            fader_gain: Smoothed::new(0.0, sr, SMOOTH_MS),
            channel_eq: ChannelEq::new(sr),
            filter: DjFilter::new(sr),
            color: ColorFx::new(sr, seed),
            color_kind: ColorFxKind::Filter,
            smart: SmartCfx::new(sr, seed.wrapping_mul(97)),
            smart_on: false,
            pad: BeatFx::new(sr),
            pad_voice: None,
            release: ReleaseFx::new(sr),
            bpm: 120.0,
            meter: [PeakMeter::default(); 2],
        }
    }

    fn refresh_filter(&mut self) {
        if self.smart_on {
            self.filter.set(self.smart.filter_value());
            return;
        }
        let scale = match self.color_kind {
            ColorFxKind::Filter => 1.0,
            ColorFxKind::Space | ColorFxKind::Noise => 0.0,
            _ => COLOR_FILTER_PAIR_SCALE,
        };
        self.filter.set(self.filter_value * scale);
    }

    fn set_color_kind(&mut self, kind: ColorFxKind) {
        self.color_kind = kind;
        self.color.set_kind(kind);
        self.refresh_filter();
    }

    fn apply(&mut self, param: MixerParam) {
        match param {
            MixerParam::Trim(v) => {
                self.trim = v.clamp(0.0, 1.0);
                self.trim_gain.set(trim_gain(self.trim));
            }
            MixerParam::Eq(band, v) => {
                let i = match band {
                    Band::Low => 0,
                    Band::Mid => 1,
                    Band::High => 2,
                };
                self.eq[i] = v.clamp(0.0, 1.0);
                self.channel_eq.set_knob(i, self.eq[i]);
            }
            MixerParam::Filter(v) => {
                self.filter_value = v.clamp(-1.0, 1.0);
                if self.smart_on {
                    self.smart.set_value(self.filter_value);
                } else {
                    self.color.set_value(self.filter_value);
                }
                self.refresh_filter();
            }
            MixerParam::Fader(v) => {
                self.fader = v.clamp(0.0, 1.0);
                self.fader_gain.set(channel_fader_gain(self.fader));
            }
            MixerParam::Pfl(on) => self.pfl = on,
        }
    }

    /// Processes the deck signal in place up to the pre-fader point (trim, EQ, colour FX, filter).
    fn process_prefader(&mut self, l: &mut [f32], r: &mut [f32]) {
        for i in 0..l.len() {
            let t = self.trim_gain.next();
            let (yl, yr) = self.channel_eq.process(l[i] * t, r[i] * t);
            l[i] = yl;
            r[i] = yr;
        }
        self.color.process(l, r);
        self.smart.process(l, r);
        self.filter.process_block(l, r);
        self.pad.process(l, r);
    }

    /// Switching either way starts from a centred knob, so nothing lurches.
    fn set_smart(&mut self, on: bool) {
        if on == self.smart_on {
            return;
        }
        self.filter_value = 0.0;
        self.color.set_value(0.0);
        self.smart.set_value(0.0);
        self.smart_on = on;
        self.refresh_filter();
    }

    fn set_pad_voice(&mut self, voice: Option<PadVoice>, beat_position: Option<f64>) {
        let Some(v) = voice else {
            self.pad_voice = None;
            self.pad.set_enabled(false);
            return;
        };
        let retune = self.pad_voice.is_some_and(|old| old.kind == v.kind) && self.pad.is_enabled();
        if !retune {
            self.pad.set_kind(v.kind);
            self.pad.set_bands(v.kind.default_bands());
        }
        if let Some(beats) = snap_beat_fx_beats_for(v.kind, v.beats) {
            self.pad.set_beats(beats as f32);
        }
        self.pad.set_percent(v.percent as f32);
        self.pad.set_level(v.level);
        if let Some(spec) = beat_fx_params(v.kind).first() {
            self.pad.set_param(0, v.param0.unwrap_or(spec.default));
        }
        self.pad_voice = Some(v);
        if !retune {
            self.pad.set_enabled(true);
            if let Some(pos) = beat_position {
                self.pad.align_phase(pos);
            }
        }
    }

    fn pad_snapshot(&self) -> PadFxSnapshot {
        PadFxSnapshot {
            voice: self.pad_voice,
            sounding: !self.pad.is_idle(),
            release: self.release.running(),
            release_progress: self.release.progress().unwrap_or(0.0),
        }
    }

    fn feed_meter(&mut self, l: &[f32], r: &[f32]) {
        for i in 0..l.len() {
            self.meter[0].feed(l[i]);
            self.meter[1].feed(r[i]);
        }
    }

    fn snapshot(&self) -> ChannelSnapshot {
        ChannelSnapshot {
            trim: self.trim,
            eq_low: self.eq[0],
            eq_mid: self.eq[1],
            eq_high: self.eq[2],
            filter: self.filter_value,
            fader: self.fader,
            pfl: self.pfl,
            meter_l: self.meter[0].level(),
            meter_r: self.meter[1].level(),
        }
    }
}

pub struct Mixer {
    sample_rate: f32,
    pub channels: [Channel; DECK_COUNT],
    pub crossfader: f32,
    curve: CrossfaderCurve,
    xf_curve: CrossfaderCurve,
    xf_pickup: bool,
    xf_engage: Smoothed,
    pub level: f32,
    pub headphone_mix: f32,
    pub headphone_level: f32,
    xf: Smoothed,
    master: Smoothed,
    hp_mix: Smoothed,
    hp_level: Smoothed,
    pub master_l: Vec<f32>,
    pub master_r: Vec<f32>,
    pub cue_l: Vec<f32>,
    pub cue_r: Vec<f32>,
    pub phones_l: Vec<f32>,
    pub phones_r: Vec<f32>,
    pub rec_l: Vec<f32>,
    pub rec_r: Vec<f32>,
    master_meter: [PeakMeter; 2],
    cue_meter: [PeakMeter; 2],
    master_limiter: PeakLimiter,
    rec_limiter: PeakLimiter,
    phones_limiter: PeakLimiter,
    clip_hold: f32,
    limiting: bool,
    pub hardware_phones: bool,
    beat_fx: BeatFx,
    fx_target: FxTarget,
    fx_applied: FxTarget,
    fx_handoff: bool,
    fx_beats: f64,
    fx_percent: f64,
    fx_bpm: f64,
    color_fx: ColorFxKind,
    color_params: [[f32; COLOR_FX_MAX_PARAMS]; ColorFxKind::ALL.len()],
    release: ReleaseFx,
    release_kind: ReleaseFxKind,
    release_beats: f64,
    smart_on: bool,
    smart_preset: SmartCfxPreset,
}

impl Mixer {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            channels: [Channel::new(sample_rate, 1), Channel::new(sample_rate, 2)],
            crossfader: 0.5,
            curve: CrossfaderCurve::default(),
            xf_curve: CrossfaderCurve::default(),
            xf_pickup: false,
            xf_engage: Smoothed::new(1.0, sample_rate, SMOOTH_MS),
            level: 0.8,
            headphone_mix: 0.0,
            headphone_level: 0.7,
            xf: Smoothed::new(0.5, sample_rate, SMOOTH_MS),
            master: Smoothed::new(master_gain(0.8), sample_rate, SMOOTH_MS),
            hp_mix: Smoothed::new(0.0, sample_rate, SMOOTH_MS),
            hp_level: Smoothed::new(master_gain(0.7), sample_rate, SMOOTH_MS),
            master_l: vec![0.0; MAX_BLOCK],
            master_r: vec![0.0; MAX_BLOCK],
            cue_l: vec![0.0; MAX_BLOCK],
            cue_r: vec![0.0; MAX_BLOCK],
            phones_l: vec![0.0; MAX_BLOCK],
            phones_r: vec![0.0; MAX_BLOCK],
            rec_l: vec![0.0; MAX_BLOCK],
            rec_r: vec![0.0; MAX_BLOCK],
            master_meter: [PeakMeter::default(); 2],
            cue_meter: [PeakMeter::default(); 2],
            master_limiter: PeakLimiter::new(sample_rate),
            rec_limiter: PeakLimiter::new(sample_rate),
            phones_limiter: PeakLimiter::new(sample_rate),
            clip_hold: 0.0,
            limiting: false,
            hardware_phones: false,
            beat_fx: BeatFx::new(sample_rate),
            fx_target: FxTarget::default(),
            fx_applied: FxTarget::default(),
            fx_handoff: false,
            fx_beats: 1.0,
            fx_percent: BEAT_FX_PERCENT_DEFAULT,
            fx_bpm: 120.0,
            color_fx: ColorFxKind::default(),
            color_params: ColorFxKind::ALL.map(|kind| {
                let mut values = [0.0; COLOR_FX_MAX_PARAMS];
                for (v, spec) in values.iter_mut().zip(color_fx_params(kind)) {
                    *v = spec.default;
                }
                values
            }),
            release: ReleaseFx::new(sample_rate),
            release_kind: ReleaseFxKind::default(),
            release_beats: RELEASE_FX_DEFAULT_BEATS,
            smart_on: false,
            smart_preset: SmartCfxPreset::default(),
        }
    }

    /// Rebuilds sample-rate dependent state; called off the audio thread.
    pub fn rebuilt_for_rate(&self, sample_rate: f32) -> Self {
        let mut m = Mixer::new(sample_rate);
        for (i, ch) in self.channels.iter().enumerate() {
            m.channel(i, MixerParam::Trim(ch.trim));
            for (band, v) in [(Band::Low, ch.eq[0]), (Band::Mid, ch.eq[1]), (Band::High, ch.eq[2])] {
                m.channel(i, MixerParam::Eq(band, v));
            }
            m.channel(i, MixerParam::Filter(ch.filter_value));
            m.channel(i, MixerParam::Fader(ch.fader));
            m.channel(i, MixerParam::Pfl(ch.pfl));
        }
        m.set_crossfader(self.crossfader);
        m.curve = self.curve;
        m.xf_curve = self.xf_curve;
        m.xf_pickup = self.xf_pickup;
        m.xf_engage.reset(if m.crossfader_active() { 1.0 } else { 0.0 });
        m.set_eq_curve(self.eq_curve());
        m.set_level(self.level);
        m.set_headphone_mix(self.headphone_mix);
        m.set_headphone_level(self.headphone_level);
        m.hardware_phones = self.hardware_phones;
        m.color_params = self.color_params;
        for ch in &mut m.channels {
            for kind in ColorFxKind::ALL {
                for (index, value) in self.color_params[kind.slot()].iter().enumerate() {
                    ch.color.set_param(kind, index, *value);
                }
            }
        }
        m.set_color_fx(self.color_fx);
        m.set_smart_cfx_preset(self.smart_preset);
        m.set_smart_cfx(self.smart_on);
        m.set_release_config(self.release_kind, self.release_beats);
        m.beat_fx.restore_band_sets(self.beat_fx.band_sets());
        m.beat_fx.restore_param_sets(self.beat_fx.param_sets());
        m.set_beat_fx_kind(self.beat_fx.kind());
        m.set_beat_fx_target(self.fx_target);
        m.fx_applied = self.fx_target;
        m.set_beat_fx_beats(self.fx_beats);
        m.set_beat_fx_percent(self.fx_percent);
        m.set_beat_fx_level(self.beat_fx.level());
        m.set_beat_fx_enabled(self.beat_fx.is_enabled(), None);
        m
    }

    pub fn set_beat_fx_kind(&mut self, kind: BeatFxKind) {
        self.beat_fx.set_kind(kind);
        if let Some(beats) = snap_beat_fx_beats_for(kind, self.fx_beats).filter(|b| *b != self.fx_beats) {
            self.set_beat_fx_beats(beats);
        }
    }

    pub fn beat_fx_kind(&self) -> BeatFxKind {
        self.beat_fx.kind()
    }

    pub fn beat_fx(&mut self) -> &mut BeatFx {
        &mut self.beat_fx
    }

    pub fn set_beat_fx_param(&mut self, index: usize, value: f32) {
        self.beat_fx.set_param(index, value);
    }

    pub fn trigger_beat_fx(&mut self, trigger: FxTrigger) {
        self.beat_fx.trigger(trigger);
    }

    pub fn set_fx_transport(&mut self, transport: Option<Transport>) {
        self.beat_fx.set_transport(transport);
    }

    pub fn set_beat_fx_target(&mut self, target: FxTarget) {
        self.fx_target = target;
    }

    pub fn beat_fx_target(&self) -> FxTarget {
        self.fx_target
    }

    pub fn set_beat_fx_beats(&mut self, beats: f64) {
        self.fx_beats = beats;
        self.beat_fx.set_beats(beats as f32);
    }

    pub fn set_beat_fx_percent(&mut self, percent: f64) {
        self.fx_percent = percent;
        self.beat_fx.set_percent(percent as f32);
    }

    pub fn set_beat_fx_level(&mut self, level: f32) {
        self.beat_fx.set_level(level);
    }

    pub fn set_beat_fx_bands(&mut self, bands: FxBands) {
        self.beat_fx.set_bands(bands);
    }

    pub fn set_beat_fx_enabled(&mut self, enabled: bool, beat_position: Option<f64>) {
        let rising = enabled && !self.beat_fx.is_enabled();
        self.beat_fx.set_enabled(enabled);
        if let (true, Some(pos)) = (rising, beat_position) {
            self.beat_fx.align_phase(pos);
        }
    }

    pub fn set_color_fx(&mut self, kind: ColorFxKind) {
        self.color_fx = kind;
        for ch in &mut self.channels {
            ch.set_color_kind(kind);
        }
    }

    /// Sets one sub-parameter of the selected Color FX kind on every channel; out-of-range values clamp, non-finite ones are ignored.
    pub fn set_color_fx_param(&mut self, index: usize, value: f32) {
        let kind = self.color_fx;
        if let Some(v) = color_fx_params(kind).get(index).and_then(|spec| spec.sanitize(value)) {
            self.color_params[kind.slot()][index] = v;
            for ch in &mut self.channels {
                ch.color.set_param(kind, index, v);
            }
        }
    }

    pub fn color_fx_params(&self) -> [f32; COLOR_FX_MAX_PARAMS] {
        self.color_params[self.color_fx.slot()]
    }

    pub fn set_tempo_context(&mut self, fx_bpm: f64, channel_bpm: [f64; DECK_COUNT]) {
        self.fx_bpm = fx_bpm;
        self.beat_fx.set_bpm(fx_bpm as f32);
        for (ch, bpm) in self.channels.iter_mut().zip(channel_bpm) {
            ch.bpm = bpm;
            ch.color.set_bpm(bpm as f32);
            ch.smart.set_bpm(bpm);
            ch.pad.set_bpm(bpm as f32);
        }
    }

    /// Each deck's beat position at the start of the next block, for grid-locked channel effects.
    pub fn set_channel_beats(&mut self, beats: [Option<f64>; DECK_COUNT]) {
        for (ch, beat) in self.channels.iter_mut().zip(beats) {
            ch.smart.set_beat_position(beat);
        }
    }

    pub fn set_pad_voice(&mut self, deck: usize, voice: Option<PadVoice>, beat_position: Option<f64>) {
        if let Some(ch) = self.channels.get_mut(deck) {
            ch.set_pad_voice(voice, beat_position);
        }
    }

    pub fn pad_release(&mut self, deck: usize, kind: ReleaseFxKind, beats: f64) {
        if let Some(ch) = self.channels.get_mut(deck) {
            ch.set_pad_voice(None, None);
            ch.release.trigger(kind, beats, ch.bpm);
        }
    }

    pub fn set_release_config(&mut self, kind: ReleaseFxKind, beats: f64) {
        self.release_kind = kind;
        if let Some(beats) = snap_release_fx_beats(beats) {
            self.release_beats = beats;
        }
    }

    /// Cancels the Beat FX (tail included) and plays the selected Release FX where the Beat FX was aimed.
    pub fn fire_release(&mut self) {
        self.beat_fx.set_enabled(false);
        self.beat_fx.flush();
        let (kind, beats) = (self.release_kind, self.release_beats);
        match self.fx_target {
            FxTarget::A => self.channels[0].release.trigger(kind, beats, self.fx_bpm),
            FxTarget::B => self.channels[1].release.trigger(kind, beats, self.fx_bpm),
            FxTarget::Master => self.release.trigger(kind, beats, self.fx_bpm),
        }
    }

    pub fn set_smart_cfx(&mut self, on: bool) {
        self.smart_on = on;
        for ch in &mut self.channels {
            ch.set_smart(on);
        }
    }

    pub fn set_smart_cfx_preset(&mut self, preset: SmartCfxPreset) {
        self.smart_preset = preset;
        for ch in &mut self.channels {
            ch.smart.set_preset(preset);
            if ch.smart_on {
                ch.filter_value = 0.0;
                ch.smart.set_value(0.0);
                ch.refresh_filter();
            }
        }
    }

    pub fn pad_fx_snapshot(&self, deck: usize) -> PadFxSnapshot {
        self.channels[deck].pad_snapshot()
    }

    pub fn release_fx_snapshot(&self) -> ReleaseFxSnapshot {
        let channel = match self.fx_target {
            FxTarget::A => Some(&self.channels[0].release),
            FxTarget::B => Some(&self.channels[1].release),
            FxTarget::Master => None,
        };
        let running = channel.filter(|r| r.is_running()).unwrap_or(&self.release);
        ReleaseFxSnapshot {
            kind: self.release_kind,
            beats: self.release_beats,
            running: running.running(),
            progress: running.progress().unwrap_or(0.0),
        }
    }

    pub fn smart_cfx_snapshot(&self) -> SmartCfxSnapshot {
        SmartCfxSnapshot { enabled: self.smart_on, preset: self.smart_preset }
    }

    fn settle_fx_target(&mut self) {
        if self.fx_applied == self.fx_target {
            return;
        }
        if !self.fx_handoff {
            self.beat_fx.flush();
            self.fx_handoff = true;
        } else if !self.beat_fx.is_swapping() {
            self.fx_applied = self.fx_target;
            self.fx_handoff = false;
        }
    }

    pub fn beat_fx_snapshot(&self) -> BeatFxSnapshot {
        BeatFxSnapshot {
            kind: self.beat_fx.kind(),
            target: self.fx_target,
            beats: self.fx_beats,
            percent: self.fx_percent,
            level: self.beat_fx.level(),
            enabled: self.beat_fx.is_enabled(),
            bpm: self.fx_bpm,
            bands: self.beat_fx.bands(),
            params: self.beat_fx.params(),
            held: self.beat_fx.held(),
            ascend_progress: self.beat_fx.ascend_progress(),
            ascend_bars: self.beat_fx.ascend_layout().map(|(bars, _)| bars),
            ascend_landing_bar: self.beat_fx.ascend_layout().map(|(_, bar)| bar),
        }
    }

    pub fn color_fx_kind(&self) -> ColorFxKind {
        self.color_fx
    }

    pub fn channel(&mut self, index: usize, param: MixerParam) {
        if let Some(ch) = self.channels.get_mut(index) {
            ch.apply(param);
        }
    }

    pub fn set_crossfader(&mut self, v: f32) {
        let previous = self.crossfader;
        self.crossfader = v.clamp(0.0, 1.0);
        self.xf.set(self.crossfader);
        let crossed = (previous - 0.5) * (self.crossfader - 0.5) <= 0.0;
        if self.xf_pickup && (crossed || (self.crossfader - 0.5).abs() <= XF_PICKUP_WINDOW) {
            self.xf_pickup = false;
            self.refresh_crossfader();
        }
    }

    /// Leaving Off waits for the crossfader to reach the centre, so turning it back on never jumps the mix.
    pub fn set_curve(&mut self, curve: CrossfaderCurve) {
        let was_off = self.curve == CrossfaderCurve::Off;
        self.curve = curve;
        if curve != CrossfaderCurve::Off {
            self.xf_curve = curve;
            if was_off {
                self.xf_pickup = (self.crossfader - 0.5).abs() > XF_PICKUP_WINDOW;
            }
        }
        self.refresh_crossfader();
    }

    pub fn curve(&self) -> CrossfaderCurve {
        self.curve
    }

    pub fn crossfader_active(&self) -> bool {
        self.curve != CrossfaderCurve::Off && !self.xf_pickup
    }

    fn refresh_crossfader(&mut self) {
        self.xf_engage.set(if self.crossfader_active() { 1.0 } else { 0.0 });
    }

    pub fn set_level(&mut self, v: f32) {
        self.level = v.clamp(0.0, 1.0);
        self.master.set(master_gain(self.level));
    }

    pub fn set_headphone_mix(&mut self, v: f32) {
        self.headphone_mix = v.clamp(0.0, 1.0);
        self.hp_mix.set(self.headphone_mix);
    }

    pub fn set_headphone_level(&mut self, v: f32) {
        self.headphone_level = v.clamp(0.0, 1.0);
        self.hp_level.set(master_gain(self.headphone_level));
    }

    /// Mixes deck signals (consumed in place) into master, cue and headphone buses.
    pub fn process(&mut self, decks: [(&mut [f32], &mut [f32]); DECK_COUNT], n: usize) {
        self.cue_l[..n].fill(0.0);
        self.cue_r[..n].fill(0.0);
        self.master_l[..n].fill(0.0);
        self.master_r[..n].fill(0.0);

        let applied = self.fx_applied;
        let curve = self.xf_curve;
        let mut xf_gains = [[0f32; 2]; MAX_BLOCK];
        for g in xf_gains.iter_mut().take(n) {
            let engage = self.xf_engage.next();
            let (a, b) = crossfader_gains(self.xf.next(), curve);
            *g = [1.0 + (a - 1.0) * engage, 1.0 + (b - 1.0) * engage];
        }

        for (idx, (l, r)) in decks.into_iter().enumerate() {
            let ch = &mut self.channels[idx];
            ch.process_prefader(&mut l[..n], &mut r[..n]);
            if applied == [FxTarget::A, FxTarget::B][idx] {
                self.beat_fx.process(&mut l[..n], &mut r[..n]);
            }
            ch.release.process(&mut l[..n], &mut r[..n]);
            ch.feed_meter(&l[..n], &r[..n]);
            if ch.pfl {
                for i in 0..n {
                    self.cue_l[i] += l[i];
                    self.cue_r[i] += r[i];
                }
            }
            for i in 0..n {
                let g = ch.fader_gain.next() * xf_gains[i][idx];
                self.master_l[i] += l[i] * g;
                self.master_r[i] += r[i] * g;
            }
        }

        if applied == FxTarget::Master {
            self.beat_fx.process(&mut self.master_l[..n], &mut self.master_r[..n]);
        }
        self.release.process(&mut self.master_l[..n], &mut self.master_r[..n]);
        self.settle_fx_target();

        let mut clipped = false;
        let mut limited = false;
        for i in 0..n {
            let (rl, rr) = self.rec_limiter.process(self.master_l[i], self.master_r[i]);
            self.rec_l[i] = backstop(rl);
            self.rec_r[i] = backstop(rr);
            let mg = self.master.next();
            let ml = self.master_l[i] * mg;
            let mr = self.master_r[i] * mg;
            clipped |= ml.abs() > 1.0 || mr.abs() > 1.0;
            let (ol, or) = self.master_limiter.process(ml, mr);
            limited |= self.master_limiter.gain() < LIMITING_GAIN;
            self.master_l[i] = backstop(ol);
            self.master_r[i] = backstop(or);
            self.master_meter[0].feed(ml);
            self.master_meter[1].feed(mr);
            self.cue_meter[0].feed(self.cue_l[i]);
            self.cue_meter[1].feed(self.cue_r[i]);

            let mix = self.hp_mix.next();
            let hp = self.hp_level.next();
            let (pl, pr) = if self.hardware_phones {
                self.phones_limiter.process(self.cue_l[i], self.cue_r[i])
            } else {
                let cue_part = (2.0 * (1.0 - mix)).min(1.0);
                let master_part = (2.0 * mix).min(1.0);
                self.phones_limiter.process((self.cue_l[i] * cue_part + ml * master_part) * hp, (self.cue_r[i] * cue_part + mr * master_part) * hp)
            };
            self.phones_l[i] = backstop(pl);
            self.phones_r[i] = backstop(pr);
        }

        let sr = self.sample_rate;
        for m in self.master_meter.iter_mut().chain(self.cue_meter.iter_mut()) {
            m.end_block(n, sr);
        }
        for ch in &mut self.channels {
            for m in &mut ch.meter {
                m.end_block(n, sr);
            }
        }
        if clipped {
            self.clip_hold = CLIP_HOLD_SECS;
        } else {
            self.clip_hold = (self.clip_hold - n as f32 / sr).max(0.0);
        }
        self.limiting = limited;
    }

    pub fn set_eq_curve(&mut self, curve: EqCurve) {
        for ch in &mut self.channels {
            ch.channel_eq.set_curve(curve);
        }
    }

    pub fn eq_curve(&self) -> EqCurve {
        self.channels[0].channel_eq.curve()
    }

    pub fn channel_snapshot(&self, i: usize) -> ChannelSnapshot {
        self.channels[i].snapshot()
    }

    pub fn master_snapshot(&self) -> MasterSnapshot {
        MasterSnapshot {
            crossfader: self.crossfader,
            curve: self.curve,
            crossfader_active: self.crossfader_active(),
            eq_curve: self.eq_curve(),
            level: self.level,
            headphone_mix: self.headphone_mix,
            headphone_level: self.headphone_level,
            meter_l: self.master_meter[0].level(),
            meter_r: self.master_meter[1].level(),
            cue_meter_l: self.cue_meter[0].level(),
            cue_meter_r: self.cue_meter[1].level(),
            clipping: self.clip_hold > 0.0,
            limiting: self.limiting,
            hardware_phones: self.hardware_phones,
        }
    }
}
