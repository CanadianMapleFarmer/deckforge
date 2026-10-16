// @ts-nocheck
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ancestors, visibleRows } from './playlistTree.ts';

const p = (id: number, name: string, parentId: number | null, isFolder = false) => ({ id, name, trackCount: 0, parentId, isFolder });
const all = [p(1, 'zeta', null), p(2, 'Crates', null, true), p(3, 'b', 2), p(4, 'A', 2), p(5, 'orphan', 99), p(6, 'Inner', 2, true), p(7, 'deep', 6)];

test('folders first, children indented and sorted', () => {
  assert.deepEqual(
    visibleRows(all, new Set()).map((r) => [r.playlist.id, r.depth]),
    [[2, 0], [6, 1], [7, 2], [4, 1], [3, 1], [5, 0], [1, 0]],
  );
});

test('collapsed folders hide their whole subtree', () => {
  assert.deepEqual(visibleRows(all, new Set([2])).map((r) => r.playlist.id), [2, 5, 1]);
  assert.deepEqual(visibleRows(all, new Set([6])).map((r) => r.playlist.id), [2, 6, 4, 3, 5, 1]);
});

test('a parent cycle does not recurse forever', () => {
  const cyclic = [p(1, 'a', 2, true), p(2, 'b', 1, true)];
  assert.deepEqual(visibleRows(cyclic, new Set()).length <= 2, true);
});

test('ancestors lists the containing folders, nearest first', () => {
  assert.deepEqual(ancestors(all, 7), [6, 2]);
  assert.deepEqual(ancestors(all, 1), []);
  assert.deepEqual(ancestors([p(1, 'a', 2, true), p(2, 'b', 1, true)], 1), [2]);
});
