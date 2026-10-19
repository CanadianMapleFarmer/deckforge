// Mirrors the Rust serde types exposed by src-tauri. Keep in sync with docs/architecture/ipc-contract.md.

export type DeckIndex = 0 | 1;
export type TempoRange = 'Six' | 'Ten' | 'Sixteen' | 'Wide';
export type CrossfaderCurve = 'linear' | 'equal-power' | 'sharp';
export type EqCurve = 'eq' | 'isolator';
export type BandName = 'Low' | 'Mid' | 'High';

export interface HotCue {
  positionSecs: number;
  color: number; // 0xRRGGBB
}

export interface DeckSnapshot {
  loaded: boolean;
  trackToken: number; // == library track id of the loaded track (0 when empty)
  playing: boolean;
  positionSecs: number;
  durationSecs: number;
  tempoRatio: number; // effective (includes sync)
  playRate: number; // track seconds per second right now: tempo with nudge and sync correction, scratch speed while the jog top is held, 0 when stopped
  faderTempoRatio: number;
  tempoRange: TempoRange;
  keyLock: boolean;
  vinylMode: boolean;
  hasGrid: boolean;
  gridBpm: number;
  firstBeatSecs: number;
  bpm: number; // effective BPM (grid bpm * tempo)
  beatPhase: number; // 0..1
  cueSecs: number;
  hotCues: (HotCue | null)[]; // length 8
  loopInSecs: number | null;
  loopOutSecs: number | null;
  loopActive: boolean;
  syncEnabled: boolean;
  isMaster: boolean;
  jogTouched: boolean;
  previewing: boolean;
  atEnd: boolean;
}

export interface ChannelSnapshot {
  trim: number; // 0..1, 0.5 = 0 dB
  eqHigh: number; // 0..1, 0.5 = flat, 0 = kill
  eqMid: number;
  eqLow: number;
  filter: number; // -1..1, 0 = off, <0 low-pass, >0 high-pass
  fader: number; // 0..1
  pfl: boolean; // headphone cue
  meterL: number; // linear peak 0..~1.4
  meterR: number;
}

export interface MasterSnapshot {
  crossfader: number; // 0 = deck A, 1 = deck B
  curve: CrossfaderCurve;
  eqCurve: EqCurve;
  level: number; // 0..1, 0.8 = unity
  headphoneMix: number; // 0 = cue, 1 = master
  headphoneLevel: number;
  meterL: number;
  meterR: number;
  cueMeterL: number;
  cueMeterR: number;
  clipping: boolean;
  limiting: boolean;
  hardwarePhones: boolean; // headphone mix/level handled by controller hardware
}

export interface EngineSnapshot {
  decks: [DeckSnapshot, DeckSnapshot];
  channels: [ChannelSnapshot, ChannelSnapshot];
  master: MasterSnapshot;
  quantize: boolean;
  masterDeck: number | null;
  sampleRate: number;
  framesProcessed: number;
  clockSecs: number; // seconds rendered since the engine started, across sample-rate changes
  outputLatencySecs: number; // from the end of the last rendered block until it is heard
  callbackLoad: number; // 0..1 fraction of audio callback budget used (peak, decaying)
  cueAvailable: boolean; // false when output device has no separate headphone pair
  beatFx: BeatFxSnapshot;
  colorFx: ColorFxKind; // applied by each channel's `filter` (CFX) knob
  colorFxParams: number[]; // sub-parameters of the selected `colorFx`; only the first `COLOR_FX_PARAMS[colorFx].length` entries are meaningful
  padFx: [PadFxSnapshot, PadFxSnapshot]; // per-deck Pad FX unit
  releaseFx: ReleaseFxSnapshot;
  smartCfx: SmartCfxSnapshot;
}

export type ReleaseFxKind = 'echoOut' | 'backspin' | 'vinylBrake' | 'rollOut';
/** Types offered for SHIFT + FX ON/OFF (rekordbox's Release FX); `rollOut` is pad-only. */
export const RELEASE_FX_KINDS = ['echoOut', 'backspin', 'vinylBrake'] as const;
export const RELEASE_FX_BEATS = [0.5, 1, 2, 4, 8] as const;
export type SmartCfxPreset = 'echoWash' | 'riser' | 'space' | 'crushChop';
export const SMART_CFX_PRESETS = ['echoWash', 'riser', 'space', 'crushChop'] as const;

/** A held Pad FX effect: a Beat FX kind with its own settings, running on the pad's deck. */
export interface PadVoice {
  kind: BeatFxKind;
  beats: number;
  percent: number;
  level: number; // 0..1
  param0: number | null; // first sub-parameter of the kind, null = its default
}

export interface PadFxSnapshot {
  voice: PadVoice | null; // held voice; null when no pad is held
  sounding: boolean; // voice or its tail still audible
  release: ReleaseFxKind | null; // one-shot transition running on this deck
  releaseProgress: number; // 0..1 while `release` runs
}

export interface ReleaseFxSnapshot {
  kind: ReleaseFxKind; // fired by SHIFT + FX ON/OFF
  beats: number; // one of RELEASE_FX_BEATS
  running: ReleaseFxKind | null; // on the Beat FX target
  progress: number; // 0..1 while running
}

export interface SmartCfxSnapshot {
  enabled: boolean; // CFX knobs drive the preset macro instead of Color FX
  preset: SmartCfxPreset;
}

export type PadFxAction =
  | { type: 'momentary'; voice: PadVoice }
  | { type: 'oneShot'; release: ReleaseFxKind; beats: number };

export interface PadFxSlot {
  label: string;
  action: PadFxAction;
}

const hold = (label: string, kind: BeatFxKind, beats: number, percent: number, level: number, param0: number | null): PadFxSlot => ({
  label,
  action: { type: 'momentary', voice: { kind, beats, percent, level, param0 } },
});
const shot = (label: string, release: ReleaseFxKind, beats: number): PadFxSlot => ({ label, action: { type: 'oneShot', release, beats } });

/** Mirrors `PAD_FX1_BANK` in the Rust common crate; checked by the contract test. */
export const PAD_FX1_BANK: PadFxSlot[] = [
  hold('Echo', 'echo', 0.5, 50, 0.7, 55),
  hold('Ping Pong', 'pingPong', 0.25, 50, 0.7, 50),
  hold('Reverb', 'reverb', 1, 60, 0.6, null),
  hold('Roll', 'roll', 0.5, 50, 1, null),
  shot('Echo Out', 'echoOut', 2),
  shot('Backspin', 'backspin', 1),
  shot('Vinyl Brake', 'vinylBrake', 1),
  shot('Roll Out', 'rollOut', 2),
];

/** Mirrors `PAD_FX1_SHIFT_BANK`: PAD FX 1 with SHIFT held. */
export const PAD_FX1_SHIFT_BANK: PadFxSlot[] = [
  hold('Flanger', 'flanger', 4, 50, 0.8, null),
  hold('Phaser', 'phaser', 4, 50, 0.8, null),
  hold('Trans', 'trans', 0.25, 50, 1, null),
  hold('Filter Sweep', 'filterSweep', 4, 50, 0.8, null),
  hold('Down Echo', 'downEcho', 1, 50, 0.7, null),
  hold('Up Echo', 'upEcho', 1, 50, 0.7, null),
  hold('Spiral', 'spiral', 1, 50, 0.7, null),
  hold('Delay', 'delay', 0.75, 50, 0.7, null),
];

/** PAD FX 2 pads set the Beat FX division (or percent) and gate it while held; with SHIFT they pick a favourite kind. */
export const PAD_FX2_BEATS = [0.125, 0.25, 0.5, 0.75, 1, 2, 4, 8] as const;
export const PAD_FX2_PERCENTS = [10, 20, 35, 50, 65, 80, 90, 100] as const;
export const PAD_FX2_FAVOURITES: BeatFxKind[] = ['echo', 'pingPong', 'reverb', 'roll', 'flanger', 'phaser', 'trans', 'filterSweep'];

export type BeatFxKind = 'delay' | 'echo' | 'upEcho' | 'downEcho' | 'pingPong' | 'spiral' | 'reverb' | 'flanger' | 'phaser' | 'roll' | 'trans' | 'filterSweep' | 'pulseVerb' | 'dubTape' | 'halo' | 'ascend' | 'backwash';
export type FxTarget = 'a' | 'b' | 'master';
export type ColorFxKind = 'filter' | 'space' | 'dubEcho' | 'crush' | 'noise';
/** Allowed `beats` values for the beat FX unit. */
export const BEAT_FX_BEATS = [0.125, 0.25, 0.5, 0.75, 1, 2, 4, 8, 16] as const;

/** Kinds whose BEAT stepper sets `percent` (1..100) instead of a beat division. */
export const BEAT_FX_PERCENT_KINDS = ['reverb', 'halo'] as const;
/** Presets the percent stepper walks through; any whole value in 1..100 is accepted. */
export const BEAT_FX_PERCENT_STEPS = [1, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100] as const;

/** Upper bound on `BeatFxSnapshot.params`; only the first `BEAT_FX_PARAMS[kind].length` entries are meaningful. */
export const BEAT_FX_MAX_PARAMS = 8;

/** Beat divisions the BEAT stepper offers per kind; kinds not listed use `BEAT_FX_BEATS`. */
export const BEAT_FX_KIND_BEATS: Partial<Record<BeatFxKind, readonly number[]>> = {
  pulseVerb: [0.125, 0.25, 0.5, 0.75, 1, 2, 4],
  backwash: [1, 2, 4],
};

/** Kinds whose BEAT stepper steps a sub-parameter (by index) instead of beats or percent. */
export const BEAT_FX_PRIMARY_PARAM: Partial<Record<BeatFxKind, number>> = { ascend: 0 };

/** Kinds with a THROW send, a FREEZE hold or a RELEASE gesture. */
export const BEAT_FX_THROW_KINDS = ['dubTape', 'backwash'] as const;
export const BEAT_FX_FREEZE_KINDS = ['delay', 'echo', 'upEcho', 'downEcho', 'pingPong', 'dubTape', 'halo'] as const;
export const BEAT_FX_RELEASE_KINDS = ['ascend'] as const;

/** One sub-parameter of a kind, beyond level, beats and percent. Choices (`options`) hold the option index. */
export interface BeatFxParamSpec {
  id: string;
  label: string;
  min: number;
  max: number;
  default: number;
  step?: number;
  unit?: '%' | 'ms' | 'beats' | 'st' | 'bars' | 'x' | '';
  options?: string[];
}

/** Mirrors `beat_fx_params` in the Rust common crate; checked by the contract test. */
export const BEAT_FX_PARAMS: Record<BeatFxKind, BeatFxParamSpec[]> = {
  delay: [
    { id: 'feedback', label: 'Feedback', min: 0, max: 100, default: 42, unit: '%' },
    { id: 'filter', label: 'Filter', min: 0, max: 100, default: 50, unit: '%' },
  ],
  echo: [
    { id: 'feedback', label: 'Feedback', min: 0, max: 100, default: 42, unit: '%' },
    { id: 'filter', label: 'Filter', min: 0, max: 100, default: 50, unit: '%' },
  ],
  upEcho: [
    { id: 'feedback', label: 'Feedback', min: 0, max: 100, default: 42, unit: '%' },
    { id: 'filter', label: 'Filter', min: 0, max: 100, default: 50, unit: '%' },
  ],
  downEcho: [
    { id: 'feedback', label: 'Feedback', min: 0, max: 100, default: 42, unit: '%' },
    { id: 'filter', label: 'Filter', min: 0, max: 100, default: 50, unit: '%' },
  ],
  pingPong: [
    { id: 'feedback', label: 'Feedback', min: 0, max: 100, default: 42, unit: '%' },
    { id: 'filter', label: 'Filter', min: 0, max: 100, default: 50, unit: '%' },
    { id: 'rRatio', label: 'R ratio', min: 0, max: 7, default: 2, step: 1, unit: 'x', options: ['0.25', '0.5', '0.75', '1', '1.25', '1.5', '1.75', '2'] },
  ],
  spiral: [
    { id: 'feedback', label: 'Feedback', min: 0, max: 150, default: 80, unit: '%' },
    { id: 'pitch', label: 'Pitch', min: -100, max: 100, default: 0, unit: '%' },
  ],
  reverb: [
    { id: 'tone', label: 'Tone', min: 0, max: 100, default: 50, unit: '%' },
    { id: 'noise', label: 'Noise', min: 0, max: 100, default: 50, unit: '%' },
    { id: 'decay', label: 'Decay', min: 1, max: 100, default: 50, unit: '%' },
    { id: 'voicing', label: 'Voicing', min: 0, max: 1, default: 0, step: 1, unit: '', options: ['Classic', 'Tension'] },
    { id: 'bend', label: 'Bend', min: 0, max: 100, default: 20, unit: '%' },
  ],
  flanger: [
    { id: 'feedback', label: 'Feedback', min: 0, max: 100, default: 50, unit: '%' },
  ],
  phaser: [
    { id: 'feedback', label: 'Feedback', min: 0, max: 100, default: 50, unit: '%' },
  ],
  roll: [],
  trans: [],
  filterSweep: [
    { id: 'resonance', label: 'Resonance', min: 0, max: 100, default: 50, unit: '%' },
  ],
  pulseVerb: [
    { id: 'size', label: 'Size', min: 1, max: 100, default: 45, unit: '%' },
    { id: 'chop', label: 'Chop', min: 0, max: 100, default: 70, unit: '%' },
    { id: 'noise', label: 'Noise', min: 0, max: 100, default: 25, unit: '%' },
    { id: 'accelerate', label: 'Accelerate', min: 0, max: 1, default: 0, step: 1, unit: '', options: ['Off', 'On'] },
    { id: 'every', label: 'Every', min: 0, max: 3, default: 2, step: 1, unit: 'bars', options: ['1', '2', '4', '8'] },
  ],
  dubTape: [
    { id: 'feedback', label: 'Feedback', min: 0, max: 105, default: 45, unit: '%' },
    { id: 'tone', label: 'Tone', min: 0, max: 100, default: 50, unit: '%' },
    { id: 'drive', label: 'Drive', min: 0, max: 100, default: 25, unit: '%' },
    { id: 'wobble', label: 'Wobble', min: 0, max: 100, default: 15, unit: '%' },
    { id: 'timeMode', label: 'Time change', min: 0, max: 2, default: 0, step: 1, unit: '', options: ['Clean', 'Tape', 'Reset'] },
    { id: 'send', label: 'Send', min: 0, max: 1, default: 0, step: 1, unit: '', options: ['Always', 'Throw'] },
    { id: 'output', label: 'Output', min: 0, max: 1, default: 0, step: 1, unit: '', options: ['Mix', 'Wet only'] },
  ],
  halo: [
    { id: 'shimmer', label: 'Shimmer', min: 0, max: 100, default: 50, unit: '%' },
    { id: 'tone', label: 'Tone', min: 0, max: 100, default: 50, unit: '%' },
    { id: 'fifth', label: 'Fifth', min: 0, max: 100, default: 0, unit: '%' },
  ],
  ascend: [
    { id: 'length', label: 'Length', min: 0, max: 3, default: 2, step: 1, unit: 'bars', options: ['4', '8', '16', '32'] },
    { id: 'landOn', label: 'Land on', min: 0, max: 1, default: 0, step: 1, unit: '', options: ['Phrase', 'Bar'] },
    { id: 'release', label: 'Release', min: 0, max: 1, default: 0, step: 1, unit: '', options: ['Downbeat', 'Now'] },
  ],
  backwash: [
    { id: 'size', label: 'Size', min: 1, max: 100, default: 60, unit: '%' },
  ],
};

/** Upper bound on `EngineSnapshot.colorFxParams`. */
export const COLOR_FX_MAX_PARAMS = 2;

/** Mirrors `color_fx_params` in the Rust common crate; checked by the contract test. */
export const COLOR_FX_PARAMS: Record<ColorFxKind, BeatFxParamSpec[]> = {
  filter: [],
  space: [
    { id: 'shimmer', label: 'Shimmer', min: 0, max: 100, default: 0, unit: '%' },
  ],
  dubEcho: [],
  crush: [],
  noise: [
    { id: 'level', label: 'Level', min: 0, max: 100, default: 50, unit: '%' },
  ],
};

export type FxTriggerAction = 'throwOn' | 'throwOff' | 'freezeOn' | 'freezeOff' | 'release';

export interface BeatFxSnapshot {
  kind: BeatFxKind;
  target: FxTarget;
  beats: number; // one of BEAT_FX_BEATS
  percent: number; // 1..100, used by BEAT_FX_PERCENT_KINDS
  level: number; // 0..1 wet/depth
  enabled: boolean;
  bpm: number; // tempo the effect is synced to (target deck, else master deck, else 120)
  bands: { low: boolean; mid: boolean; high: boolean }; // FX FREQUENCY bands for the selected kind
  params: number[]; // current values of the selected kind's BEAT_FX_PARAMS (first N of BEAT_FX_MAX_PARAMS entries)
  held: boolean; // a throw, freeze or ascend hold is engaged
  ascendProgress: number | null; // 0..1 through the build, null when no ascent is running
  ascendBars: number | null; // bars the placed ramp spans
  ascendLandingBar: number | null; // bar number the ramp lands on
}

/** Commands accepted by `engine_command`. Tagged by `type`. */
export type UiCommand =
  | { type: 'play'; deck: DeckIndex }
  | { type: 'pause'; deck: DeckIndex }
  | { type: 'togglePlay'; deck: DeckIndex }
  | { type: 'cueDown'; deck: DeckIndex }
  | { type: 'cueUp'; deck: DeckIndex }
  | { type: 'seek'; deck: DeckIndex; secs: number }
  | { type: 'setTempo'; deck: DeckIndex; ratio: number }
  | { type: 'setTempoRange'; deck: DeckIndex; range: TempoRange }
  | { type: 'setKeyLock'; deck: DeckIndex; enabled: boolean }
  | { type: 'setVinylMode'; deck: DeckIndex; enabled: boolean }
  | { type: 'nudge'; deck: DeckIndex; amount: number } // temporary bend, +/- revolutions (e.g. ±0.05)
  | { type: 'hotCue'; deck: DeckIndex; slot: number; pressed: boolean }
  | { type: 'hotCueDelete'; deck: DeckIndex; slot: number }
  | { type: 'loopIn'; deck: DeckIndex }
  | { type: 'loopOut'; deck: DeckIndex }
  | { type: 'reloopExit'; deck: DeckIndex }
  | { type: 'beatLoop'; deck: DeckIndex; beats: number }
  | { type: 'loopScale'; deck: DeckIndex; factor: number }
  | { type: 'beatJump'; deck: DeckIndex; beats: number }
  | { type: 'sync'; deck: DeckIndex; enabled: boolean }
  | { type: 'setMasterDeck'; deck: DeckIndex }
  | { type: 'trim'; channel: DeckIndex; value: number }
  | { type: 'eq'; channel: DeckIndex; band: BandName; value: number }
  | { type: 'filter'; channel: DeckIndex; value: number }
  | { type: 'fader'; channel: DeckIndex; value: number }
  | { type: 'pfl'; channel: DeckIndex; enabled: boolean }
  | { type: 'crossfader'; value: number }
  | { type: 'crossfaderCurve'; curve: CrossfaderCurve }
  | { type: 'eqCurve'; curve: EqCurve }
  | { type: 'masterLevel'; value: number }
  | { type: 'headphoneMix'; value: number }
  | { type: 'headphoneLevel'; value: number }
  | { type: 'quantize'; enabled: boolean }
  | { type: 'hardwarePhones'; enabled: boolean }
  | { type: 'beatFxKind'; kind: BeatFxKind }
  | { type: 'beatFxTarget'; target: FxTarget }
  | { type: 'beatFxBeats'; beats: number }
  | { type: 'beatFxPercent'; value: number }
  | { type: 'beatFxLevel'; value: number }
  | { type: 'beatFxEnabled'; enabled: boolean }
  | { type: 'beatFxBands'; low: boolean; mid: boolean; high: boolean }
  | { type: 'beatFxParam'; index: number; value: number }
  | { type: 'beatFxTrigger'; action: FxTriggerAction }
  | { type: 'colorFx'; kind: ColorFxKind }
  | { type: 'colorFxParam'; index: number; value: number }
  | { type: 'padFx'; deck: DeckIndex; slot: number; shift: boolean; pressed: boolean } // PAD FX 1 pad, as on the hardware
  | { type: 'releaseFx' } // SHIFT + FX ON/OFF
  | { type: 'releaseFxConfig'; kind: ReleaseFxKind; beats: number }
  | { type: 'smartCfx'; enabled: boolean }
  | { type: 'smartCfxPreset'; preset: SmartCfxPreset };

export type AnalysisState = 'pending' | 'running' | 'done' | 'failed';

export interface TrackSummary {
  id: number;
  path: string;
  title: string;
  artist: string;
  album: string;
  genre: string;
  durationSecs: number;
  bpm: number | null; // analysed BPM, else tag BPM
  key: string | null; // Camelot ("8A"): analysed, else normalised tag key
  keyName: string | null; // musical notation ("Am", "F#")
  analysis: AnalysisState;
  hasArtwork: boolean;
  fileType: string; // "flac", "mp3", ...
  addedAt: number; // unix secs
}

export interface Playlist {
  id: number;
  name: string;
  trackCount: number;
  parentId: number | null;
  isFolder: boolean;
}

export interface ImportResult {
  added: number;
  skipped: number;
  failed: number;
}

export interface PlaylistImportSummary {
  id: number;
  name: string;
  found: number;
  total: number;
  missing: string[];
}

export interface PlaylistImportReport {
  added: number;
  skipped: number;
  failed: number;
  playlists: PlaylistImportSummary[];
}

export interface LoadResult {
  deck: DeckIndex;
  track: TrackSummary;
}

export type PhraseKind = 'intro' | 'build' | 'drop' | 'break' | 'outro';

export interface Phrase {
  startSecs: number;
  endSecs: number;
  kind: PhraseKind;
  bars: number;
  energy: number; // 0..1 relative to the track
  fillStartSecs: number | null; // fill-in leading into the next phrase
}

/** Decoded from the binary returned by `track_waveform`. */
export interface Waveform {
  pointsPerSec: number;
  peak: Uint8Array; // 0..255 per point, full band
  low: Uint8Array;
  mid: Uint8Array;
  high: Uint8Array;
}

export interface OutputDeviceInfo {
  id: string;
  name: string;
  maxChannels: number;
  sampleRates: number[];
  isDefault: boolean;
}

export type RoutingMode = 'master-and-phones' | 'master-only' | 'split-mono';

export interface Routing {
  mode: RoutingMode;
  master: [number, number]; // zero-based channel indices
  phones: [number, number];
}

export interface ActiveOutput {
  deviceId: string;
  deviceName: string;
  sampleRate: number;
  channels: number;
  requestedBuffer: number | null;
  routing: Routing;
  sampleFormat: string;
}

export interface OutputStatus {
  state: 'running' | 'lost' | 'silent';
  active: ActiveOutput | null;
  lastError: string | null;
  callbacks: number;
  lastCallbackFrames: number;
  streamErrors: number;
  xruns: number; // buffer underruns/overruns reported by the device
  secondary: SecondaryOutputStatus | null;
}

export type SecondarySource = 'master' | 'cue';
export type SecondaryState = 'running' | 'lost';

export interface SecondaryOutputRequest {
  deviceId: string;
  source: SecondarySource;
  level: number; // 0..1, 0.8 = unity
  channels: [number, number]; // zero-based output channel pair on the device
}

export interface SecondaryOutputStatus {
  deviceId: string;
  deviceName: string;
  source: SecondarySource;
  level: number;
  state: SecondaryState;
  sampleRate: number;
  latencyMs: number; // ring fill plus one device buffer, i.e. how far behind the main output
  driftPpm: number; // clock correction currently applied by the resampler
  underruns: number;
  channels: [number, number];
}

export interface OutputRequest {
  deviceId?: string | null;
  sampleRate?: number | null;
  bufferFrames?: number | null;
  routing?: Routing | null;
}

export interface ControllerStatus {
  connected: boolean;
  mapping: string;
  inputPort: string | null;
  outputPort: string | null;
  availableInputs: string[];
  availableOutputs: string[];
  messagesIn: number;
  messagesOut: number;
  lastError: string | null;
}

export interface MidiLogEntry {
  timeMs: number;
  direction: 'in' | 'out' | 'injected';
  bytes: number[];
  summary: string;
}

/** Actions requested by the hardware controller that the UI must execute (browser lives in the UI). */
export type ControllerAppAction =
  | { type: 'browseScroll'; delta: number }
  | { type: 'browseToggleFocus' }
  | { type: 'loadSelected'; deck: DeckIndex }
  | { type: 'hardwareKnob'; name: 'masterLevel' | 'headphoneMix' | 'headphoneLevel'; value: number }
  | ({ type: 'fxControl' } & FxControlState);

export type PadMode = 'hotCue' | 'padFx1' | 'beatJump' | 'sampler' | 'keyboard' | 'padFx2' | 'beatLoop' | 'keyShift';
export type FxKnobTarget = 'level' | 'param' | 'padLevel';
export type FxHudControl = 'level' | 'param' | 'page' | 'beats' | 'kind' | 'onOff' | 'release' | 'padFx' | 'padMode' | 'smartCfx' | 'smartPreset';

/** LEVEL/DEPTH is waiting to catch up with the stored value of its new target (soft pickup). */
export interface FxPickup {
  target: FxKnobTarget;
  deck: number | null; // padLevel only
  index: number | null; // param only
  value: number; // stored value, 0..1 of the target's range
  knob: number; // physical knob, 0..1
}

export interface FxHud {
  seq: number; // changes on every FX hardware gesture
  control: FxHudControl;
  deck: number | null;
}

export interface HeldPad {
  pad: number; // 0..7
  shift: boolean;
  mode: PadMode;
}

/** Controller-side FX state sent with every change (`controller://app-action`, type `fxControl`). */
export interface FxControlState {
  shift: boolean; // either deck's SHIFT is held
  knobTarget: FxKnobTarget; // what LEVEL/DEPTH drives now
  focus: number | null; // BEAT_FX_PARAMS index that SHIFT + LEVEL/DEPTH drives for the current kind
  pickup: FxPickup | null;
  padModes: [PadMode, PadMode];
  held: [HeldPad | null, HeldPad | null];
  smartArming: [boolean, boolean]; // CFX knob ignored until it passes the centre
  hud: FxHud | null;
}

export interface AnalysisProgress {
  trackId: number;
  state: AnalysisState;
  queued: number; // remaining in queue
}

export interface SpotifyUser {
  id: string;
  displayName: string;
  product: string | null; // "premium", "free"
  imageUrl: string | null;
}

export interface SpotifyStatus {
  clientId: string | null;
  connected: boolean;
  user: SpotifyUser | null;
  redirectUri: string; // must be registered in the Spotify developer dashboard
  lastError: string | null;
}

export interface SpotifyPlaylist {
  id: string;
  name: string;
  owner: string;
  trackCount: number;
  imageUrl: string | null;
}

export interface SpotifyTrack {
  id: string;
  name: string;
  artists: string[];
  album: string;
  durationMs: number;
  isrc: string | null;
  imageUrl: string | null;
  localTrackId: number | null; // best local library match
  matchScore: number | null; // 0..1
}

export interface SpotifyImportResult {
  playlist: Playlist;
  matched: number;
  unmatched: number;
}

export interface SetSummary {
  id: number;
  name: string;
  startedAt: number; // Unix milliseconds
  endedAt: number | null;
  durationSecs: number;
  trackCount: number;
  hasRecording: boolean;
  live: boolean;
}

export interface SetEntry {
  id: number;
  trackId: number | null;
  title: string;
  artist: string;
  path: string;
  key: string | null;
  bpm: number | null;
  audibleSecs: number; // from the set start
  tookOverSecs: number | null;
  teased: boolean;
}

export interface SetRecording {
  id: number;
  path: string;
  startedSecs: number; // from the set start
  stoppedSecs: number | null;
  format: string;
}

export interface SetDetail {
  summary: SetSummary;
  entries: SetEntry[];
  recordings: SetRecording[];
}

export type AudioFormat = 'wav' | 'flac';
export type ExportFormat = 'text' | 'csv' | 'cue' | 'fcpXml';

export interface MarkerFormats {
  csv: boolean;
  cue: boolean;
  fcpXml: boolean;
}

export interface RecordingSettings {
  folder: string;
  format: AudioFormat;
  bits: 16 | 24;
  sampleRate: 44100 | 48000;
  markers: MarkerFormats;
}

export interface RecorderStatus {
  path: string;
  elapsedSecs: number;
  bytes: number;
  droppedSecs: number;
  warning: string | null;
  finished: boolean;
}
