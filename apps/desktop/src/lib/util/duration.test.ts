// @ts-nocheck
import test from 'node:test';
import assert from 'node:assert/strict';
import { formatDuration } from './duration.ts';

test('formats like the tracklist', () => {
  assert.equal(formatDuration(252), '04:12');
  assert.equal(formatDuration(3723), '1:02:03');
  assert.equal(formatDuration(0), '00:00');
  assert.equal(formatDuration(59.9), '00:59');
  assert.equal(formatDuration(-3), '00:00');
});
