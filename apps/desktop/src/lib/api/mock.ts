// PREVIEW ONLY — no audio engine. Loaded exclusively when `window.__TAURI_INTERNALS__` is absent
// (plain `pnpm dev` in a browser) so the layout can be reviewed. Never active inside Tauri.
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
  BEAT_FX_KIND_BEATS, BEAT_FX_MAX_PARAMS, BEAT_FX_BEATS, BEAT_FX_PARAMS, BEAT_FX_PERCENT_KINDS, COLOR_FX_PARAMS, EVENTS, PAD_FX1_BANK, PAD_FX1_SHIFT_BANK,
  PAD_FX2_BEATS, PAD_FX2_FAVOURITES, PAD_FX2_PERCENTS, SMART_CFX_PRESETS, type api as RealApi,
} from './index';
import type {
  ActiveOutput, AnalysisProgress, AnalysisState, BeatFxKind, BeatFxSnapshot, ChannelSnapshot, ControllerAppAction, ControllerStatus,
  DeckSnapshot, EngineSnapshot, FxControlState, FxHudControl, FxKnobTarget, MidiLogEntry, PadMode, OutputDeviceInfo, OutputRequest, OutputStatus, Phrase, PhraseKind, Playlist,
  Routing, SecondaryOutputRequest, SecondaryOutputStatus, SpotifyPlaylist, SpotifyStatus, SpotifyTrack, TempoRange, TrackSummary, UiCommand, Waveform,
} from './types';

type Handler = (payload: unknown) => void;
const listeners = new Map<string, Set<Handler>>();

function emit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((h) => h(payload));
}

export function mockOn<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  let set = listeners.get(event);
  if (!set) listeners.set(event, (set = new Set()));
  set.add(handler as Handler);
  return Promise.resolve(() => set.delete(handler as Handler));
}

// ───────────────────────── seeded content ─────────────────────────

function rng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

interface MockTrack extends TrackSummary {
  firstBeat: number;
  hue: number;
}

type Seed = Omit<MockTrack, 'id' | 'path' | 'album' | 'hasArtwork' | 'addedAt' | 'analysis' | 'fileType' | 'keyName'>;

const KEY_NAMES: Record<string, string> = {
  '1A': 'Abm', '2A': 'Ebm', '3A': 'Bbm', '4A': 'Fm', '5A': 'Cm', '6A': 'Gm', '7A': 'Dm', '8A': 'Am', '9A': 'Em', '10A': 'Bm', '11A': 'F#m', '12A': 'Dbm',
  '1B': 'B', '2B': 'F#', '3B': 'Db', '4B': 'Ab', '5B': 'Eb', '6B': 'Bb', '7B': 'F', '8B': 'C', '9B': 'G', '10B': 'D', '11B': 'A', '12B': 'E',
};

const SEED_TRACKS: Seed[] = [
  { title: 'Night Shift', artist: 'Kora Vale', genre: 'Deep House', durationSecs: 372, bpm: 122, key: '8A', firstBeat: 0.21, hue: 205 },
  { title: 'Copper Skies', artist: 'Nuvo & Sable', genre: 'Melodic House', durationSecs: 405, bpm: 124, key: '9A', firstBeat: 0.34, hue: 280 },
  { title: 'Static Bloom', artist: 'Ilya Marsh', genre: 'Techno', durationSecs: 428, bpm: 128, key: '5A', firstBeat: 0.12, hue: 335 },
  { title: 'Golden Hour Protocol', artist: 'Peregrine', genre: 'Progressive', durationSecs: 391, bpm: 126, key: '11B', firstBeat: 0.5, hue: 32 },
  { title: 'Low Tide Memory', artist: 'Anouk Reyes', genre: 'Organic House', durationSecs: 344, bpm: 120, key: '6A', firstBeat: 0.18, hue: 165 },
  { title: 'Velvet Circuit', artist: 'Kora Vale', genre: 'Deep House', durationSecs: 318, bpm: 123, key: '8A', firstBeat: 0.27, hue: 250 },
  { title: 'Parallel Lines', artist: 'Haru Tanaka', genre: 'Tech House', durationSecs: 366, bpm: 125, key: '7A', firstBeat: 0.09, hue: 12 },
  { title: 'Saltwater Radio', artist: 'Nuvo & Sable', genre: 'Afro House', durationSecs: 411, bpm: 121, key: '8B', firstBeat: 0.4, hue: 195 },
  { title: 'Afterimage', artist: 'Ilya Marsh', genre: 'Techno', durationSecs: 452, bpm: 130, key: '3A', firstBeat: 0.15, hue: 310 },
  { title: 'Paper Lanterns', artist: 'Mirela', genre: 'Melodic House', durationSecs: 383, bpm: 122, key: '10A', firstBeat: 0.3, hue: 42 },
  { title: 'Concrete Garden', artist: 'Peregrine', genre: 'Progressive', durationSecs: 398, bpm: 127, key: '12A', firstBeat: 0.22, hue: 120 },
  { title: 'Sundial', artist: 'Anouk Reyes', genre: 'Organic House', durationSecs: 357, bpm: 119, key: '2B', firstBeat: 0.44, hue: 70 },
  { title: 'Hollow Frequencies', artist: 'Haru Tanaka', genre: 'Techno', durationSecs: 437, bpm: 131, key: '1A', firstBeat: 0.11, hue: 270 },
  { title: 'Quiet Machines', artist: 'Mirela', genre: 'Deep House', durationSecs: 349, bpm: 121, key: '9A', firstBeat: 0.26, hue: 220 },
  { title: 'Tessellate', artist: 'Kora Vale', genre: 'Tech House', durationSecs: 376, bpm: 124, key: '7A', firstBeat: 0.19, hue: 90 },
  { title: 'Marigold Line', artist: 'Anouk Reyes', genre: 'Afro House', durationSecs: 402, bpm: 122, key: '8A', firstBeat: 0.31, hue: 25 },
  { title: 'Signal & Noise', artist: 'Ilya Marsh', genre: 'Techno', durationSecs: 441, bpm: 129, key: '4A', firstBeat: 0.14, hue: 350 },
  { title: 'Harbour Lights', artist: 'Nuvo & Sable', genre: 'Melodic House', durationSecs: 388, bpm: 123, key: '9B', firstBeat: 0.37, hue: 180 },
  { title: 'Daybreak Theory', artist: 'Peregrine', genre: 'Progressive', durationSecs: 415, bpm: 125, key: '8B', firstBeat: 0.23, hue: 48 },
  { title: 'Fresh Import (analysing)', artist: 'Unknown Artist', genre: '', durationSecs: 301, bpm: null, key: null, firstBeat: 0.2, hue: 100 },
];

const tracks: MockTrack[] = SEED_TRACKS.map((t, i) => ({
  ...t,
  keyName: t.key ? (KEY_NAMES[t.key] ?? null) : null,
  id: i + 1,
  path: `/Music/Preview/${t.artist} - ${t.title}.${i % 4 === 0 ? 'flac' : i % 3 === 0 ? 'wav' : 'mp3'}`,
  album: 'Preview Sessions',
  hasArtwork: true,
  addedAt: 1_760_000_000 - i * 86_400,
  analysis: (i === SEED_TRACKS.length - 1 ? 'pending' : 'done') as AnalysisState,
  fileType: i % 4 === 0 ? 'flac' : i % 3 === 0 ? 'wav' : 'mp3',
}));

const PHRASE_ENERGY: Record<PhraseKind, number> = { intro: 0.25, build: 0.55, drop: 0.95, break: 0.3, outro: 0.2 };

function phrasesFor(t: MockTrack): Phrase[] {
  if (t.analysis !== 'done' || !t.bpm) return [];
  const bar = (60 / t.bpm) * 4;
  const chunk = bar * 8;
  const kinds: PhraseKind[] = ['build', 'drop', 'break', 'drop'];
  const rand = rng(t.id * 31);
  const out: Phrase[] = [];
  let at = t.firstBeat;
  let i = 0;
  while (at < t.durationSecs - 1) {
    const remaining = t.durationSecs - at;
    const kind: PhraseKind = i < 2 ? 'intro' : remaining < chunk * 2.2 ? 'outro' : kinds[(i - 2) % 4];
    const bars = i === 2 ? 2 : 8;
    const end = Math.min(t.durationSecs, at + bar * bars);
    const prev = out[out.length - 1];
    if (prev && prev.kind === kind) {
      prev.endSecs = end;
      prev.bars += bars;
    } else {
      out.push({ startSecs: at, endSecs: end, kind, bars, energy: Math.min(1, PHRASE_ENERGY[kind] + (rand() - 0.5) * 0.12), fillStartSecs: null });
    }
    at = end;
    i++;
  }
  return out;
}

const waveformCache = new Map<number, Waveform>();
function waveformFor(t: MockTrack): Waveform {
  const hit = waveformCache.get(t.id);
  if (hit) return hit;
  const pps = 100;
  const n = Math.floor(t.durationSecs * pps);
  const low = new Uint8Array(n);
  const mid = new Uint8Array(n);
  const high = new Uint8Array(n);
  const peak = new Uint8Array(n);
  const rand = rng(t.id * 7919);
  const bpm = t.bpm ?? 124;
  const beat = 60 / bpm;
  let envMid = 0;
  for (let i = 0; i < n; i++) {
    const time = i / pps;
    const bt = (time - t.firstBeat) / beat;
    const phase = bt - Math.floor(bt);
    const bar = Math.floor(bt / 4);
    const block = Math.floor(bar / 8) % 4;
    const section = [0.55, 1, 0.35, 1][block] * (time < 20 ? time / 20 : 1) * (t.durationSecs - time < 24 ? (t.durationSecs - time) / 24 : 1);
    const kick = Math.exp(-phase * 5.5);
    const off = (bt % 1) > 0.5 ? Math.exp(-((bt % 1) - 0.5) * 12) : 0;
    const snare = Math.floor(bt) % 2 === 1 ? Math.exp(-phase * 9) : 0;
    const hat = Math.exp(-(((bt * 2) % 1)) * 14);
    envMid = envMid * 0.985 + (0.55 + 0.45 * Math.sin(bar * 0.9 + t.id)) * 0.015;
    const l = (0.12 + 0.88 * kick) * section;
    const m = (0.18 + 0.5 * envMid + 0.55 * snare + 0.25 * off) * section * (block === 2 ? 1.25 : 0.9);
    const h = (0.08 + 0.55 * hat + 0.2 * rand()) * section * (block === 0 ? 0.55 : 1);
    low[i] = Math.min(255, Math.round(255 * l));
    mid[i] = Math.min(255, Math.round(255 * m));
    high[i] = Math.min(255, Math.round(255 * h));
    peak[i] = Math.min(255, Math.max(low[i], mid[i], high[i]));
  }
  const wf = { pointsPerSec: pps, peak, low, mid, high };
  waveformCache.set(t.id, wf);
  return wf;
}

function artworkFor(t: MockTrack): string {
  const c = document.createElement('canvas');
  c.width = c.height = 160;
  const g = c.getContext('2d')!;
  const grad = g.createLinearGradient(0, 0, 160, 160);
  grad.addColorStop(0, `hsl(${t.hue} 48% 46%)`);
  grad.addColorStop(1, `hsl(${(t.hue + 50) % 360} 42% 20%)`);
  g.fillStyle = grad;
  g.fillRect(0, 0, 160, 160);
  g.globalAlpha = 0.28;
  g.fillStyle = '#fff';
  for (let i = 0; i < 6; i++) {
    g.beginPath();
    g.arc(80, 80, 12 + i * 14, 0, Math.PI * 2);
    g.lineWidth = 3;
    g.strokeStyle = '#fff';
    g.stroke();
  }
  return c.toDataURL('image/png');
}

// ───────────────────────── engine simulation ─────────────────────────

const rangeOf = (r: TempoRange) => (r === 'Six' ? 0.06 : r === 'Ten' ? 0.1 : r === 'Sixteen' ? 0.16 : 1);

function emptyDeck(): DeckSnapshot {
  return {
    loaded: false, trackToken: 0, playing: false, positionSecs: 0, durationSecs: 0, tempoRatio: 1, playRate: 0, faderTempoRatio: 1,
    tempoRange: 'Ten', keyLock: true, vinylMode: false, hasGrid: false, gridBpm: 0, firstBeatSecs: 0, bpm: 0, beatPhase: 0,
    cueSecs: 0, hotCues: Array(8).fill(null), loopInSecs: null, loopOutSecs: null, loopActive: false, syncEnabled: false,
    isMaster: false, jogTouched: false, previewing: false, atEnd: false,
  };
}
const emptyChannel = (): ChannelSnapshot => ({ trim: 0.5, eqHigh: 0.5, eqMid: 0.5, eqLow: 0.5, filter: 0, fader: 0.85, pfl: false, meterL: 0, meterR: 0 });

const state: EngineSnapshot = {
  decks: [emptyDeck(), emptyDeck()],
  channels: [emptyChannel(), emptyChannel()],
  master: {
    crossfader: 0.5, curve: 'equal-power', eqCurve: 'eq', level: 0.8, headphoneMix: 0.5, headphoneLevel: 0.6, meterL: 0, meterR: 0,
    cueMeterL: 0, cueMeterR: 0, clipping: false, limiting: false, hardwarePhones: true,
  },
  quantize: true, masterDeck: null, sampleRate: 48000, framesProcessed: 0, outputLatencySecs: 0.01, callbackLoad: 0.1, cueAvailable: true,
  beatFx: {
    kind: 'echo', target: 'master', beats: 1, percent: 50, level: 0.5, enabled: false, bpm: 120, bands: { low: false, mid: true, high: true },
    params: [42, 50, 0, 0, 0, 0, 0, 0], held: false, ascendProgress: null, ascendBars: null, ascendLandingBar: null,
  },
  colorFx: 'filter',
  colorFxParams: [0, 0],
  padFx: [
    { voice: null, sounding: false, release: null, releaseProgress: 0 },
    { voice: null, sounding: false, release: null, releaseProgress: 0 },
  ],
  releaseFx: { kind: 'echoOut', beats: 2, running: null, progress: 0 },
  smartCfx: { enabled: false, preset: 'echoWash' },
};

const LOW_DRY_KINDS: BeatFxKind[] = ['delay', 'echo', 'upEcho', 'downEcho', 'pingPong', 'spiral', 'reverb', 'pulseVerb', 'dubTape', 'halo', 'backwash'];
const paramSets = new Map<BeatFxKind, number[]>();
const ASCEND_BARS = [4, 8, 16, 32];
const fxHold = { throw: false, freeze: false, ascendElapsed: 0, ascending: false };

function paramsFor(kind: BeatFxKind): number[] {
  const saved = paramSets.get(kind);
  if (saved) return saved;
  const values = BEAT_FX_PARAMS[kind].map((p) => p.default);
  while (values.length < BEAT_FX_MAX_PARAMS) values.push(0);
  return values;
}

function snapBeats(kind: BeatFxKind, beats: number): number {
  const options = BEAT_FX_KIND_BEATS[kind] ?? BEAT_FX_BEATS;
  return options.reduce((best, b) => (Math.abs(Math.log(b / beats)) < Math.abs(Math.log(best / beats)) ? b : best), options[0]);
}

function startAscent() {
  fxHold.ascendElapsed = 0;
  fxHold.ascending = true;
  state.beatFx.ascendProgress = 0;
}

function stopAscent() {
  fxHold.ascending = false;
  Object.assign(state.beatFx, { ascendProgress: null, ascendBars: null, ascendLandingBar: null, held: false });
}
const bandSets = new Map<BeatFxKind, BeatFxSnapshot['bands']>();

function bandsFor(kind: BeatFxKind): BeatFxSnapshot['bands'] {
  return bandSets.get(kind) ?? { low: !LOW_DRY_KINDS.includes(kind), mid: true, high: true };
}

const bend = [0, 0];
const hotHeld: (number | null)[] = [null, null];

function beatLen(d: DeckSnapshot) { return d.hasGrid ? 60 / d.gridBpm : 0.5; }
function snapToBeat(d: DeckSnapshot, t: number) {
  if (!state.quantize || !d.hasGrid) return t;
  const b = beatLen(d);
  return d.firstBeatSecs + Math.round((t - d.firstBeatSecs) / b) * b;
}

function beatFxBpm(): number {
  const t = state.beatFx.target;
  const i = t === 'a' ? 0 : t === 'b' ? 1 : state.masterDeck;
  const d = i === null ? null : state.decks[i];
  return d && d.loaded && d.bpm > 0 ? d.bpm : (state.masterDeck !== null && state.decks[state.masterDeck].bpm > 0 ? state.decks[state.masterDeck].bpm : 120);
}

function recomputeSync() {
  const m = state.masterDeck;
  state.decks.forEach((d, i) => {
    d.isMaster = m === i;
    if (d.syncEnabled && m !== null && m !== i && state.decks[m].loaded && state.decks[m].bpm > 0 && d.hasGrid) {
      const target = state.decks[m].bpm;
      d.tempoRatio = target / d.gridBpm;
    } else {
      d.tempoRatio = d.faderTempoRatio;
    }
    d.bpm = d.hasGrid ? d.gridBpm * d.tempoRatio : 0;
  });
}

function setMasterIfNeeded() {
  if (state.masterDeck !== null && state.decks[state.masterDeck].loaded) return;
  const i = state.decks.findIndex((d) => d.playing);
  state.masterDeck = i >= 0 ? i : state.decks.findIndex((d) => d.loaded) >= 0 ? state.decks.findIndex((d) => d.loaded) : null;
}

function engineCommand(c: UiCommand) {
  const dk = 'deck' in c ? state.decks[c.deck] : null;
  switch (c.type) {
    case 'play': dk!.playing = true; dk!.atEnd = false; break;
    case 'pause': dk!.playing = false; break;
    case 'togglePlay': if (dk!.loaded) { dk!.playing = !dk!.playing; dk!.previewing = false; } break;
    case 'cueDown': {
      const d = dk!;
      if (!d.loaded) break;
      if (d.playing) { d.playing = false; d.positionSecs = d.cueSecs; }
      else if (Math.abs(d.positionSecs - d.cueSecs) < 0.05) { d.playing = true; d.previewing = true; }
      else d.cueSecs = snapToBeat(d, d.positionSecs);
      break;
    }
    case 'cueUp': if (dk!.previewing) { dk!.playing = false; dk!.previewing = false; dk!.positionSecs = dk!.cueSecs; } break;
    case 'seek': dk!.positionSecs = Math.max(0, Math.min(dk!.durationSecs, c.secs)); break;
    case 'setTempo': dk!.faderTempoRatio = Math.max(1 - rangeOf(dk!.tempoRange), Math.min(1 + rangeOf(dk!.tempoRange), c.ratio)); break;
    case 'setTempoRange': dk!.tempoRange = c.range; dk!.faderTempoRatio = Math.max(1 - rangeOf(c.range), Math.min(1 + rangeOf(c.range), dk!.faderTempoRatio)); break;
    case 'setKeyLock': dk!.keyLock = c.enabled; break;
    case 'setVinylMode': dk!.vinylMode = c.enabled; break;
    case 'nudge': bend[c.deck] = c.amount; setTimeout(() => (bend[c.deck] = 0), 120); break;
    case 'hotCue': {
      const d = dk!;
      if (!d.loaded) break;
      if (c.pressed) {
        const cue = d.hotCues[c.slot];
        if (!cue) {
          d.hotCues[c.slot] = { positionSecs: snapToBeat(d, d.positionSecs), color: [0x1ee3ff, 0xff4aa8, 0xffc247, 0x3ee8a2, 0xa06bff, 0xff7a3d, 0x4d9bff, 0xf5f5ff][c.slot] };
        } else {
          d.positionSecs = cue.positionSecs;
          if (!d.playing) { d.playing = true; hotHeld[c.deck] = c.slot; d.previewing = true; }
        }
      } else if (hotHeld[c.deck] === c.slot) {
        hotHeld[c.deck] = null;
        d.playing = false; d.previewing = false;
        d.positionSecs = d.hotCues[c.slot]?.positionSecs ?? d.positionSecs;
      }
      break;
    }
    case 'hotCueDelete': dk!.hotCues[c.slot] = null; break;
    case 'loopIn': dk!.loopInSecs = snapToBeat(dk!, dk!.positionSecs); dk!.loopOutSecs = null; dk!.loopActive = false; break;
    case 'loopOut':
      if (dk!.loopInSecs !== null) { dk!.loopOutSecs = snapToBeat(dk!, dk!.positionSecs); if (dk!.loopOutSecs > dk!.loopInSecs) dk!.loopActive = true; }
      break;
    case 'reloopExit':
      if (dk!.loopActive) dk!.loopActive = false;
      else if (dk!.loopInSecs !== null && dk!.loopOutSecs !== null) dk!.loopActive = true;
      break;
    case 'beatLoop': {
      const d = dk!;
      const start = snapToBeat(d, d.positionSecs);
      d.loopInSecs = start; d.loopOutSecs = start + c.beats * beatLen(d); d.loopActive = true;
      break;
    }
    case 'loopScale': {
      const d = dk!;
      if (d.loopInSecs !== null && d.loopOutSecs !== null) d.loopOutSecs = d.loopInSecs + (d.loopOutSecs - d.loopInSecs) * c.factor;
      break;
    }
    case 'beatJump': dk!.positionSecs = Math.max(0, Math.min(dk!.durationSecs, dk!.positionSecs + c.beats * beatLen(dk!))); break;
    case 'sync': dk!.syncEnabled = c.enabled; if (c.enabled && state.masterDeck === null) state.masterDeck = c.deck === 0 ? 1 : 0; break;
    case 'setMasterDeck': state.masterDeck = c.deck; break;
    case 'trim': state.channels[c.channel].trim = c.value; break;
    case 'eq': state.channels[c.channel][c.band === 'High' ? 'eqHigh' : c.band === 'Mid' ? 'eqMid' : 'eqLow'] = c.value; break;
    case 'filter':
      if (fxc.smartArming[c.channel] && (Math.abs(c.value) <= 0.02 || Math.sign(c.value) !== Math.sign(state.channels[c.channel].filter))) {
        fxc.smartArming = fxc.smartArming.map((a, i) => (i === c.channel ? false : a)) as [boolean, boolean];
        emitFxc();
      }
      state.channels[c.channel].filter = c.value;
      break;
    case 'fader': state.channels[c.channel].fader = c.value; break;
    case 'pfl': state.channels[c.channel].pfl = c.enabled; break;
    case 'crossfader': state.master.crossfader = c.value; break;
    case 'crossfaderCurve': state.master.curve = c.curve; break;
    case 'eqCurve': state.master.eqCurve = c.curve; break;
    case 'masterLevel': state.master.level = c.value; break;
    case 'headphoneMix': state.master.headphoneMix = c.value; break;
    case 'headphoneLevel': state.master.headphoneLevel = c.value; break;
    case 'quantize': state.quantize = c.enabled; break;
    case 'hardwarePhones': state.master.hardwarePhones = c.enabled; break;
    case 'beatFxKind':
      bandSets.set(state.beatFx.kind, state.beatFx.bands);
      paramSets.set(state.beatFx.kind, state.beatFx.params);
      state.beatFx.kind = c.kind;
      state.beatFx.bands = bandsFor(c.kind);
      state.beatFx.params = paramsFor(c.kind);
      state.beatFx.beats = snapBeats(c.kind, state.beatFx.beats);
      fxHold.throw = fxHold.freeze = false;
      stopAscent();
      if (state.beatFx.enabled && c.kind === 'ascend') startAscent();
      break;
    case 'beatFxParam': {
      const spec = BEAT_FX_PARAMS[state.beatFx.kind][c.index];
      if (!spec || !Number.isFinite(c.value)) break;
      let v = Math.min(spec.max, Math.max(spec.min, c.value));
      if (spec.step) v = spec.min + Math.round((v - spec.min) / spec.step) * spec.step;
      state.beatFx.params = state.beatFx.params.map((p, i) => (i === c.index ? v : p));
      break;
    }
    case 'beatFxTrigger': {
      const on = c.action === 'throwOn' || c.action === 'freezeOn';
      if (c.action === 'throwOn' || c.action === 'throwOff') fxHold.throw = on && state.beatFx.enabled;
      if (c.action === 'freezeOn' || c.action === 'freezeOff') fxHold.freeze = on && state.beatFx.enabled;
      if (c.action === 'release' && state.beatFx.kind === 'ascend') { state.beatFx.enabled = false; stopAscent(); }
      state.beatFx.held = fxHold.throw || fxHold.freeze || (fxHold.ascending && (state.beatFx.ascendProgress ?? 0) >= 1);
      break;
    }
    case 'beatFxTarget': state.beatFx.target = c.target; break;
    case 'beatFxBeats': state.beatFx.beats = snapBeats(state.beatFx.kind, c.beats); break;
    case 'beatFxPercent': state.beatFx.percent = Math.min(100, Math.max(1, Math.round(c.value))); break;
    case 'beatFxLevel': state.beatFx.level = c.value; break;
    case 'beatFxEnabled':
      state.beatFx.enabled = c.enabled;
      if (!c.enabled) { fxHold.throw = fxHold.freeze = false; state.beatFx.held = false; stopAscent(); }
      else if (state.beatFx.kind === 'ascend') startAscent();
      break;
    case 'beatFxBands': state.beatFx.bands = { low: c.low, mid: c.mid, high: c.high }; break;
    case 'padFx': padFxCommand(c.deck, c.slot, c.shift, c.pressed); break;
    case 'releaseFx':
      Object.assign(state.releaseFx, { running: state.releaseFx.kind, progress: 0 });
      state.beatFx.enabled = false;
      break;
    case 'releaseFxConfig': Object.assign(state.releaseFx, { kind: c.kind, beats: c.beats }); break;
    case 'smartCfx':
      state.smartCfx.enabled = c.enabled;
      armSmart();
      break;
    case 'smartCfxPreset':
      state.smartCfx.preset = c.preset;
      armSmart();
      break;
    case 'colorFx':
      state.colorFx = c.kind;
      state.colorFxParams = [0, 0].map((_, i) => COLOR_FX_PARAMS[c.kind][i]?.default ?? 0);
      break;
    case 'colorFxParam': {
      const spec = COLOR_FX_PARAMS[state.colorFx][c.index];
      if (spec) {
        const next = [...state.colorFxParams];
        next[c.index] = Math.min(spec.max, Math.max(spec.min, c.value));
        state.colorFxParams = next;
      }
      break;
    }
  }
  setMasterIfNeeded();
  recomputeSync();
}

let last = performance.now();
const meterHold = [0, 0, 0, 0, 0, 0];

function tick() {
  const now = performance.now();
  const dt = Math.min(0.05, (now - last) / 1000);
  last = now;
  recomputeSync();
  state.beatFx.bpm = beatFxBpm();
  if (fxHold.ascending && state.beatFx.kind === 'ascend') {
    const bars = ASCEND_BARS[state.beatFx.params[0]] ?? 16;
    const barSecs = 240 / state.beatFx.bpm;
    fxHold.ascendElapsed += dt;
    const progress = Math.min(1, fxHold.ascendElapsed / (bars * barSecs));
    const master = state.masterDeck === null ? null : state.decks[state.masterDeck];
    const startBar = master ? Math.floor((master.positionSecs - master.firstBeatSecs) / barSecs) + 1 : 1;
    Object.assign(state.beatFx, { ascendProgress: progress, ascendBars: bars, ascendLandingBar: startBar + Math.round(bars * (1 - progress)), held: progress >= 1 });
  }
  tickFx(dt);
  state.decks.forEach((d, i) => {
    d.playRate = d.playing && d.loaded ? d.tempoRatio + bend[i] : 0;
    if (d.playing && d.loaded) {
      d.positionSecs += dt * d.playRate;
      if (d.loopActive && d.loopInSecs !== null && d.loopOutSecs !== null && d.positionSecs >= d.loopOutSecs) {
        d.positionSecs = d.loopInSecs + (d.positionSecs - d.loopOutSecs);
      }
      if (d.positionSecs >= d.durationSecs) {
        d.positionSecs = d.durationSecs; d.playing = false; d.atEnd = true;
      }
    }
    if (d.hasGrid) {
      const bt = (d.positionSecs - d.firstBeatSecs) / beatLen(d);
      d.beatPhase = ((bt % 1) + 1) % 1;
    }
  });
  state.framesProcessed += Math.round(dt * state.sampleRate);
  state.callbackLoad = 0.14 + 0.05 * Math.sin(now / 900) + 0.02 * Math.random();

  let mL = 0, mR = 0;
  state.channels.forEach((ch, i) => {
    const d = state.decks[i];
    let l = 0, r = 0;
    if (d.playing) {
      const kick = Math.exp(-d.beatPhase * 4);
      const base = (0.35 + 0.55 * kick) * (0.6 + ch.trim * 0.8);
      l = base * (0.92 + 0.08 * Math.random());
      r = base * (0.9 + 0.1 * Math.random());
    }
    const prevL = meterHold[i * 2], prevR = meterHold[i * 2 + 1];
    ch.meterL = Math.max(l, prevL - dt * 1.6);
    ch.meterR = Math.max(r, prevR - dt * 1.6);
    meterHold[i * 2] = ch.meterL; meterHold[i * 2 + 1] = ch.meterR;
    const gain = ch.fader * (i === 0 ? Math.cos(state.master.crossfader * Math.PI / 2) : Math.sin(state.master.crossfader * Math.PI / 2));
    mL += l * gain; mR += r * gain;
  });
  mL *= state.master.level * 1.1; mR *= state.master.level * 1.1;
  state.master.meterL = Math.max(mL, state.master.meterL - dt * 1.4);
  state.master.meterR = Math.max(mR, state.master.meterR - dt * 1.4);
  state.master.clipping = state.master.meterL > 1.05 || state.master.meterR > 1.05;
  state.master.cueMeterL = state.channels.some((c) => c.pfl) ? state.master.meterL * 0.9 : 0;
  state.master.cueMeterR = state.master.cueMeterL;

  emit(EVENTS.engineState, structuredClone(state));
}

// ───────────────────────── library / settings ─────────────────────────

let playlists: (Playlist & { ids: number[] })[] = [
  { id: 1, name: 'Warm-up 21:00', trackCount: 0, parentId: 4, isFolder: false, ids: [1, 5, 8, 12] },
  { id: 2, name: 'Peak hour', trackCount: 0, parentId: 4, isFolder: false, ids: [3, 9, 13, 7] },
  { id: 3, name: 'Beer fest — closing', trackCount: 0, parentId: null, isFolder: false, ids: [4, 2, 10] },
  { id: 4, name: 'Beer fest', trackCount: 0, parentId: null, isFolder: true, ids: [] },
];
let nextPlaylistId = 5;

const analysisProgress = (trackId: number, st: AnalysisState, queued = 0): AnalysisProgress => ({ trackId, state: st, queued });

function finishAnalysis(id: number) {
  const t = tracks.find((x) => x.id === id);
  if (!t) return;
  t.analysis = 'running';
  emit(EVENTS.analysisProgress, analysisProgress(id, 'running', 0));
  setTimeout(() => {
    t.analysis = 'done';
    t.bpm = t.bpm ?? 126;
    t.key = t.key ?? '7A';
    t.keyName = t.keyName ?? 'Dm';
    emit(EVENTS.analysisProgress, analysisProgress(id, 'done', 0));
  }, 5000);
}

const midiLog: MidiLogEntry[] = [];
const midiStart = performance.now();
function logMidi(bytes: number[], summary: string) {
  midiLog.push({ timeMs: Math.round(performance.now() - midiStart), direction: 'injected', bytes, summary });
  if (midiLog.length > 400) midiLog.shift();
}

const controller: ControllerStatus = {
  connected: false, mapping: 'Pioneer DDJ-FLX4 (preview)', inputPort: null, outputPort: null,
  availableInputs: [], availableOutputs: [], messagesIn: 0, messagesOut: 0, lastError: null,
};

// ───────────────────────── FLX4 FX control (simulated controller) ─────────────────────────

const fxc: FxControlState = {
  shift: false, knobTarget: 'level', focus: 0, pickup: null, padModes: ['hotCue', 'hotCue'], held: [null, null], smartArming: [false, false], hud: null,
};
let hudSeq = 0;
let fxKnob: number | null = null;
const padHeld: (number | null)[] = [null, null];
const padRelease = [{ beats: 1 }, { beats: 1 }];
const tailTimer: (ReturnType<typeof setTimeout> | undefined)[] = [undefined, undefined];
const gate = [false, false];

function emitFxc(control?: FxHudControl, deck: number | null = null) {
  if (control) fxc.hud = { seq: ++hudSeq, control, deck };
  emit(EVENTS.controllerAction, { type: 'fxControl', ...structuredClone(fxc) } satisfies ControllerAppAction);
}

const knobbable = (kind: BeatFxKind) => BEAT_FX_PARAMS[kind].flatMap((p, i) => (p.options ? [] : [i]));
const isPercentKind = (kind: BeatFxKind) => (BEAT_FX_PERCENT_KINDS as readonly string[]).includes(kind);

function deckBeatSecs(deck: number) {
  const d = state.decks[deck];
  return 60 / (d.bpm > 0 ? d.bpm : 120);
}

function padFxCommand(deck: 0 | 1, slot: number, shift: boolean, pressed: boolean) {
  const pad = state.padFx[deck];
  const def = (shift ? PAD_FX1_SHIFT_BANK : PAD_FX1_BANK)[slot];
  if (!def) return;
  if (def.action.type === 'oneShot') {
    if (pressed) {
      Object.assign(pad, { release: def.action.release, releaseProgress: 0 });
      padRelease[deck].beats = def.action.beats;
    }
    return;
  }
  if (pressed) {
    clearTimeout(tailTimer[deck]);
    padHeld[deck] = slot;
    Object.assign(pad, { voice: { ...def.action.voice }, sounding: true });
  } else if (padHeld[deck] === slot) {
    padHeld[deck] = null;
    pad.voice = null;
    tailTimer[deck] = setTimeout(() => (pad.sounding = false), 700);
  }
}

function armSmart() {
  fxc.smartArming = state.smartCfx.enabled ? (state.channels.map((c) => Math.abs(c.filter) > 0.02) as [boolean, boolean]) : [false, false];
}

function tickFx(dt: number) {
  const r = state.releaseFx;
  if (r.running) {
    r.progress = Math.min(1, r.progress + dt / (r.beats * (60 / state.beatFx.bpm)));
    if (r.progress >= 1) Object.assign(r, { running: null, progress: 0 });
  }
  state.padFx.forEach((p, i) => {
    if (!p.release) return;
    p.releaseProgress = Math.min(1, p.releaseProgress + dt / (padRelease[i].beats * deckBeatSecs(i)));
    if (p.releaseProgress >= 1) Object.assign(p, { release: null, releaseProgress: 0 });
  });
}

function knobTarget(): { target: FxKnobTarget; deck: number | null; index: number | null; value: number } {
  const padDeck = state.padFx.findIndex((p) => p.voice !== null);
  if (padDeck >= 0 && fxc.held[padDeck]) return { target: 'padLevel', deck: padDeck, index: null, value: state.padFx[padDeck].voice!.level };
  if (fxc.shift && fxc.focus !== null) {
    const spec = BEAT_FX_PARAMS[state.beatFx.kind][fxc.focus];
    if (spec) return { target: 'param', deck: null, index: fxc.focus, value: ((state.beatFx.params[fxc.focus] ?? spec.default) - spec.min) / (spec.max - spec.min) };
  }
  return { target: 'level', deck: null, index: null, value: state.beatFx.level };
}

function retarget() {
  const t = knobTarget();
  fxc.knobTarget = t.target;
  fxc.pickup = fxKnob !== null && Math.abs(fxKnob - t.value) > 0.03 ? { ...t, knob: fxKnob } : null;
}

function applyKnob(v: number) {
  const prev = fxKnob;
  fxKnob = v;
  const t = knobTarget();
  if (fxc.pickup) {
    const crossed = prev !== null && (prev - t.value) * (v - t.value) <= 0;
    if (Math.abs(v - t.value) > 0.03 && !crossed) {
      fxc.pickup.knob = v;
      return emitFxc(t.target === 'level' ? 'level' : t.target === 'param' ? 'param' : 'level', t.deck);
    }
    fxc.pickup = null;
  }
  if (t.target === 'padLevel') state.padFx[t.deck!].voice!.level = v;
  else if (t.target === 'param') {
    const spec = BEAT_FX_PARAMS[state.beatFx.kind][t.index!];
    engineCommand({ type: 'beatFxParam', index: t.index!, value: Math.round(spec.min + v * (spec.max - spec.min)) });
  } else engineCommand({ type: 'beatFxLevel', value: v });
  emitFxc(t.target === 'param' ? 'param' : 'level', t.deck);
}

function stepBeats(dir: 1 | -1) {
  const heldDeck = state.padFx.findIndex((p) => p.voice !== null);
  if (heldDeck >= 0) {
    const v = state.padFx[heldDeck].voice!;
    const i = BEAT_FX_BEATS.findIndex((b) => b >= v.beats - 1e-6);
    v.beats = BEAT_FX_BEATS[Math.min(BEAT_FX_BEATS.length - 1, Math.max(0, i + dir))];
    return emitFxc('beats', heldDeck);
  }
  const b = state.beatFx;
  if (isPercentKind(b.kind)) engineCommand({ type: 'beatFxPercent', value: b.percent + dir * 10 });
  else {
    const options = BEAT_FX_KIND_BEATS[b.kind] ?? BEAT_FX_BEATS;
    const i = options.findIndex((o) => Math.abs(o - b.beats) < 1e-6);
    engineCommand({ type: 'beatFxBeats', beats: options[Math.min(options.length - 1, Math.max(0, i + dir))] });
  }
  emitFxc('beats');
}

function page(dir: 1 | -1) {
  const pages = knobbable(state.beatFx.kind);
  if (!pages.length) return;
  const i = Math.max(0, pages.indexOf(fxc.focus ?? pages[0]));
  fxc.focus = pages[(i + dir + pages.length) % pages.length];
  retarget();
  emitFxc('page');
}

const PAD_MODE_NOTES: Record<number, PadMode> = { 0x1b: 'hotCue', 0x1e: 'padFx1', 0x6b: 'padFx2', 0x20: 'beatJump', 0x6d: 'beatLoop', 0x22: 'sampler' };

function handleFxMidi(s: number, d1: number, d2: number) {
  const on = d2 > 0;
  if ((s === 0x90 || s === 0x91) && d1 === 0x3f) {
    fxc.shift = on;
    retarget();
    return emitFxc();
  }
  if ((s === 0x90 || s === 0x91) && on && PAD_MODE_NOTES[d1]) {
    const deck = s & 1;
    fxc.padModes[deck] = PAD_MODE_NOTES[d1];
    fxc.held[deck] = null;
    if (padHeld[deck] !== null) padFxCommand(deck as 0 | 1, padHeld[deck]!, false, false);
    return emitFxc('padMode', deck);
  }
  if (s >= 0x97 && s <= 0x9a) {
    const deck = s < 0x99 ? 0 : 1;
    const shift = s === 0x98 || s === 0x9a;
    const mode = fxc.padModes[deck];
    if (mode === 'padFx1' && d1 >= 0x10 && d1 < 0x18) {
      const pad = d1 - 0x10;
      if (on) fxc.held[deck] = { pad, shift, mode };
      else if (fxc.held[deck]?.pad === pad) fxc.held[deck] = null;
      engineCommand({ type: 'padFx', deck: deck as 0 | 1, slot: pad, shift, pressed: on });
      retarget();
      return emitFxc('padFx', deck);
    }
    if (mode === 'padFx2' && d1 >= 0x50 && d1 < 0x58) {
      const pad = d1 - 0x50;
      if (on) fxc.held[deck] = { pad, shift, mode };
      else if (fxc.held[deck]?.pad === pad) fxc.held[deck] = null;
      if (shift) {
        if (on) engineCommand({ type: 'beatFxKind', kind: PAD_FX2_FAVOURITES[pad] });
      } else if (on) {
        const b = state.beatFx;
        if (isPercentKind(b.kind)) engineCommand({ type: 'beatFxPercent', value: PAD_FX2_PERCENTS[pad] });
        else engineCommand({ type: 'beatFxBeats', beats: PAD_FX2_BEATS[pad] });
        gate[deck] = !b.enabled;
        if (gate[deck]) engineCommand({ type: 'beatFxEnabled', enabled: true });
      } else if (gate[deck]) {
        gate[deck] = false;
        engineCommand({ type: 'beatFxEnabled', enabled: false });
      }
      return emitFxc('padFx', deck);
    }
    return;
  }
  if (s === 0xb4 && d1 === 0x02) return applyKnob(d2 / 127);
  if (s === 0x94 && on) {
    if (d1 === 0x4a || d1 === 0x4b) return stepBeats(d1 === 0x4a ? -1 : 1);
    if (d1 === 0x66 || d1 === 0x6b) return page(d1 === 0x66 ? -1 : 1);
    if (d1 === 0x63 || d1 === 0x64) {
      const kinds = Object.keys(BEAT_FX_PARAMS) as BeatFxKind[];
      const i = kinds.indexOf(state.beatFx.kind);
      engineCommand({ type: 'beatFxKind', kind: kinds[(i + (d1 === 0x63 ? 1 : -1) + kinds.length) % kinds.length] });
      fxc.focus = knobbable(state.beatFx.kind)[0] ?? null;
      retarget();
      return emitFxc('kind');
    }
  }
  if ((s === 0x94 || s === 0x95) && on && d1 === 0x47) {
    engineCommand({ type: 'beatFxEnabled', enabled: !state.beatFx.enabled });
    return emitFxc('onOff');
  }
  if ((s === 0x94 || s === 0x95) && on && d1 === 0x43) {
    engineCommand({ type: 'releaseFx' });
    return emitFxc('release');
  }
  if (s === 0x96 && on && d1 === 0x00) {
    engineCommand({ type: 'smartCfx', enabled: !state.smartCfx.enabled });
    return emitFxc('smartCfx');
  }
  if (s === 0x96 && on && d1 === 0x08) {
    const i = SMART_CFX_PRESETS.indexOf(state.smartCfx.preset);
    engineCommand({ type: 'smartCfxPreset', preset: SMART_CFX_PRESETS[(i + 1) % SMART_CFX_PRESETS.length] });
    return emitFxc('smartPreset');
  }
  if (s === 0xb6 && (d1 === 0x17 || d1 === 0x18)) engineCommand({ type: 'filter', channel: d1 === 0x17 ? 0 : 1, value: (d2 / 127) * 2 - 1 });
}

/** Preview scenes for screenshots: `?fx3=pads|shift|level|smart`. */
function fxScene(name: string | null) {
  if (name) setTimeout(() => playFxScene(name), 700);
}

function playFxScene(name: string) {
  const midi = (...b: number[]) => handleMidi(b);
  Object.assign(state.beatFx, { enabled: true, kind: 'echo', beats: 0.5, level: 0.62, params: paramsFor('echo').map((p, i) => (i === 0 ? 62 : p)) });
  midi(0xb4, 0x02, Math.round(0.62 * 127));
  if (name === 'pads') {
    midi(0x90, 0x1e, 0x7f);
    midi(0x91, 0x6b, 0x7f);
    midi(0x97, 0x11, 0x7f);
    setInterval(() => emitFxc('padFx', 0), 900);
  } else if (name === 'shift') {
    midi(0x90, 0x1e, 0x7f);
    midi(0xb4, 0x02, 22);
    midi(0x90, 0x3f, 0x7f);
    midi(0x94, 0x6b, 0x7f);
    midi(0x94, 0x66, 0x7f);
  } else if (name === 'level') {
    midi(0x91, 0x6b, 0x7f);
    midi(0x90, 0x3f, 0x7f);
    midi(0xb4, 0x02, 30);
    midi(0x90, 0x3f, 0x00);
    setInterval(() => emitFxc('level'), 900);
  } else if (name === 'smart') {
    state.channels[1].filter = 0.45;
    midi(0x96, 0x00, 0x7f);
    midi(0x96, 0x08, 0x7f);
    const fire = () => {
      Object.assign(state.releaseFx, { kind: 'backspin', beats: 4 });
      engineCommand({ type: 'releaseFx' });
      state.releaseFx.progress = 0.45;
    };
    fire();
    setInterval(fire, 2600);
    setInterval(() => emitFxc('smartPreset'), 900);
  }
}

function handleMidi(b: number[]) {
  const [s, d1, d2] = b;
  const ch = s & 0x0f;
  const type = s & 0xf0;
  if (type === 0x90 && ch < 2 && d2 === 0x7f) {
    if (d1 === 0x0b) engineCommand({ type: 'togglePlay', deck: ch as 0 | 1 });
    if (d1 === 0x58) engineCommand({ type: 'sync', deck: ch as 0 | 1, enabled: !state.decks[ch].syncEnabled });
    if (d1 === 0x54) engineCommand({ type: 'pfl', channel: ch as 0 | 1, enabled: !state.channels[ch].pfl });
  }
  if (type === 0x90 && ch < 2 && d1 === 0x0c) engineCommand({ type: d2 ? 'cueDown' : 'cueUp', deck: ch as 0 | 1 });
  if ((s === 0x97 || s === 0x99) && d1 < 8 && fxc.padModes[s === 0x97 ? 0 : 1] === 'hotCue') engineCommand({ type: 'hotCue', deck: s === 0x97 ? 0 : 1, slot: d1, pressed: d2 > 0 });
  handleFxMidi(s, d1, d2);
  if (s === 0x96 && d2 === 0x7f && (d1 === 0x46 || d1 === 0x47)) emit(EVENTS.controllerAction, { type: 'loadSelected', deck: d1 === 0x46 ? 0 : 1 } satisfies ControllerAppAction);
  if (s === 0xb6 && d1 === 0x40) emit(EVENTS.controllerAction, { type: 'browseScroll', delta: d2 >= 0x40 ? d2 - 0x80 : d2 } satisfies ControllerAppAction);
  if (s === 0xb6 && d1 === 0x1f) engineCommand({ type: 'crossfader', value: (d2 << 7) / 16383 });
  if (type === 0xb0 && ch < 2 && d1 === 0x13) engineCommand({ type: 'fader', channel: ch as 0 | 1, value: (d2 << 7) / 16383 });
  if (type === 0xb0 && ch < 2 && d1 === 0x22) state.decks[ch].positionSecs += (d2 - 0x40) * 0.02;
}

const devices: OutputDeviceInfo[] = [
  { id: 'preview-builtin', name: 'Preview — MacBook Speakers (2 ch)', maxChannels: 2, sampleRates: [44100, 48000], isDefault: true },
  { id: 'preview-flx4', name: 'Preview — DDJ-FLX4 (4 ch)', maxChannels: 4, sampleRates: [44100, 48000, 96000], isDefault: false },
  { id: 'preview-hdmi', name: 'Preview — Studio Monitors (8 ch)', maxChannels: 8, sampleRates: [44100, 48000], isDefault: false },
];
let active: ActiveOutput = {
  deviceId: 'preview-flx4', deviceName: devices[1].name, sampleRate: 48000, channels: 4, requestedBuffer: 256,
  routing: { mode: 'master-and-phones', master: [0, 1], phones: [2, 3] }, sampleFormat: 'f32',
};
let streamCallbacks = 0;
let secondary: SecondaryOutputStatus | null = null;

function secondaryStatus(): SecondaryOutputStatus | null {
  if (!secondary) return null;
  const wobble = Math.sin(performance.now() / 4000);
  return { ...secondary, latencyMs: 24 + wobble * 0.6, driftPpm: 38 + wobble * 6 };
}

function outputStatus(): OutputStatus {
  return {
    state: 'running', active, lastError: null, callbacks: streamCallbacks, lastCallbackFrames: active.requestedBuffer ?? 256,
    streamErrors: 0, xruns: 0, secondary: secondaryStatus(),
  };
}

function filterTracks(query: string, playlistId: number | null): TrackSummary[] {
  let pool: TrackSummary[] = tracks;
  if (playlistId !== null) {
    const pl = playlists.find((p) => p.id === playlistId);
    pool = pl ? pl.ids.map((id) => tracks.find((t) => t.id === id)!).filter(Boolean) : [];
  }
  const q = query.trim().toLowerCase();
  return q ? pool.filter((t) => `${t.title} ${t.artist} ${t.genre} ${t.key ?? ''}`.toLowerCase().includes(q)) : pool;
}

function refreshCounts() {
  playlists.forEach((p) => (p.trackCount = p.ids.length));
}

let started = false;
function start() {
  if (started) return;
  started = true;
  refreshCounts();
  setInterval(tick, 1000 / 60);
  setInterval(() => {
    streamCallbacks += 190;
    emit(EVENTS.audioStatus, outputStatus());
    emit(EVENTS.controllerStatus, controller);
  }, 1000);
  setTimeout(() => finishAnalysis(tracks[tracks.length - 1].id), 7000);
}

function loadIntoDeck(deck: 0 | 1, t: MockTrack) {
  const d = emptyDeck();
  Object.assign(d, {
    loaded: true, trackToken: t.id, durationSecs: t.durationSecs, hasGrid: t.bpm !== null, gridBpm: t.bpm ?? 0,
    firstBeatSecs: t.firstBeat, bpm: t.bpm ?? 0, cueSecs: t.firstBeat, positionSecs: t.firstBeat, tempoRange: state.decks[deck].tempoRange,
    keyLock: state.decks[deck].keyLock,
  } satisfies Partial<DeckSnapshot>);
  state.decks[deck] = d;
  setMasterIfNeeded();
  recomputeSync();
}

// ───────────────────────── spotify ─────────────────────────

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

const spotifyState: SpotifyStatus = {
  clientId: null,
  connected: false,
  user: null,
  redirectUri: 'http://127.0.0.1:8888/callback',
  lastError: null,
};

const SPOTIFY_PLAYLISTS: SpotifyPlaylist[] = [
  { id: 'liked', name: 'Liked Songs', owner: 'You', trackCount: 24, imageUrl: null },
  { id: 'pl-beer', name: 'Beer fest 2026 — warm-up', owner: 'Preview DJ', trackCount: 18, imageUrl: null },
  { id: 'pl-peak', name: 'Peak hour weapons', owner: 'Preview DJ', trackCount: 22, imageUrl: null },
  { id: 'pl-sunday', name: 'Sunday sunset', owner: 'Preview DJ', trackCount: 16, imageUrl: null },
];

const SPOTIFY_ONLY: [string, string[]][] = [
  ['Midnight Overdrive', ['Lena Orsay']], ['Glasshouse', ['Tomas Wren', 'Idris']], ['Neon Understudy', ['Pale Harbour']],
  ['Slow Burn Paradox', ['Okafor']], ['Dust Devils', ['Marguerite']], ['Tidal Lock', ['Vesper & Co']],
];

function spotifyTracks(playlistId: string): SpotifyTrack[] {
  const rand = rng([...playlistId].reduce((a, c) => a + c.charCodeAt(0), 7));
  const out: SpotifyTrack[] = [];
  const count = Math.min(SPOTIFY_PLAYLISTS.find((p) => p.id === playlistId)?.trackCount ?? 14, 24);
  const pool = tracks.filter((t) => t.analysis === 'done');
  for (let i = 0; i < count; i++) {
    const matched = rand() < 0.62;
    if (matched) {
      const t = pool[Math.floor(rand() * pool.length)];
      out.push({
        id: `sp-${playlistId}-${i}`, name: t.title, artists: t.artist.split(' & '), album: t.album, durationMs: Math.round(t.durationSecs * 1000),
        isrc: null, imageUrl: null, localTrackId: t.id, matchScore: 0.88 + rand() * 0.12,
      });
    } else {
      const [name, artists] = SPOTIFY_ONLY[Math.floor(rand() * SPOTIFY_ONLY.length)];
      out.push({
        id: `sp-${playlistId}-${i}`, name, artists, album: 'Streaming Only', durationMs: Math.round((280 + rand() * 160) * 1000),
        isrc: null, imageUrl: null, localTrackId: null, matchScore: rand() < 0.3 ? 0.41 : null,
      });
    }
  }
  return out;
}

export const mockApi: typeof RealApi = {
  async engine(cmd) { start(); engineCommand(cmd); },
  async snapshot() { start(); return structuredClone(state); },
  async loadTrack(deck, trackId, force = false) {
    start();
    if (state.decks[deck].playing && !force) throw 'DECK_PLAYING';
    const t = tracks.find((x) => x.id === trackId);
    if (!t) throw 'track not found';
    loadIntoDeck(deck, t);
    return { deck, track: t };
  },
  async eject(deck) { state.decks[deck] = emptyDeck(); setMasterIfNeeded(); recomputeSync(); },
  async phrases(trackId) {
    const t = tracks.find((x) => x.id === trackId);
    return t ? phrasesFor(t) : [];
  },
  async tracks(query = '', playlistId = null) { start(); return structuredClone(filterTracks(query, playlistId)); },
  async track(id) { return structuredClone(tracks.find((t) => t.id === id)!); },
  async importPaths() { return { added: 0, skipped: 0, failed: 0 }; },
  async importPlaylists() { return { added: 0, skipped: 0, failed: 0, playlists: [] }; },
  async playlists() { refreshCounts(); return playlists.map(({ ids: _ids, ...p }) => ({ ...p })); },
  async createPlaylist(name, parentId = null, isFolder = false) {
    const p = { id: nextPlaylistId++, name, trackCount: 0, parentId, isFolder, ids: [] as number[] };
    playlists.push(p);
    emit(EVENTS.libraryChanged, null);
    return { id: p.id, name: p.name, trackCount: 0, parentId, isFolder };
  },
  async renamePlaylist(id, name) { const p = playlists.find((x) => x.id === id); if (p) p.name = name; emit(EVENTS.libraryChanged, null); },
  async deletePlaylist(id) { playlists = playlists.filter((p) => p.id !== id && p.parentId !== id); emit(EVENTS.libraryChanged, null); },
  async addToPlaylist(id, trackIds) {
    const p = playlists.find((x) => x.id === id);
    if (p && !p.isFolder) trackIds.forEach((t) => !p.ids.includes(t) && p.ids.push(t));
    emit(EVENTS.libraryChanged, null);
  },
  async removeFromPlaylist(id, trackId) {
    const p = playlists.find((x) => x.id === id);
    if (p) p.ids = p.ids.filter((t) => t !== trackId);
    emit(EVENTS.libraryChanged, null);
  },
  async reanalyze(trackId) { waveformCache.delete(trackId); finishAnalysis(trackId); },
  async setBeatgrid(trackId, bpm, firstBeatSecs) {
    const t = tracks.find((x) => x.id === trackId);
    if (t) { t.bpm = bpm; t.firstBeat = firstBeatSecs; }
  },
  async waveform(trackId) {
    const t = tracks.find((x) => x.id === trackId);
    if (!t || t.analysis === 'pending' || t.analysis === 'running') return null;
    return waveformFor(t);
  },
  async artwork(trackId) {
    const t = tracks.find((x) => x.id === trackId);
    return t ? artworkFor(t) : null;
  },
  async spotifyStatus() { return structuredClone(spotifyState); },
  async spotifySetClientId(clientId) {
    spotifyState.clientId = clientId.trim() || null;
    return structuredClone(spotifyState);
  },
  async spotifyLogin() {
    await sleep(1600);
    spotifyState.connected = true;
    spotifyState.user = { id: 'preview', displayName: 'Preview DJ', product: 'premium', imageUrl: null };
    return structuredClone(spotifyState);
  },
  async spotifyLogout() {
    spotifyState.connected = false;
    spotifyState.user = null;
    return structuredClone(spotifyState);
  },
  async spotifyPlaylists() {
    await sleep(250);
    return structuredClone(SPOTIFY_PLAYLISTS);
  },
  async spotifyPlaylistTracks(playlistId) {
    await sleep(350);
    return spotifyTracks(playlistId);
  },
  async spotifyImportPlaylist(playlistId) {
    await sleep(600);
    const pl = SPOTIFY_PLAYLISTS.find((p) => p.id === playlistId);
    const list = spotifyTracks(playlistId);
    const ids = [...new Set(list.flatMap((t) => (t.localTrackId ? [t.localTrackId] : [])))];
    const created = { id: nextPlaylistId++, name: `${pl?.name ?? 'Spotify import'} (Spotify)`, trackCount: ids.length, parentId: null, isFolder: false, ids };
    playlists.push(created);
    emit(EVENTS.libraryChanged, null);
    return { playlist: { id: created.id, name: created.name, trackCount: ids.length, parentId: null, isFolder: false }, matched: ids.length, unmatched: list.length - list.filter((t) => t.localTrackId).length };
  },
  async audioDevices() { return devices; },
  async audioOpen(req: OutputRequest) {
    const dev = devices.find((d) => d.id === req.deviceId) ?? devices[0];
    active = {
      deviceId: dev.id, deviceName: dev.name, sampleRate: req.sampleRate ?? 48000, channels: dev.maxChannels,
      requestedBuffer: req.bufferFrames ?? 256, routing: req.routing ?? active.routing, sampleFormat: 'f32',
    };
    state.cueAvailable = dev.maxChannels >= 4 && active.routing.mode === 'master-and-phones';
    state.sampleRate = active.sampleRate;
    return active;
  },
  async audioStatus() { return outputStatus(); },
  async audioSetRouting(routing: Routing) { active = { ...active, routing }; },
  async audioSetSecondary(request: SecondaryOutputRequest | null) {
    if (request) {
      if (request.deviceId === active.deviceId) throw 'that device is already the main output; pick a different one for the additional output';
      const dev = devices.find((d) => d.id === request.deviceId);
      if (!dev) throw `output device not found: ${request.deviceId}`;
      secondary = {
        deviceId: dev.id, deviceName: dev.name.replace('Preview — ', '').replace(/ \(\d+ ch\)$/, ''), source: request.source,
        level: request.level, state: 'running', sampleRate: 44100, latencyMs: 24, driftPpm: 38, underruns: 0, channels: request.channels,
      };
    } else {
      secondary = null;
    }
    emit(EVENTS.audioStatus, outputStatus());
    return outputStatus();
  },
  async audioSetSecondaryLevel(level: number) { if (secondary) secondary = { ...secondary, level }; },
  async controllerStatus() { return controller; },
  async midiLog() { return midiLog.slice(); },
  async midiClearLog() { midiLog.length = 0; },
  async midiInject(bytes) {
    controller.messagesIn++;
    logMidi(bytes, `preview inject ${bytes.map((x) => x.toString(16)).join(' ')}`);
    handleMidi(bytes);
  },
};

function placeInPhase(deck: 0 | 1, ref: 0 | 1, offsetBeats: number) {
  const beats = (d: DeckSnapshot) => ((d.positionSecs - d.firstBeatSecs) * d.gridBpm) / 60;
  const a = state.decks[ref];
  const b = state.decks[deck];
  const diff = beats(a) + offsetBeats - beats(b);
  const wrapped = ((((diff + 8) % 16) + 16) % 16) - 8;
  b.positionSecs = b.firstBeatSecs + ((beats(b) + wrapped) * 60) / b.gridBpm;
}

// Initial content so the preview looks alive immediately.
queueMicrotask(() => {
  start();
  loadIntoDeck(0, tracks[0]);
  loadIntoDeck(1, tracks[1]);
  state.decks[0].positionSecs = 62.4;
  state.decks[1].positionSecs = 118.7;
  state.decks[0].hotCues[0] = { positionSecs: 0.21, color: 0x1ee3ff };
  state.decks[0].hotCues[1] = { positionSecs: 30.4, color: 0xff4aa8 };
  state.decks[0].hotCues[2] = { positionSecs: 92.5, color: 0xffc247 };
  state.decks[1].hotCues[0] = { positionSecs: 0.34, color: 0x3ee8a2 };
  state.decks[1].hotCues[3] = { positionSecs: 150, color: 0xa06bff };
  state.decks[1].loopInSecs = 125.4;
  state.decks[1].loopOutSecs = 133;
  state.decks[0].playing = true;
  state.decks[1].playing = true;
  state.decks[1].syncEnabled = true;
  state.masterDeck = 0;
  state.decks[0].tempoRatio = state.decks[0].faderTempoRatio = 1.018;
  const offPhase = new URLSearchParams(location.search).get('phase') === 'off';
  placeInPhase(1, 0, offPhase ? 0.12 : 0);
  state.decks[0].cueSecs = 0.21;
  state.channels[0].fader = 0.92;
  state.channels[1].fader = 0.74;
  state.master.crossfader = 0.5;

  const scene = new URLSearchParams(location.search);
  if (scene.get('fx') === 'on') {
    Object.assign(state.beatFx, { enabled: true, kind: 'pingPong', beats: 0.5, level: 0.62, target: 'master' });
  }
  fxScene(scene.get('fx3'));
  if (scene.get('spotify') === 'connected') {
    spotifyState.clientId = 'preview-client-id';
    spotifyState.connected = true;
    spotifyState.user = { id: 'preview', displayName: 'Preview DJ', product: 'premium', imageUrl: null };
  }
});
