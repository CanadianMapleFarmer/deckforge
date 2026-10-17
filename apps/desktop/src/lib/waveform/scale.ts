/** Slowest tempo the scrolling waveforms scale for; matches the drawing in `draw.ts`. */
export const MIN_DRAWN_TEMPO = 0.2;

/** Track seconds under one CSS pixel of a scrolling waveform drawn at `pxPerSec` and the deck's tempo. */
export function trackSecsPerPx(pxPerSec: number, tempoRatio: number): number {
  return Math.max(MIN_DRAWN_TEMPO, tempoRatio) / pxPerSec;
}
