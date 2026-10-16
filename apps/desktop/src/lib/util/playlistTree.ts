import type { Playlist } from '../api/types';

export interface TreeRow {
  playlist: Playlist;
  depth: number;
}

const order = (a: Playlist, b: Playlist) =>
  Number(b.isFolder) - Number(a.isFolder) || a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });

export function visibleRows(playlists: readonly Playlist[], collapsed: ReadonlySet<number>): TreeRow[] {
  const ids = new Set(playlists.map((p) => p.id));
  const children = new Map<number | null, Playlist[]>();
  for (const p of playlists) {
    const parent = p.parentId !== null && ids.has(p.parentId) ? p.parentId : null;
    children.set(parent, [...(children.get(parent) ?? []), p]);
  }
  const rows: TreeRow[] = [];
  const seen = new Set<number>();
  const walk = (parent: number | null, depth: number) => {
    for (const p of (children.get(parent) ?? []).sort(order)) {
      if (seen.has(p.id)) continue;
      seen.add(p.id);
      rows.push({ playlist: p, depth });
      if (p.isFolder && !collapsed.has(p.id)) walk(p.id, depth + 1);
    }
  };
  walk(null, 0);
  return rows;
}

export function ancestors(playlists: readonly Playlist[], id: number): number[] {
  const byId = new Map(playlists.map((p) => [p.id, p]));
  const out: number[] = [];
  for (let parent = byId.get(id)?.parentId ?? null; parent !== null && byId.has(parent) && !out.includes(parent) && parent !== id; ) {
    out.push(parent);
    parent = byId.get(parent)?.parentId ?? null;
  }
  return out;
}
