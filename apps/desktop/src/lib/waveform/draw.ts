import { MIN_DRAWN_TEMPO } from './scale';
import { engine } from '$lib/state/engine.svelte';
import { library } from '$lib/state/library.svelte';
import { clocks } from '$lib/ticker';
import { hexColor } from '$lib/util/format';
import { PHRASE_STYLE } from './phrases';
import { deckGrid, deckPhrases } from './beats';
import { gridLines, type BarGrid } from './bars';
import { barSecs, classify, phraseBars, phraseIndexAt, wallSecsToNext, type BarLevel } from './phraseNav';
import { makeGradients, renderColumns } from './render';
import { TILE_PX, TileCache } from './tiles';
import { waveforms } from './store';
import type { DeckSnapshot, Phrase, PhraseKind } from '$lib/api';

export const DECK_RGB = [
  [56, 189, 248],
  [251, 113, 133],
] as const;
const LOOP_RGB = [163, 230, 53] as const;
const CUE_COLOR = '#f59e0b';
const FONT = '"Geist Variable", system-ui, sans-serif';

function rgba(rgb: readonly number[], a: number) {
  return `rgba(${rgb[0]},${rgb[1]},${rgb[2]},${a})`;
}

const phrasesFor = deckPhrases;

function placeholder(ctx: CanvasRenderingContext2D, W: number, H: number, dpr: number, text: string, shimmer: boolean, now: number) {
  if (shimmer) {
    const t = (now / 1400) % 1;
    const g = ctx.createLinearGradient(W * (t * 2 - 0.6), 0, W * (t * 2 - 0.1), 0);
    g.addColorStop(0, 'rgba(255,255,255,0)');
    g.addColorStop(0.5, 'rgba(255,255,255,0.06)');
    g.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, W, H);
    ctx.strokeStyle = 'rgba(161,161,170,0.2)';
    ctx.lineWidth = dpr;
    ctx.beginPath();
    ctx.moveTo(0, H / 2);
    ctx.lineTo(W, H / 2);
    ctx.stroke();
  }
  ctx.fillStyle = shimmer ? 'rgba(212,212,216,0.9)' : 'rgba(113,113,122,0.9)';
  ctx.font = `500 ${12 * dpr}px ${FONT}`;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(text, W / 2, H / 2);
}

function markers(ctx: CanvasRenderingContext2D, d: DeckSnapshot, dpr: number, H: number, xOf: (t: number) => number, W: number, compact: boolean, top = 0) {
  const fh = (compact ? 8 : 14) * dpr;
  d.hotCues.forEach((c, i) => {
    if (!c) return;
    const x = Math.round(xOf(c.positionSecs));
    if (x < -30 * dpr || x > W + 30 * dpr) return;
    const col = hexColor(c.color);
    ctx.fillStyle = col;
    if (!compact) {
      ctx.globalAlpha = 0.7;
      ctx.fillRect(x, top, dpr, H - top);
      ctx.globalAlpha = 1;
    } else {
      ctx.globalAlpha = 0.55;
      ctx.fillRect(x, top, dpr, H - top);
      ctx.globalAlpha = 1;
    }
    const w = (compact ? 7 : 15) * dpr;
    ctx.fillRect(x, top, w, fh);
    if (!compact) {
      ctx.fillStyle = '#09090b';
      ctx.font = `700 ${9.5 * dpr}px ${FONT}`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(String(i + 1), x + w / 2, top + fh / 2 + dpr * 0.5);
    }
  });
  if (d.loaded) {
    const x = Math.round(xOf(d.cueSecs));
    if (x > -30 * dpr && x < W + 30 * dpr) {
      ctx.fillStyle = CUE_COLOR;
      if (!compact) {
        ctx.globalAlpha = 0.85;
        ctx.fillRect(x, top, dpr, H - top);
        ctx.globalAlpha = 1;
      }
      const s = (compact ? 4 : 7) * dpr;
      const y = compact ? H : top;
      const dir = compact ? -1 : 1;
      ctx.beginPath();
      ctx.moveTo(x - s, y);
      ctx.lineTo(x + s, y);
      ctx.lineTo(x, y + dir * s * 1.4);
      ctx.closePath();
      ctx.fill();
    }
  }
}

const BAND_CSS = 16;
const INK = '#09090b';
const GRID = {
  beat: 'rgba(250,250,250,0.3)',
  beatFaint: 'rgba(250,250,250,0.09)',
  bar: 'rgba(250,250,250,0.5)',
  four: 'rgba(250,250,250,0.78)',
  shadow: 'rgba(9,9,11,0.6)',
  marker: 'rgba(250,250,250,0.72)',
  label: 'rgba(212,212,216,0.6)',
  aligned: '#4ade80',
};

export const bandHeight = (hasPhrases: boolean, dpr: number) => (hasPhrases ? Math.round(BAND_CSS * dpr) : 0);

function withAlpha(color: string, a: number) {
  if (color.startsWith('#') && color.length === 7) {
    const n = parseInt(color.slice(1), 16);
    return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
  }
  return color;
}

const hatchCache = new Map<string, CanvasPattern>();
function hatch(ctx: CanvasRenderingContext2D, color: string, period: number, anchorX: number): CanvasPattern | string {
  const key = `${color}:${period}`;
  let p = hatchCache.get(key);
  if (!p) {
    const c = document.createElement('canvas');
    c.width = c.height = Math.max(2, Math.round(period));
    const g = c.getContext('2d')!;
    g.strokeStyle = color;
    g.lineWidth = period * 0.44 * Math.SQRT1_2;
    for (const o of [-period, 0, period]) {
      g.beginPath();
      g.moveTo(o, period);
      g.lineTo(o + period, 0);
      g.stroke();
    }
    p = ctx.createPattern(c, 'repeat') ?? undefined;
    if (!p) return color;
    hatchCache.set(key, p);
  }
  p.setTransform(new DOMMatrix().translate(anchorX, 0));
  return p;
}

function setFont(ctx: CanvasRenderingContext2D, size: number, tracking = 0) {
  ctx.font = `600 ${size}px ${FONT}`;
  (ctx as CanvasRenderingContext2D & { letterSpacing?: string }).letterSpacing = `${tracking * size}px`;
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, [0, r, r, 0]);
  ctx.fill();
}

function triangle(ctx: CanvasRenderingContext2D, cx: number, y: number, w: number, h: number, down: boolean) {
  ctx.beginPath();
  ctx.moveTo(cx - w / 2, y + (down ? 0 : h));
  ctx.lineTo(cx + w / 2, y + (down ? 0 : h));
  ctx.lineTo(cx, y + (down ? h : 0));
  ctx.closePath();
  ctx.fill();
}

function fillMarker(ctx: CanvasRenderingContext2D, x: number, end: number, band: number, dpr: number, color: string) {
  ctx.fillStyle = color;
  ctx.fillRect(x, 0, end - x, band);
  ctx.fillStyle = '#fafafa';
  ctx.fillRect(x, 0, Math.round(1.5 * dpr), band);
  triangle(ctx, x + 0.75 * dpr, band, 4 * dpr, 4 * dpr, true);
  const size = 8.5 * dpr;
  setFont(ctx, size, 0.06);
  if (end - x >= ctx.measureText('FILL').width + 10 * dpr) {
    ctx.fillStyle = 'rgba(9,9,11,0.85)';
    ctx.textAlign = 'left';
    ctx.textBaseline = 'alphabetic';
    ctx.fillText('FILL', x + 5 * dpr, Math.round(band / 2 + 3 * dpr));
  }
}

const LEVEL_WIDTH: Record<BarLevel, number> = { phrase: 3, sixteen: 2, eight: 1.5, four: 1.5, bar: 1 };
const LEVEL_SPAN: Record<BarLevel, number> = { phrase: 0, sixteen: 16, eight: 8, four: 4, bar: 1 };

export class ScrollerRenderer {
  private tiles = new TileCache();
  private frame = 0;
  constructor(readonly deck: 0 | 1) {}

  resetCache() {
    this.tiles.clear();
  }

  drawBase(ctx: CanvasRenderingContext2D, W: number, H: number, dpr: number, ppsCss: number, now: number) {
    this.frame++;
    const d = engine.latest.decks[this.deck];
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, W, H);
    if (!d.loaded) {
      placeholder(ctx, W, H, dpr, `Deck ${this.deck === 0 ? 'A' : 'B'} is empty — drag a track here or press ${this.deck === 0 ? '←' : '→'}`, false, now);
      return;
    }
    const pos = clocks[this.deck].pos;
    const ppsDev = ppsCss * dpr;
    const cx = Math.floor(W / 2);
    const scale = 1 / Math.max(MIN_DRAWN_TEMPO, d.tempoRatio);
    const wf = waveforms.get(d.trackToken);
    if (!wf) {
      placeholder(ctx, W, H, dpr, 'Analysing…', true, now);
      return;
    }
    const top = bandHeight(phrasesFor(this.deck, d.trackToken).length > 0, dpr);
    const waveH = H - top;
    const version = waveforms.version(d.trackToken);
    const centre = pos * ppsDev;
    const first = Math.max(0, Math.floor((centre - cx / scale) / TILE_PX));
    const last = Math.floor((centre + (W - cx) / scale) / TILE_PX);
    const maxIdx = Math.ceil((d.durationSecs * ppsDev) / TILE_PX);
    const screenX = (px: number) => Math.round(cx + (px - centre) * scale);
    for (let i = first; i <= last && i <= maxIdx; i++) {
      const tile = this.tiles.get(d.trackToken, version, wf, ppsDev, waveH, i, this.frame);
      const x0 = screenX(i * TILE_PX);
      ctx.drawImage(tile, 0, 0, TILE_PX, waveH, x0, top, screenX((i + 1) * TILE_PX) - x0, waveH);
    }
    const pre = [last + 1, first - 1].find((i) => i >= 0 && i <= maxIdx && !this.tiles.has(d.trackToken, version, ppsDev, waveH, i));
    if (pre !== undefined) this.tiles.get(d.trackToken, version, wf, ppsDev, waveH, pre, this.frame);
  }

  drawOverlay(ctx: CanvasRenderingContext2D, W: number, H: number, dpr: number, ppsCss: number) {
    const snap = engine.latest;
    const d = snap.decks[this.deck];
    const rgb = DECK_RGB[this.deck];
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, W, H);
    if (!d.loaded) return;
    const pos = clocks[this.deck].pos;
    const ratio = Math.max(MIN_DRAWN_TEMPO, d.tempoRatio);
    const pps = (ppsCss * dpr) / ratio;
    const cx = Math.floor(W / 2);
    const xOf = (t: number) => cx + (t - pos) * pps;
    const phrases = phrasesFor(this.deck, d.trackToken);
    const band = bandHeight(phrases.length > 0, dpr);
    const grid = d.hasGrid && d.gridBpm > 0 ? deckGrid(d) : null;

    ctx.fillStyle = 'rgba(9,9,11,0.46)';
    ctx.fillRect(0, 0, cx, H);

    if (band > 0) this.drawBand(ctx, phrases, grid, W, H, dpr, pos, xOf, band);

    if (d.loopInSecs !== null && d.loopOutSecs !== null) {
      const x1 = xOf(d.loopInSecs);
      const x2 = xOf(d.loopOutSecs);
      ctx.fillStyle = rgba(LOOP_RGB, d.loopActive ? 0.16 : 0.05);
      ctx.fillRect(x1, band, x2 - x1, H - band);
      ctx.fillStyle = rgba(LOOP_RGB, d.loopActive ? 0.9 : 0.35);
      ctx.fillRect(x1, band, 2 * dpr, H - band);
      ctx.fillRect(x2 - 2 * dpr, band, 2 * dpr, H - band);
      ctx.fillRect(x1, band, x2 - x1, 2 * dpr);
      ctx.fillRect(x1, H - 2 * dpr, x2 - x1, 2 * dpr);
    }

    if (grid) this.drawGrid(ctx, phrases, grid, W, H, dpr, pps, pos, cx, rgb, band);

    const other = (1 - this.deck) as 0 | 1;
    const od = snap.decks[other];
    if (snap.masterDeck === this.deck && od.loaded) {
      const theirs = wallSecsToNext(phrasesFor(other, od.trackToken), clocks[other].pos, Math.max(0.05, od.tempoRatio));
      if (theirs) {
        const ours = wallSecsToNext(phrases, pos, ratio);
        const aligned = !!ours && !!grid && Math.abs(ours.secs - theirs.secs) < (0.25 * 60) / (grid.bpm * ratio);
        const kind = phrasesFor(other, od.trackToken)[theirs.next].kind;
        this.drawGhost(ctx, cx + theirs.secs * ppsCss * dpr, H, dpr, band, other, kind, aligned);
      }
    }

    markers(ctx, d, dpr, H, xOf, W, false, band);

    ctx.fillStyle = '#fafafa';
    ctx.fillRect(cx - dpr, band, 2 * dpr, H - band);
    ctx.fillStyle = rgba(rgb, 1);
    const s = 6 * dpr;
    triangle(ctx, cx, band, 2 * s, s * 1.3, true);
    triangle(ctx, cx, H - s * 1.3, 2 * s, s * 1.3, false);
  }

  private drawBand(ctx: CanvasRenderingContext2D, phrases: Phrase[], grid: BarGrid | null, W: number, H: number, dpr: number, pos: number, xOf: (t: number) => number, band: number) {
    const edge = 1.5 * dpr;
    const bar = grid ? barSecs(grid) : 0;
    const dim = (p: Phrase, a: number) => (p.endSecs <= pos ? a * 0.55 : a);
    phrases.forEach((p, i) => {
      const x1 = xOf(p.startSecs) - edge;
      const x2 = xOf(p.endSecs) - edge;
      if (x2 < 0 || x1 > W) return;
      const color = PHRASE_STYLE[p.kind].color;
      ctx.fillStyle = withAlpha(color, dim(p, 0.17));
      ctx.fillRect(x1, 0, x2 - x1 - dpr, band);
      ctx.fillStyle = withAlpha(color, dim(p, 0.75));
      ctx.fillRect(x1, band - 2 * dpr, x2 - x1 - dpr, 2 * dpr);
      if (!grid) return;
      const len = phraseBars(p, grid);
      if (!len.whole || len.bars % 4 !== 0) {
        const tx = xOf(Math.min(p.endSecs, p.startSecs + Math.floor(len.bars / 4) * 4 * bar)) - edge;
        ctx.fillStyle = hatch(ctx, withAlpha(color, 0.5), 6 * dpr, xOf(p.startSecs));
        ctx.fillRect(tx, 0, x2 - tx - dpr, band);
      }
      if (i > 0) {
        const prev = phrases[i - 1];
        const run = xOf(Math.max(p.startSecs - bar, prev.startSecs)) - edge;
        const f = prev.fillStartSecs;
        const fill = f !== null && f !== undefined && f >= prev.startSecs && f < p.startSecs ? xOf(f) - edge : null;
        ctx.fillStyle = withAlpha(color, dim(p, 0.38));
        ctx.fillRect(run, 0, x1 - run, band);
        const from = fill === null ? run : Math.min(fill, run);
        ctx.fillStyle = hatch(ctx, withAlpha(color, dim(p, fill === null ? 0.11 : 0.16)), 6 * dpr, x1);
        ctx.fillRect(from, band, x1 - from, H - band);
        if (fill !== null) fillMarker(ctx, fill, x1, band, dpr, withAlpha(color, dim(p, 0.72)));
      }
    });

    const size = 10 * dpr;
    const pad = 6 * dpr;
    const gap = 4 * dpr;
    const baseline = Math.round(band / 2 + 3.6 * dpr);
    const current = phraseIndexAt(phrases, pos);
    ctx.textAlign = 'left';
    ctx.textBaseline = 'alphabetic';
    phrases.forEach((p, i) => {
      const start = xOf(p.startSecs) - edge;
      const next = (phrases[i + 1] ? xOf(phrases[i + 1].startSecs) : xOf(p.endSecs)) - edge;
      if (next < 0 || start > W) return;
      const style = PHRASE_STYLE[p.kind];
      const num = grid ? String(phraseBars(p, grid).bars) : '';
      setFont(ctx, size, 0.06);
      const nameW = ctx.measureText(style.name).width;
      setFont(ctx, size);
      const numW = ctx.measureText(num).width;
      const full = pad + nameW + (num ? gap + numW : 0) + pad;
      const compact = pad + Math.max(numW, ctx.measureText('D').width) + pad;
      const room = next - start - 2 * dpr;
      let tabW: number;
      let showName: boolean;
      if (room >= full || i === phrases.length - 1) [tabW, showName] = [full, true];
      else if (room >= compact) [tabW, showName] = [compact, false];
      else return;
      const x = Math.min(Math.max(start, 0), next - tabW - 2 * dpr);
      ctx.globalAlpha = p.endSecs <= pos && current !== i ? 0.55 : 1;
      ctx.fillStyle = style.color;
      roundRect(ctx, x, 0, tabW, band, 3 * dpr);
      ctx.globalAlpha = 1;
      ctx.fillStyle = INK;
      if (showName) {
        setFont(ctx, size, 0.06);
        ctx.fillText(style.name, x + pad, baseline);
        setFont(ctx, size);
        ctx.fillStyle = 'rgba(9,9,11,0.62)';
        ctx.fillText(num, x + pad + nameW + gap, baseline);
      } else {
        setFont(ctx, size);
        ctx.fillText(num || style.name[0], x + pad, baseline);
      }
      if (current === i) {
        const progress = Math.min(1, Math.max(0, (pos - p.startSecs) / Math.max(1e-9, p.endSecs - p.startSecs)));
        ctx.fillStyle = 'rgba(9,9,11,0.18)';
        ctx.fillRect(x, band - 2 * dpr, tabW, 2 * dpr);
        ctx.fillStyle = 'rgba(9,9,11,0.7)';
        ctx.fillRect(x, band - 2 * dpr, tabW * progress, 2 * dpr);
      }
    });
    setFont(ctx, size);
  }

  private drawGrid(
    ctx: CanvasRenderingContext2D,
    phrases: Phrase[],
    grid: BarGrid,
    W: number,
    H: number,
    dpr: number,
    pxPerSec: number,
    pos: number,
    cx: number,
    rgb: readonly number[],
    top: number,
  ) {
    const beatPx = (60 / grid.bpm) * pxPerSec;
    const barPx = beatPx * 4;
    const lines = gridLines(phrases, grid, pos - (cx + 40 * dpr) / pxPerSec, pos + (W - cx + 40 * dpr) / pxPerSec);
    const xOfLine = (secs: number) => cx + (secs - pos) * pxPerSec;
    const line = (x: number, widthCss: number, y: number, h: number) => {
      const w = Math.max(1, Math.round(widthCss * dpr));
      const left = x - w / 2;
      ctx.fillRect(left, y, w, h);
      return left;
    };
    const waveH = H - top;
    const tick = Math.max(3 * dpr, Math.round(waveH * 0.11));
    if (beatPx >= 6 * dpr) {
      for (const l of lines) {
        if (l.kind !== 'beat') continue;
        const x = xOfLine(l.secs);
        if (beatPx >= 22 * dpr) {
          ctx.fillStyle = GRID.beatFaint;
          line(x, 1, top + tick, waveH - 2 * tick);
        }
        ctx.fillStyle = GRID.beat;
        line(x, 1, top, tick);
        line(x, 1, H - tick, tick);
      }
    }
    const visible: Record<BarLevel, boolean> = { bar: barPx >= 10 * dpr, four: barPx * 4 >= 12 * dpr, eight: barPx * 8 >= 12 * dpr, sixteen: true, phrase: true };
    const deck = rgba(rgb, 1);
    const size = 9.5 * dpr;
    for (const l of lines) {
      if (l.kind === 'beat') continue;
      const mark = classify(l, phrases, grid);
      if (!visible[mark.level]) continue;
      const phraseColor = phrases[mark.phrase] ? PHRASE_STYLE[phrases[mark.phrase].kind].color : deck;
      const color = { phrase: phraseColor, sixteen: rgba(rgb, 0.95), eight: rgba(rgb, 0.68), four: GRID.four, bar: GRID.bar }[mark.level];
      const marker = { phrase: phraseColor, sixteen: deck, eight: rgba(rgb, 0.85), four: GRID.four, bar: GRID.marker }[mark.level];
      const x = xOfLine(l.secs);
      ctx.fillStyle = color;
      const left = line(x, LEVEL_WIDTH[mark.level], top, waveH);
      const w = Math.max(1, Math.round(LEVEL_WIDTH[mark.level] * dpr));
      if (mark.level === 'bar') {
        ctx.fillStyle = GRID.shadow;
        ctx.fillRect(left + w, top, Math.max(1, Math.round(dpr)), waveH);
      }
      if (mark.level !== 'phrase' && barPx >= 14 * dpr) {
        ctx.fillStyle = marker;
        triangle(ctx, left + w / 2, top, 6 * dpr, 4.5 * dpr, true);
        triangle(ctx, left + w / 2, H - 4.5 * dpr, 6 * dpr, 4.5 * dpr, false);
      }
      const span = LEVEL_SPAN[mark.level];
      const labelled = span > 0 && span * barPx >= (span > 1 ? 46 : 88) * dpr;
      if (!labelled || mark.barInPhrase < 1) continue;
      const lx = left + w + 4 * dpr;
      const baseline = Math.round(H - tick - 5 * dpr);
      setFont(ctx, size);
      ctx.textAlign = 'left';
      ctx.textBaseline = 'alphabetic';
      const n = String(mark.barInPhrase);
      ctx.fillStyle = mark.level === 'bar' ? GRID.label : 'rgba(212,212,216,0.95)';
      ctx.fillText(n, lx, baseline);
      if (mark.phraseBars > 0) {
        ctx.fillStyle = 'rgba(212,212,216,0.45)';
        ctx.fillText(`/${mark.phraseBars}`, lx + ctx.measureText(n).width, baseline);
      }
    }
  }

  private drawGhost(ctx: CanvasRenderingContext2D, x: number, H: number, dpr: number, band: number, deck: 0 | 1, kind: PhraseKind, aligned: boolean) {
    if (x < -20 * dpr || x > ctx.canvas.width + 20 * dpr) return;
    const color = aligned ? GRID.aligned : rgba(DECK_RGB[deck], 1);
    const width = Math.round(2 * dpr);
    ctx.fillStyle = aligned ? color : rgba(DECK_RGB[deck], 0.85);
    for (let y = band; y < H; y += 10 * dpr) ctx.fillRect(x - width / 2, y, width, Math.min(6 * dpr, H - y));
    ctx.fillStyle = color;
    triangle(ctx, x, band, 10 * dpr, 6 * dpr, true);
    const size = 9.5 * dpr;
    const letter = deck === 0 ? 'A' : 'B';
    const name = PHRASE_STYLE[kind].name;
    setFont(ctx, size);
    const lw = ctx.measureText(letter).width;
    setFont(ctx, size, 0.06);
    const nw = ctx.measureText(name).width;
    const pad = 5 * dpr;
    const chipH = 15 * dpr;
    const cy = H - chipH - 12 * dpr;
    const chipX = x + 4 * dpr;
    roundRect(ctx, chipX, cy, pad * 2 + lw + 4 * dpr + nw, chipH, 3 * dpr);
    const baseline = Math.round(cy + chipH / 2 + 3.4 * dpr);
    ctx.textAlign = 'left';
    ctx.textBaseline = 'alphabetic';
    ctx.fillStyle = INK;
    setFont(ctx, size);
    ctx.fillText(letter, chipX + pad, baseline);
    ctx.fillStyle = 'rgba(9,9,11,0.75)';
    setFont(ctx, size, 0.06);
    ctx.fillText(name, chipX + pad + lw + 4 * dpr, baseline);
    setFont(ctx, size);
  }
}

export class OverviewRenderer {
  private bg: HTMLCanvasElement | null = null;
  private bgKey = '';
  constructor(readonly deck: 0 | 1) {}

  drawBase(ctx: CanvasRenderingContext2D, W: number, H: number, dpr: number, now: number) {
    const d = engine.latest.decks[this.deck];
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, W, H);
    if (!d.loaded || d.durationSecs <= 0) return;
    const wf = waveforms.get(d.trackToken);
    if (!wf) {
      placeholder(ctx, W, H, dpr, 'Analysing…', true, now);
      return;
    }
    const key = `${d.trackToken}:${waveforms.version(d.trackToken)}:${W}x${H}`;
    if (key !== this.bgKey || !this.bg) {
      const c = document.createElement('canvas');
      c.width = W;
      c.height = H;
      const cctx = c.getContext('2d')!;
      renderColumns(cctx, wf, W, H, 0, d.durationSecs / W, makeGradients(cctx, H, true), true);
      this.bg = c;
      this.bgKey = key;
    }
    ctx.drawImage(this.bg, 0, 0);
  }

  drawOverlay(ctx: CanvasRenderingContext2D, W: number, H: number, dpr: number, now: number) {
    const d = engine.latest.decks[this.deck];
    const rgb = DECK_RGB[this.deck];
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, W, H);
    if (!d.loaded || d.durationSecs <= 0) return;
    const pos = clocks[this.deck].pos;
    const xOf = (t: number) => (t / d.durationSecs) * W;
    const px = Math.round(xOf(pos));

    const grid = d.hasGrid && d.gridBpm > 0 ? deckGrid(d) : null;
    for (const p of phrasesFor(this.deck, d.trackToken)) {
      if (grid) {
        const bar = barSecs(grid);
        ctx.fillStyle = GRID.bar;
        for (let k = 16; p.startSecs + k * bar < p.endSecs - bar / 2; k += 16) ctx.fillRect(Math.round(xOf(p.startSecs + k * bar)), 0, dpr, Math.round(H / 2));
      }
      ctx.fillStyle = withAlpha(PHRASE_STYLE[p.kind].color, 0.65);
      ctx.fillRect(Math.round(xOf(p.startSecs)), 0, dpr, H);
    }

    ctx.fillStyle = 'rgba(9,9,11,0.55)';
    ctx.fillRect(0, 0, px, H);

    if (d.durationSecs > 60) {
      const warnStart = d.durationSecs - 30;
      const pulse = pos > warnStart - 1e-6 && d.playing ? 0.09 + 0.06 * Math.sin(now / 160) : pos > warnStart ? 0.09 : 0.05;
      ctx.fillStyle = `rgba(239,68,68,${pulse})`;
      ctx.fillRect(xOf(warnStart), 0, W - xOf(warnStart), H);
    }

    if (d.loopInSecs !== null && d.loopOutSecs !== null) {
      const x1 = xOf(d.loopInSecs);
      const x2 = xOf(d.loopOutSecs);
      ctx.fillStyle = rgba(LOOP_RGB, d.loopActive ? 0.3 : 0.12);
      ctx.fillRect(x1, 0, Math.max(2 * dpr, x2 - x1), H);
      ctx.fillStyle = rgba(LOOP_RGB, d.loopActive ? 0.95 : 0.5);
      ctx.fillRect(x1, 0, Math.max(2 * dpr, x2 - x1), dpr);
    }
    markers(ctx, d, dpr, H, xOf, W, true);

    ctx.fillStyle = '#fafafa';
    ctx.fillRect(px - dpr, 0, 2 * dpr, H);
    ctx.fillStyle = rgba(rgb, 1);
    ctx.fillRect(px - 2.5 * dpr, 0, 5 * dpr, 2 * dpr);
  }
}
