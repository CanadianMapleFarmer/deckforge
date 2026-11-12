use std::sync::Arc;

use serde::{Deserialize, Serialize};

mod fx_params;
mod pad_fx;

pub use fx_params::{beat_fx_knob_params, beat_fx_params, color_fx_params, BeatFxParamSpec, BEAT_FX_MAX_PARAMS, COLOR_FX_MAX_PARAMS};
pub use pad_fx::{
    snap_release_fx_beats, PadFxAction, PadFxSlot, PadVoice, ReleaseFxKind, SmartCfxPreset, PAD_FX1_BANK, PAD_FX1_SHIFT_BANK, PAD_FX2_BEATS,
    PAD_FX2_FAVOURITES, PAD_FX2_PERCENTS, RELEASE_FX_BEATS, RELEASE_FX_DEFAULT_BEATS,
};

pub const DECK_COUNT: usize = 2;
pub const HOT_CUE_SLOTS: usize = 8;

/// Decoded audio held fully in memory: interleaved stereo f32 at the file's native rate.
#[derive(Debug)]
pub struct TrackAudio {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

impl TrackAudio {
    pub fn new(sample_rate: u32, samples: Vec<f32>) -> Self {
        debug_assert!(samples.len().is_multiple_of(2));
        Self { sample_rate, samples }
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }

    #[inline]
    pub fn frame(&self, index: usize) -> (f32, f32) {
        let i = index * 2;
        (self.samples[i], self.samples[i + 1])
    }
}

pub type SharedTrack = Arc<TrackAudio>;

/// Constant-tempo beatgrid expressed in track seconds, independent of sample rate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Beatgrid {
    pub bpm: f64,
    pub first_beat_secs: f64,
}

impl Beatgrid {
    pub fn beat_secs(&self) -> f64 {
        60.0 / self.bpm
    }

    /// Fractional beat index at a track time (0.0 at the first beat).
    pub fn beat_at(&self, secs: f64) -> f64 {
        (secs - self.first_beat_secs) / self.beat_secs()
    }

    pub fn secs_of_beat(&self, beat: f64) -> f64 {
        self.first_beat_secs + beat * self.beat_secs()
    }

    pub fn nearest_beat_secs(&self, secs: f64) -> f64 {
        self.secs_of_beat(self.beat_at(secs).round())
    }

    /// Beat phase in [0, 1).
    pub fn phase_at(&self, secs: f64) -> f64 {
        self.beat_at(secs).rem_euclid(1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotCue {
    pub position_secs: f64,
    pub color: u32,
}

pub const HOT_CUE_PALETTE: [u32; HOT_CUE_SLOTS] = [
    0xFF3B5C, 0xFF8A00, 0xFFD21F, 0x2BE38A, 0x1FC8FF, 0x4E7CFF, 0xA25BFF, 0xFF4FD8,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Band {
    Low,
    Mid,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrossfaderCurve {
    Linear,
    #[default]
    EqualPower,
    Sharp,
    Off,
}

/// Channel EQ knob behaviour, after the DJM-900NXS2 EQ CURVE switch: `Eq` cuts to -26 dB, `Isolator` kills fully.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EqCurve {
    #[default]
    Eq,
    Isolator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TempoRange {
    Six,
    #[default]
    Ten,
    Sixteen,
    Wide,
}

impl TempoRange {
    pub fn fraction(self) -> f64 {
        match self {
            TempoRange::Six => 0.06,
            TempoRange::Ten => 0.10,
            TempoRange::Sixteen => 0.16,
            TempoRange::Wide => 1.0,
        }
    }

    pub fn next(self) -> Self {
        match self {
            TempoRange::Six => TempoRange::Ten,
            TempoRange::Ten => TempoRange::Sixteen,
            TempoRange::Sixteen => TempoRange::Wide,
            TempoRange::Wide => TempoRange::Six,
        }
    }
}

/// Allowed beat divisions for the Beat FX unit.
pub const BEAT_FX_BEATS: [f64; 9] = [0.125, 0.25, 0.5, 0.75, 1.0, 2.0, 4.0, 8.0, 16.0];

pub const BACKWASH_BEATS: [f64; 3] = [1.0, 2.0, 4.0];
pub const PULSE_VERB_BEATS: [f64; 7] = [0.125, 0.25, 0.5, 0.75, 1.0, 2.0, 4.0];

/// Snaps a requested beat division to the nearest allowed value; `None` for non-positive or non-finite input.
pub fn snap_beat_fx_beats(beats: f64) -> Option<f64> {
    snap_beat_fx_beats_in(&BEAT_FX_BEATS, beats)
}

/// Snaps to the nearest division the given kind offers.
pub fn snap_beat_fx_beats_for(kind: BeatFxKind, beats: f64) -> Option<f64> {
    snap_beat_fx_beats_in(kind.beat_options(), beats)
}

fn snap_beat_fx_beats_in(options: &[f64], beats: f64) -> Option<f64> {
    if !beats.is_finite() || beats <= 0.0 {
        return None;
    }
    options.iter().copied().min_by(|a, b| (a / beats).ln().abs().total_cmp(&(b / beats).ln().abs()))
}

/// Preset percentages the BEAT buttons step through for percent-driven effects such as Reverb.
pub const BEAT_FX_PERCENT_STEPS: [f64; 11] = [1.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0];
pub const BEAT_FX_PERCENT_DEFAULT: f64 = 50.0;

/// Clamps a requested percent to 1..=100; `None` for non-finite input.
pub fn clamp_beat_fx_percent(percent: f64) -> Option<f64> {
    percent.is_finite().then(|| percent.clamp(1.0, 100.0))
}

/// Moves to the next preset above (`up`) or below the current percent, clamping at 1 and 100.
pub fn step_beat_fx_percent(current: f64, up: bool) -> f64 {
    let current = clamp_beat_fx_percent(current).unwrap_or(BEAT_FX_PERCENT_DEFAULT);
    let steps = BEAT_FX_PERCENT_STEPS;
    if up {
        steps.iter().copied().find(|s| *s > current + 1e-9).unwrap_or(100.0)
    } else {
        steps.iter().rev().copied().find(|s| *s < current - 1e-9).unwrap_or(1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BeatFxKind {
    Delay,
    #[default]
    Echo,
    UpEcho,
    DownEcho,
    PingPong,
    Spiral,
    Reverb,
    Flanger,
    Phaser,
    Roll,
    Trans,
    FilterSweep,
    PulseVerb,
    DubTape,
    Halo,
    Ascend,
    Backwash,
}

impl BeatFxKind {
    pub const ALL: [BeatFxKind; 17] = [
        BeatFxKind::Delay,
        BeatFxKind::Echo,
        BeatFxKind::UpEcho,
        BeatFxKind::DownEcho,
        BeatFxKind::PingPong,
        BeatFxKind::Spiral,
        BeatFxKind::Reverb,
        BeatFxKind::Flanger,
        BeatFxKind::Phaser,
        BeatFxKind::Roll,
        BeatFxKind::Trans,
        BeatFxKind::FilterSweep,
        BeatFxKind::PulseVerb,
        BeatFxKind::DubTape,
        BeatFxKind::Halo,
        BeatFxKind::Ascend,
        BeatFxKind::Backwash,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|k| *k == self).unwrap_or(0)
    }

    /// Kinds whose BEAT buttons set an amount in percent instead of a beat division.
    pub fn uses_percent(self) -> bool {
        matches!(self, BeatFxKind::Reverb | BeatFxKind::Halo)
    }

    /// Kinds whose BEAT buttons step a sub-parameter (by index) instead of beats or percent.
    pub fn primary_param(self) -> Option<usize> {
        matches!(self, BeatFxKind::Ascend).then_some(0)
    }

    /// Beat divisions the BEAT buttons step through for this kind.
    pub fn beat_options(self) -> &'static [f64] {
        match self {
            BeatFxKind::Backwash => &BACKWASH_BEATS,
            BeatFxKind::PulseVerb => &PULSE_VERB_BEATS,
            _ => &BEAT_FX_BEATS,
        }
    }

    /// Kinds with a momentary throw send (and therefore a THROW button).
    pub fn has_throw(self) -> bool {
        matches!(self, BeatFxKind::DubTape | BeatFxKind::Backwash)
    }

    /// Kinds with a FREEZE hold.
    pub fn has_freeze(self) -> bool {
        (self.is_echo_family() && self != BeatFxKind::Spiral) || matches!(self, BeatFxKind::DubTape | BeatFxKind::Halo)
    }

    /// Kinds that run a timed build and end with a RELEASE.
    pub fn has_release(self) -> bool {
        matches!(self, BeatFxKind::Ascend)
    }

    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub fn is_echo_family(self) -> bool {
        matches!(
            self,
            BeatFxKind::Delay | BeatFxKind::Echo | BeatFxKind::UpEcho | BeatFxKind::DownEcho | BeatFxKind::PingPong | BeatFxKind::Spiral
        )
    }

    /// Bands the effect applies to until the user changes them; echoes and reverb leave the lows dry.
    pub fn default_bands(self) -> FxBands {
        let low = !(self.is_echo_family()
            || matches!(self, BeatFxKind::Reverb | BeatFxKind::PulseVerb | BeatFxKind::DubTape | BeatFxKind::Halo | BeatFxKind::Backwash));
        FxBands { low, mid: true, high: true }
    }

    pub fn slot(self) -> usize {
        self.index()
    }
}

/// Momentary actions for the Beat FX kinds that have a hold, send or release gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FxTrigger {
    ThrowOn,
    ThrowOff,
    FreezeOn,
    FreezeOff,
    Release,
}

/// FX FREQUENCY selection: the bands a Beat FX acts on; unselected bands pass dry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FxBands {
    pub low: bool,
    pub mid: bool,
    pub high: bool,
}

impl FxBands {
    pub const ALL: FxBands = FxBands { low: true, mid: true, high: true };

    pub fn is_all(self) -> bool {
        self == Self::ALL
    }

    pub fn gains(self) -> [f32; 3] {
        [self.low, self.mid, self.high].map(|on| if on { 1.0 } else { 0.0 })
    }
}

impl Default for FxBands {
    fn default() -> Self {
        Self::ALL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FxTarget {
    A,
    B,
    #[default]
    Master,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorFxKind {
    #[default]
    Filter,
    Space,
    DubEcho,
    Crush,
    Noise,
}

impl ColorFxKind {
    pub const ALL: [ColorFxKind; 5] = [ColorFxKind::Filter, ColorFxKind::Space, ColorFxKind::DubEcho, ColorFxKind::Crush, ColorFxKind::Noise];

    pub fn slot(self) -> usize {
        Self::ALL.iter().position(|k| *k == self).unwrap_or(0)
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(1e-9).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_fx_kinds_cycle() {
        assert_eq!(BeatFxKind::Delay.prev(), BeatFxKind::Backwash);
        assert_eq!(BeatFxKind::Backwash.next(), BeatFxKind::Delay);
        assert_eq!(BeatFxKind::FilterSweep.next(), BeatFxKind::PulseVerb);
        assert_eq!(BeatFxKind::Ascend.next(), BeatFxKind::Backwash);
        assert_eq!(BeatFxKind::Echo.next(), BeatFxKind::UpEcho);
        assert_eq!(BeatFxKind::DownEcho.next(), BeatFxKind::PingPong);
        assert_eq!(BeatFxKind::PingPong.next(), BeatFxKind::Spiral);
    }

    #[test]
    fn default_bands_leave_the_lows_dry_for_echoes_reverbs_and_tension_washes() {
        for kind in BeatFxKind::ALL {
            let want_low = !(kind.is_echo_family()
                || matches!(kind, BeatFxKind::Reverb | BeatFxKind::PulseVerb | BeatFxKind::DubTape | BeatFxKind::Halo | BeatFxKind::Backwash));
            assert_eq!(kind.default_bands(), FxBands { low: want_low, mid: true, high: true }, "{kind:?}");
        }
    }

    #[test]
    fn beat_fx_percent_steps_walk_the_presets_and_clamp() {
        assert_eq!(step_beat_fx_percent(1.0, true), 10.0);
        assert_eq!(step_beat_fx_percent(10.0, false), 1.0);
        assert_eq!(step_beat_fx_percent(37.0, true), 40.0);
        assert_eq!(step_beat_fx_percent(37.0, false), 30.0);
        assert_eq!(step_beat_fx_percent(100.0, true), 100.0);
        assert_eq!(step_beat_fx_percent(1.0, false), 1.0);
        assert_eq!(step_beat_fx_percent(f64::NAN, true), 60.0);
        assert_eq!(clamp_beat_fx_percent(250.0), Some(100.0));
        assert_eq!(clamp_beat_fx_percent(-4.0), Some(1.0));
        assert_eq!(clamp_beat_fx_percent(f64::INFINITY), None);
        assert!(BeatFxKind::Reverb.uses_percent() && BeatFxKind::Halo.uses_percent() && !BeatFxKind::Echo.uses_percent());
    }

    #[test]
    fn kinds_snap_to_their_own_beat_options_and_ascend_steps_a_parameter() {
        assert_eq!(snap_beat_fx_beats_for(BeatFxKind::Backwash, 0.5), Some(1.0));
        assert_eq!(snap_beat_fx_beats_for(BeatFxKind::Backwash, 2.5), Some(2.0));
        assert_eq!(snap_beat_fx_beats_for(BeatFxKind::Backwash, 16.0), Some(4.0));
        assert_eq!(snap_beat_fx_beats_for(BeatFxKind::PulseVerb, 16.0), Some(4.0));
        assert_eq!(snap_beat_fx_beats_for(BeatFxKind::Echo, 16.0), Some(16.0));
        assert_eq!(BeatFxKind::Ascend.primary_param(), Some(0));
        assert_eq!(BeatFxKind::Echo.primary_param(), None);
        assert!(BeatFxKind::DubTape.has_throw() && BeatFxKind::DubTape.has_freeze() && BeatFxKind::Halo.has_freeze() && BeatFxKind::Ascend.has_release());
        assert!(BeatFxKind::Echo.has_freeze() && BeatFxKind::PingPong.has_freeze() && !BeatFxKind::Reverb.has_freeze() && !BeatFxKind::Spiral.has_freeze());
    }

    #[test]
    fn beat_fx_beats_snap_to_allowed_values() {
        assert_eq!(snap_beat_fx_beats(1.0), Some(1.0));
        assert_eq!(snap_beat_fx_beats(0.7), Some(0.75));
        assert_eq!(snap_beat_fx_beats(100.0), Some(16.0));
        assert_eq!(snap_beat_fx_beats(0.01), Some(0.125));
        assert_eq!(snap_beat_fx_beats(f64::NAN), None);
        assert_eq!(snap_beat_fx_beats(-1.0), None);
    }

    #[test]
    fn beatgrid_phase_and_nearest() {
        let g = Beatgrid { bpm: 120.0, first_beat_secs: 0.25 };
        assert!((g.beat_secs() - 0.5).abs() < 1e-12);
        assert!((g.phase_at(0.5) - 0.5).abs() < 1e-12);
        assert!((g.nearest_beat_secs(0.74) - 0.75).abs() < 1e-12);
        assert!((g.phase_at(0.0) - 0.5).abs() < 1e-12);
    }
}
