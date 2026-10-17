import { clamp } from '$lib/util/format';
import { engine } from '$lib/state/engine.svelte';

type FrameFn = (now: number, dt: number) => void;
const subs: FrameFn[] = [];
let running = false;
let prev = 0;
export let frameCount = 0;
export const frameErrors: string[] = [];

/** Runs every subscriber once outside rAF (self-test when the window is occluded). */
export function stepOnce(now = performance.now()): void {
  runSubscribers(now);
}

function loop(now: number) {
  requestAnimationFrame(loop);
  runSubscribers(now);
}

function runSubscribers(now: number) {
  const dt = Math.min(0.05, Math.max(0, (now - prev) / 1000));
  prev = now;
  frameCount++;
  for (const fn of subs) {
    try {
      fn(now, dt);
    } catch (e) {
      if (frameErrors.length < 20) {
        frameErrors.push(e instanceof Error ? `${e.message} @ ${e.stack?.split('\n')[1]?.trim() ?? ''}` : String(e));
        console.error(e);
      }
    }
  }
}

/** Single shared rAF loop; subscribers run in registration order. Returns an unsubscribe fn. */
export function onFrame(fn: FrameFn): () => void {
  subs.push(fn);
  if (!running) {
    running = true;
    prev = performance.now();
    requestAnimationFrame(loop);
  }
  return () => {
    const i = subs.indexOf(fn);
    if (i >= 0) subs.splice(i, 1);
  };
}

/** Smoothed per-deck playhead extrapolated from the latest engine snapshot. */
export class DeckClock {
  pos = 0;
  token = -1;
  hold: number | null = null;
  constructor(readonly deck: 0 | 1) {}

  update(now: number, dt: number) {
    const d = engine.latest.decks[this.deck];
    if (this.hold !== null && d.loaded) {
      this.pos = clamp(this.hold, 0, d.durationSecs);
      this.token = d.trackToken;
      return;
    }
    if (!d.loaded) {
      this.pos = 0;
      this.token = 0;
      return;
    }
    const moving = d.playing || d.previewing || d.jogTouched;
    const age = clamp((now - engine.snapAt) / 1000 - engine.latest.outputLatencySecs, -0.25, 0.12);
    const target = clamp(d.positionSecs + (moving ? age * d.playRate : 0), 0, d.durationSecs);
    if (d.trackToken !== this.token || !moving || Math.abs(target - this.pos) > 0.2) {
      this.pos = target;
      this.token = d.trackToken;
      return;
    }
    this.pos += dt * d.playRate;
    this.pos += (target - this.pos) * 0.25;
  }
}

export const clocks: [DeckClock, DeckClock] = [new DeckClock(0), new DeckClock(1)];
onFrame((now, dt) => {
  clocks[0].update(now, dt);
  clocks[1].update(now, dt);
});
