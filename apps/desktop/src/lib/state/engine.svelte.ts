import { EVENTS, type ControllerStatus, type DeckIndex, type EngineSnapshot, type OutputStatus, type UiCommand } from '$lib/api';
import * as be from '$lib/api/backend';
import { errorMessage } from '$lib/util/format';
import { toast } from './toast.svelte';

function emptyDeck(): EngineSnapshot['decks'][0] {
  return {
    loaded: false, trackToken: 0, playing: false, positionSecs: 0, durationSecs: 0, tempoRatio: 1, playRate: 0, faderTempoRatio: 1,
    tempoRange: 'Ten', keyLock: false, vinylMode: false, hasGrid: false, gridBpm: 0, firstBeatSecs: 0, bpm: 0, beatPhase: 0,
    cueSecs: 0, hotCues: Array(8).fill(null), loopInSecs: null, loopOutSecs: null, loopActive: false, syncEnabled: false,
    isMaster: false, jogTouched: false, previewing: false, atEnd: false,
  };
}
const emptyChannel = () => ({ trim: 0.5, eqHigh: 0.5, eqMid: 0.5, eqLow: 0.5, filter: 0, fader: 0, pfl: false, meterL: 0, meterR: 0 });

export const EMPTY_SNAPSHOT: EngineSnapshot = {
  decks: [emptyDeck(), emptyDeck()],
  channels: [emptyChannel(), emptyChannel()],
  master: {
    crossfader: 0.5, curve: 'equal-power', eqCurve: 'eq', level: 0.8, headphoneMix: 0.5, headphoneLevel: 0.5, meterL: 0, meterR: 0,
    cueMeterL: 0, cueMeterR: 0, clipping: false, limiting: false, hardwarePhones: false,
  },
  quantize: true, masterDeck: null, sampleRate: 48000, framesProcessed: 0, outputLatencySecs: 0, callbackLoad: 0, cueAvailable: true,
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

class EngineStore {
  /** Latest engine snapshot; replaced wholesale per event. Read untracked from rAF loops via `latest`. */
  snap = $state.raw<EngineSnapshot>(EMPTY_SNAPSHOT);
  /** `performance.now()` at which `snap` arrived. */
  snapAt = 0;
  live = $state(false);
  audio = $state.raw<OutputStatus | null>(null);
  controller = $state.raw<ControllerStatus | null>(null);
  preview = $state(false);
  clipLatch = $state(false);
  /** Last positions reported by controller knobs that the hardware handles itself. */
  hardware = $state<Partial<Record<'masterLevel' | 'headphoneMix' | 'headphoneLevel', number>>>({});
  private clipTimer: ReturnType<typeof setTimeout> | undefined;

  get latest(): EngineSnapshot {
    return this.snap;
  }

  private ingest(s: EngineSnapshot) {
    this.snap = s;
    this.snapAt = performance.now();
    this.live = true;
    if (s.master.clipping) {
      this.clipLatch = true;
      clearTimeout(this.clipTimer);
      this.clipTimer = setTimeout(() => (this.clipLatch = false), 1500);
    }
  }

  async init(): Promise<() => void> {
    await be.initBackend();
    this.preview = !be.inTauri;
    const offs = await Promise.all([
      be.on<EngineSnapshot>(EVENTS.engineState, (s) => this.ingest(s)),
      be.on<OutputStatus>(EVENTS.audioStatus, (s) => (this.audio = s)),
      be.on<ControllerStatus>(EVENTS.controllerStatus, (s) => (this.controller = s)),
    ]);
    be.backend.snapshot().then((s) => this.ingest(s)).catch((e) => console.warn('snapshot', e));
    be.backend.audioStatus().then((s) => (this.audio = s)).catch(() => {});
    be.backend.controllerStatus().then((s) => (this.controller = s)).catch(() => {});
    return () => offs.forEach((o) => o());
  }
}

export const engine = new EngineStore();

export function cmd(c: UiCommand): void {
  be.backend.engine(c).catch((e) => toast.error(`${c.type}: ${errorMessage(e)}`));
}

export const deckCmd = (deck: DeckIndex) => ({
  togglePlay: () => cmd({ type: 'togglePlay', deck }),
  cue: (down: boolean) => cmd({ type: down ? 'cueDown' : 'cueUp', deck }),
  sync: (enabled: boolean) => cmd({ type: 'sync', deck, enabled }),
});
