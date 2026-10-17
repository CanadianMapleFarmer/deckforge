use deckforge_common::{
    Band, BeatFxKind, Beatgrid, ColorFxKind, CrossfaderCurve, EqCurve, FxBands, FxTarget, FxTrigger, HotCue, PadVoice, ReleaseFxKind, SharedTrack,
    SmartCfxPreset, TempoRange, HOT_CUE_SLOTS,
};

#[derive(Debug, Clone)]
pub struct LoadRequest {
    pub track: SharedTrack,
    pub token: u64,
    pub grid: Option<Beatgrid>,
    pub cue_secs: Option<f64>,
    pub hot_cues: [Option<HotCue>; HOT_CUE_SLOTS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JogMode {
    /// Platter top touched in vinyl mode: wheel motion drives playback directly.
    Scratch,
    /// Wheel edge (or top with vinyl mode off): temporary tempo nudge.
    Bend,
    /// Fine position search, used while paused.
    Search,
    /// SHIFT + jog: fast search.
    FastSearch,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MixerParam {
    Trim(f32),
    Eq(Band, f32),
    Filter(f32),
    Fader(f32),
    Pfl(bool),
}

#[derive(Debug, Clone)]
pub enum Command {
    Load { deck: usize, request: Box<LoadRequest> },
    Eject { deck: usize },
    Play { deck: usize },
    Pause { deck: usize },
    TogglePlay { deck: usize },
    CueDown { deck: usize },
    CueUp { deck: usize },
    SeekSecs { deck: usize, secs: f64 },
    SetTempo { deck: usize, ratio: f64 },
    /// Physical tempo fader position, -1.0 (slowest) to 1.0 (fastest), scaled by the deck's range.
    SetPitchFader { deck: usize, position: f64 },
    SetTempoRange { deck: usize, range: TempoRange },
    SetKeyLock { deck: usize, enabled: bool },
    SetVinylMode { deck: usize, enabled: bool },
    JogTouch { deck: usize, touched: bool },
    Jog { deck: usize, ticks: i32, mode: JogMode },
    HotCue { deck: usize, slot: usize, pressed: bool },
    HotCueDelete { deck: usize, slot: usize },
    HotCueColor { deck: usize, slot: usize, color: u32 },
    LoopIn { deck: usize },
    LoopOut { deck: usize },
    ReloopExit { deck: usize },
    BeatLoop { deck: usize, beats: f64 },
    LoopScale { deck: usize, factor: f64 },
    BeatJump { deck: usize, beats: f64 },
    SetBeatgrid { deck: usize, grid: Option<Beatgrid> },
    Sync { deck: usize, enabled: bool },
    SetMasterDeck { deck: usize },
    Mixer { channel: usize, param: MixerParam },
    Crossfader(f32),
    CrossfaderCurve(CrossfaderCurve),
    EqCurve(EqCurve),
    MasterLevel(f32),
    HeadphoneMix(f32),
    HeadphoneLevel(f32),
    Quantize(bool),
    HardwarePhones(bool),
    BeatFxKind(BeatFxKind),
    BeatFxTarget(FxTarget),
    BeatFxBeats(f64),
    BeatFxPercent(f64),
    BeatFxLevel(f32),
    BeatFxEnabled(bool),
    BeatFxBands(FxBands),
    BeatFxParam { index: usize, value: f32 },
    BeatFxTrigger(FxTrigger),
    /// Sorted phrase start times (track seconds) that anchor the deck's bar lines; allocated off the audio thread.
    SetPhraseStarts { deck: usize, starts: Box<[f64]> },
    ColorFx(ColorFxKind),
    ColorFxParam { index: usize, value: f32 },
    /// Starts or retunes (`Some`) or lets go of (`None`) the deck's held Pad FX voice; its tail rings out.
    PadFx { deck: usize, voice: Option<PadVoice> },
    /// One-shot transition on the deck's channel; ends the deck's Pad FX voice.
    PadFxRelease { deck: usize, kind: ReleaseFxKind, beats: f64 },
    /// SHIFT + FX ON/OFF: cancels the Beat FX and plays the selected Release FX on the Beat FX target.
    ReleaseFx,
    ReleaseFxConfig { kind: ReleaseFxKind, beats: f64 },
    SmartCfx(bool),
    SmartCfxPreset(SmartCfxPreset),
}

impl Command {
    /// The deck whose playback state this command changes, if any.
    pub fn deck(&self) -> Option<usize> {
        use Command::*;
        match self {
            Load { deck, .. } | Eject { deck } | Play { deck } | Pause { deck } | TogglePlay { deck } | CueDown { deck } | CueUp { deck }
            | SeekSecs { deck, .. } | SetTempo { deck, .. } | SetPitchFader { deck, .. } | SetTempoRange { deck, .. }
            | SetKeyLock { deck, .. } | SetVinylMode { deck, .. } | JogTouch { deck, .. } | Jog { deck, .. } | HotCue { deck, .. }
            | HotCueDelete { deck, .. } | HotCueColor { deck, .. } | LoopIn { deck } | LoopOut { deck } | ReloopExit { deck }
            | BeatLoop { deck, .. } | LoopScale { deck, .. } | BeatJump { deck, .. } | SetBeatgrid { deck, .. } | Sync { deck, .. }
            | SetPhraseStarts { deck, .. } => Some(*deck),
            _ => None,
        }
    }
}
