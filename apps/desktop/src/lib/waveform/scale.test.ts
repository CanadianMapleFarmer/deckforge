// @ts-nocheck
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { trackSecsPerPx } from './scale.ts';

test('dragging follows the waveform at normal tempo', () => {
  assert.equal(trackSecsPerPx(100, 1), 0.01);
});

test('a faster deck is drawn compressed, so one pixel covers more track time', () => {
  assert.ok(Math.abs(trackSecsPerPx(100, 1.08) - 0.0108) < 1e-12);
  assert.ok(Math.abs(trackSecsPerPx(100, 0.92) - 0.0092) < 1e-12);
});

test('extreme tempos use the same floor as the drawing', () => {
  assert.equal(trackSecsPerPx(100, 0), 0.002);
});
