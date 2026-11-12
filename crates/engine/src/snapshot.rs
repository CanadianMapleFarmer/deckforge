use deckforge_common::{
    beat_fx_params, BeatFxKind, ColorFxKind, CrossfaderCurve, EqCurve, FxBands, FxTarget, HotCue, PadVoice, ReleaseFxKind, SmartCfxPreset, TempoRange,
    BEAT_FX_MAX_PARAMS, BEAT_FX_PERCENT_DEFAULT, COLOR_FX_MAX_PARAMS, DECK_COUNT, HOT_CUE_SLOTS, RELEASE_FX_DEFAULT_BEATS,
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckSnapshot {
    pub loaded: bool,
    pub track_token: u64,
    pub playing: bool,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub tempo_ratio: f64,
    /// Track seconds per second actually playing now: tempo with jog nudge and sync correction, scratch speed while
    /// the jog top is held (negative backwards), 0 when stopped.
    pub play_rate: f64,
    pub fader_tempo_ratio: f64,
    pub tempo_range: TempoRange,
    pub key_lock: bool,
    pub vinyl_mode: bool,
    pub has_grid: bool,
    pub grid_bpm: f64,
    pub first_beat_secs: f64,
    pub bpm: f64,
    pub beat_phase: f64,
    pub cue_secs: f64,
    pub hot_cues: [Option<HotCue>; HOT_CUE_SLOTS],
    pub loop_in_secs: Option<f64>,
    pub loop_out_secs: Option<f64>,
    pub loop_active: bool,
    pub sync_enabled: bool,
    pub is_master: bool,
    pub jog_touched: bool,
    pub previewing: bool,
    pub at_end: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSnapshot {
    pub trim: f32,
    pub eq_high: f32,
    pub eq_mid: f32,
    pub eq_low: f32,
    pub filter: f32,
    pub fader: f32,
    pub pfl: bool,
    pub meter_l: f32,
    pub meter_r: f32,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterSnapshot {
    pub crossfader: f32,
    pub curve: CrossfaderCurve,
    pub crossfader_active: bool,
    pub eq_curve: EqCurve,
    pub level: f32,
    pub headphone_mix: f32,
    pub headphone_level: f32,
    pub meter_l: f32,
    pub meter_r: f32,
    pub cue_meter_l: f32,
    pub cue_meter_r: f32,
    pub clipping: bool,
    pub limiting: bool,
    pub hardware_phones: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeatFxSnapshot {
    pub kind: BeatFxKind,
    pub target: FxTarget,
    pub beats: f64,
    pub percent: f64,
    pub level: f32,
    pub enabled: bool,
    pub bpm: f64,
    pub bands: FxBands,
    pub params: [f32; BEAT_FX_MAX_PARAMS],
    pub held: bool,
    pub ascend_progress: Option<f32>,
    pub ascend_bars: Option<f64>,
    pub ascend_landing_bar: Option<u32>,
}

impl Default for BeatFxSnapshot {
    fn default() -> Self {
        let kind = BeatFxKind::default();
        let mut params = [0.0; BEAT_FX_MAX_PARAMS];
        for (v, spec) in params.iter_mut().zip(beat_fx_params(kind)) {
            *v = spec.default;
        }
        Self {
            kind,
            target: FxTarget::default(),
            beats: 1.0,
            percent: BEAT_FX_PERCENT_DEFAULT,
            level: 0.5,
            enabled: false,
            bpm: 120.0,
            bands: kind.default_bands(),
            params,
            held: false,
            ascend_progress: None,
            ascend_bars: None,
            ascend_landing_bar: None,
        }
    }
}

/// A deck's Pad FX: the held voice, whether it still sounds (tail included) and any one-shot transition.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PadFxSnapshot {
    pub voice: Option<PadVoice>,
    pub sounding: bool,
    pub release: Option<ReleaseFxKind>,
    pub release_progress: f32,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseFxSnapshot {
    pub kind: ReleaseFxKind,
    pub beats: f64,
    pub running: Option<ReleaseFxKind>,
    pub progress: f32,
}

impl Default for ReleaseFxSnapshot {
    fn default() -> Self {
        Self { kind: ReleaseFxKind::default(), beats: RELEASE_FX_DEFAULT_BEATS, running: None, progress: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartCfxSnapshot {
    pub enabled: bool,
    pub preset: SmartCfxPreset,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    pub decks: [DeckSnapshot; DECK_COUNT],
    pub channels: [ChannelSnapshot; DECK_COUNT],
    pub master: MasterSnapshot,
    pub quantize: bool,
    pub master_deck: Option<u8>,
    pub sample_rate: u32,
    pub frames_processed: u64,
    /// Seconds of audio rendered since the engine started, summed across sample-rate changes.
    pub clock_secs: f64,
    /// Seconds from the end of the last rendered block until it is heard: device latency plus that block.
    pub output_latency_secs: f64,
    pub callback_load: f32,
    pub cue_available: bool,
    pub beat_fx: BeatFxSnapshot,
    pub color_fx: ColorFxKind,
    pub color_fx_params: [f32; COLOR_FX_MAX_PARAMS],
    pub pad_fx: [PadFxSnapshot; DECK_COUNT],
    pub release_fx: ReleaseFxSnapshot,
    pub smart_cfx: SmartCfxSnapshot,
}
