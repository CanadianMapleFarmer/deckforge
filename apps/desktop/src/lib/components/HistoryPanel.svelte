<script lang="ts">
  import { history, EXPORT_FORMATS, folderOf } from '$lib/state/history.svelte';
  import { formatDuration } from '$lib/util/duration';
  import { keyHue } from '$lib/util/camelot';
  import type { SetEntry, SetSummary } from '$lib/api';
  import KeyBadge from './KeyBadge.svelte';
  import ContextMenu, { type MenuItem } from './ContextMenu.svelte';
  import Copy from '@lucide/svelte/icons/copy';
  import Download from '@lucide/svelte/icons/download';
  import ListPlus from '@lucide/svelte/icons/list-plus';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import Ellipsis from '@lucide/svelte/icons/ellipsis';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import History from '@lucide/svelte/icons/history';
  import { tick } from 'svelte';

  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
  let renaming = $state(false);
  let confirmDelete = $state(false);
  let draft = $state('');
  let nameEl = $state<HTMLInputElement | null>(null);
  let hovered = $state<number | null>(null);

  const detail = $derived(history.detail);
  const summary = $derived(detail?.summary ?? null);
  const recording = $derived(detail?.recordings.at(-1) ?? null);
  const playedEntries = $derived(
    (detail?.entries ?? []).filter((e) => e.tookOverSecs !== null).sort((a, b) => a.tookOverSecs! - b.tookOverSecs!),
  );
  const rows = $derived(
    [...(detail?.entries ?? [])].sort((a, b) => (a.tookOverSecs ?? a.audibleSecs) - (b.tookOverSecs ?? b.audibleSecs)),
  );
  const span = $derived(Math.max(summary?.durationSecs ?? 0, ...(detail?.entries ?? []).map((e) => Math.max(e.audibleSecs, e.tookOverSecs ?? 0) + 30), 60));
  const segments = $derived(
    playedEntries.map((e, i) => {
      const start = e.tookOverSecs!;
      const end = playedEntries[i + 1]?.tookOverSecs ?? summary?.durationSecs ?? span;
      return { e, start, end: Math.max(end, start), blend: Math.max(0, start - e.audibleSecs) };
    }),
  );
  const ticks = $derived.by(() => {
    const step = [60, 300, 600, 900, 1800, 3600].find((s) => span / s <= 8) ?? 3600;
    return Array.from({ length: Math.floor(span / step) + 1 }, (_, i) => i * step);
  });

  function hueStyle(e: SetEntry): string {
    const h = keyHue(e.key);
    return h === null ? '--h:240;--s:6%' : `--h:${h};--s:78%`;
  }
  function pct(secs: number): number {
    return (100 * secs) / span;
  }
  function dateLabel(s: SetSummary): string {
    return new Date(s.startedAt).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  }
  function timeLabel(s: SetSummary): string {
    return new Date(s.startedAt).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  }
  function blendText(e: SetEntry): string | null {
    if (e.tookOverSecs === null) return `heard from ${formatDuration(e.audibleSecs)}`;
    return e.tookOverSecs - e.audibleSecs >= 1 ? `mixed in from ${formatDuration(e.audibleSecs)}` : null;
  }

  async function startRename() {
    if (!summary) return;
    draft = summary.name;
    renaming = true;
    await tick();
    nameEl?.focus();
    nameEl?.select();
  }
  async function commitRename() {
    if (!renaming || !summary) return;
    renaming = false;
    if (draft.trim() && draft.trim() !== summary.name) await history.rename(summary.id, draft);
  }
  function nameKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === 'Enter') commitRename();
    else if (e.key === 'Escape') renaming = false;
  }
  function anchored(e: MouseEvent, items: MenuItem[]) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { x: r.left, y: r.bottom + 4, items };
  }
  function exportMenu(e: MouseEvent) {
    if (!summary) return;
    const id = summary.id;
    anchored(e, EXPORT_FORMATS.map((f) => ({ label: f.label, onselect: () => history.export(id, f.format) })));
  }
  function moreMenu(e: MouseEvent) {
    if (!summary) return;
    const id = summary.id;
    const items: MenuItem[] = [{ label: 'Rename', onselect: startRename }];
    if (detail?.recordings.length) items.push({ label: 'Rewrite recording files', hint: 'tracklist and markers', onselect: () => history.rewriteFiles(id) });
    items.push({ divider: true }, { label: 'Delete set', hint: 'recordings stay on disk', danger: true, onselect: () => (confirmDelete = true) });
    anchored(e, items);
  }
  function rowMenu(ev: MouseEvent, e: SetEntry) {
    ev.preventDefault();
    const items: MenuItem[] = [];
    if (e.trackId !== null) items.push({ label: 'Show in collection', onselect: () => history.showTrack(e.trackId!) });
    items.push({ label: 'Remove from set', danger: true, onselect: () => history.removeEntry(e.id) });
    menu = { x: ev.clientX, y: ev.clientY, items };
  }
  $effect(() => {
    history.openId;
    confirmDelete = false;
    renaming = false;
  });
</script>

<div class="history">
  <nav class="sets" aria-label="Sets">
    {#each history.sets as s (s.id)}
      <button class="set" class:sel={s.id === history.openId} class:live={s.live} onclick={() => history.open(s.id)}>
        <span class="row1">
          {#if s.live}<i class="pulse" aria-hidden="true"></i>{/if}
          <span class="sname">{s.name}</span>
          {#if s.hasRecording}<span class="rec" title="Has a recording">REC</span>{/if}
        </span>
        <span class="row2 num">
          <span>{s.live ? 'Playing now' : dateLabel(s)}</span>
          <span>{formatDuration(s.durationSecs)}</span>
          <span>{s.trackCount} track{s.trackCount === 1 ? '' : 's'}</span>
        </span>
      </button>
    {:else}
      <div class="empty-list">
        <History size={18} strokeWidth={1.8} />
        <p>Every set is logged here once a track reaches the master.</p>
      </div>
    {/each}
  </nav>

  <section class="detail">
    {#if summary && detail}
      <header>
        <div class="title">
          {#if renaming}
            <input class="name-edit" bind:this={nameEl} bind:value={draft} onblur={commitRename} onkeydown={nameKey} aria-label="Set name" />
          {:else}
            <h2 ondblclick={startRename} title="Double-click to rename">{summary.name}</h2>
          {/if}
          <div class="meta num">
            {#if summary.live}<span class="livetag"><i class="pulse"></i>Live</span>{/if}
            <span>{dateLabel(summary)}, {timeLabel(summary)}</span>
            <span>{formatDuration(summary.durationSecs)}</span>
            <span>{summary.trackCount} track{summary.trackCount === 1 ? '' : 's'}</span>
          </div>
        </div>
        <div class="actions">
          <button class="btn btn-primary" onclick={() => history.copyTracklist(summary.id)}><Copy size={14} strokeWidth={2.1} />Copy tracklist</button>
          <button class="btn" onclick={exportMenu} aria-haspopup="menu"><Download size={14} strokeWidth={2.1} />Export<ChevronDown size={13} strokeWidth={2.2} /></button>
          <button class="btn" onclick={() => history.toPlaylist(summary.id)}><ListPlus size={14} strokeWidth={2.1} />Make playlist</button>
          {#if recording}
            <button class="btn" onclick={() => history.openFolder(recording.path)} title={folderOf(recording.path)}><FolderOpen size={14} strokeWidth={2.1} />Recording</button>
          {/if}
          <button class="btn btn-ghost icon" onclick={moreMenu} aria-label="More set actions" aria-haspopup="menu"><Ellipsis size={16} strokeWidth={2} /></button>
        </div>
      </header>

      {#if confirmDelete}
        <div class="confirm" role="alertdialog" aria-label="Delete set">
          <p>Delete <b>{summary.name}</b> from the history? Recording files stay where they are.</p>
          <button class="btn" onclick={() => (confirmDelete = false)}>Keep set</button>
          <button class="btn danger" onclick={() => { confirmDelete = false; void history.remove(summary.id); }}>Delete set</button>
        </div>
      {/if}

      <div class="ribbon" aria-hidden="true">
        <div class="lane">
          {#each segments as seg, i (seg.e.id)}
            <div
              class="seg"
              class:hot={hovered === seg.e.id}
              style="left:{pct(seg.start)}%;width:{pct(seg.end - seg.start)}%;{hueStyle(seg.e)}"
              title="{formatDuration(seg.start)} {seg.e.artist ? `${seg.e.artist} – ` : ''}{seg.e.title}"
            >
              <span>{i + 1}</span>
            </div>
            {#if seg.blend > 0}
              <div class="blend" style="left:{pct(seg.start - seg.blend)}%;width:{pct(seg.blend)}%;{hueStyle(seg.e)}"></div>
            {/if}
          {/each}
          {#each detail.entries.filter((e) => e.teased) as e (e.id)}
            <i class="tease" style="left:{pct(e.audibleSecs)}%;{hueStyle(e)}" title="Teased: {e.title}"></i>
          {/each}
        </div>
        {#each detail.recordings as r (r.id)}
          <div class="recspan" style="left:{pct(r.startedSecs)}%;width:{pct((r.stoppedSecs ?? summary.durationSecs) - r.startedSecs)}%"></div>
        {/each}
        <div class="ticks num">
          {#each ticks as t (t)}<span style="left:{pct(t)}%">{formatDuration(t)}</span>{/each}
        </div>
      </div>

      <div class="rows" role="list">
        {#each rows as e (e.id)}
          <div
            class="entry"
            class:teased={e.teased}
            class:pending={e.tookOverSecs === null && !e.teased}
            class:clickable={e.trackId !== null}
            role="listitem"
            style={hueStyle(e)}
            onpointerenter={() => (hovered = e.id)}
            onpointerleave={() => (hovered = null)}
            oncontextmenu={(ev) => rowMenu(ev, e)}
            ondblclick={() => e.trackId !== null && history.showTrack(e.trackId)}
            title={e.trackId !== null ? 'Double-click to show it in the collection' : undefined}
          >
            <span class="at num">{e.tookOverSecs !== null ? formatDuration(e.tookOverSecs) : '–'}</span>
            <span class="what">
              <span class="t">{e.title || e.path.split(/[\\/]/).pop()}</span>
              <span class="a">{e.artist}</span>
            </span>
            <span class="blendtxt num">
              {#if e.teased}<span class="tag">Teased</span>{:else if e.tookOverSecs === null}<span class="tag in">Mixing in</span>{/if}
              {blendText(e) ?? ''}
            </span>
            <span class="key"><KeyBadge value={e.key} size="sm" /></span>
            <span class="bpm num">{e.bpm ? e.bpm.toFixed(1) : ''}</span>
          </div>
        {:else}
          <p class="empty">Nothing has taken over yet. Tracks appear here as soon as they're the only one on the master.</p>
        {/each}
      </div>
    {:else}
      <div class="empty-detail">
        <History size={28} strokeWidth={1.6} />
        <p>Play a track to the master and the set starts logging itself.</p>
      </div>
    {/if}
  </section>
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .history {
    display: grid;
    grid-template-columns: 232px minmax(0, 1fr);
    min-height: 0;
    height: 100%;
    background: var(--panel);
  }
  .sets {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px;
    border-right: 1px solid var(--line);
    overflow-y: auto;
  }
  .set {
    display: grid;
    gap: 3px;
    padding: 8px 10px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--dim);
    text-align: left;
  }
  .set:hover {
    background: rgba(255, 255, 255, 0.05);
    color: var(--fg);
  }
  .set.sel {
    background: var(--raised-3);
    color: var(--fg);
  }
  .row1 {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    font-weight: 600;
    font-size: 12.5px;
  }
  .sname {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row2 {
    display: flex;
    gap: 10px;
    font-size: 11px;
    color: var(--faint);
  }
  .rec {
    padding: 1px 5px;
    border-radius: 4px;
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
    color: #fecaca;
    background: color-mix(in srgb, var(--danger) 28%, transparent);
  }
  .pulse {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--ok);
    box-shadow: 0 0 0 0 color-mix(in srgb, var(--ok) 60%, transparent);
    animation: pulse 1.8s ease-out infinite;
  }
  @keyframes pulse {
    70% {
      box-shadow: 0 0 0 6px transparent;
    }
    100% {
      box-shadow: 0 0 0 0 transparent;
    }
  }
  .detail {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
    padding: 14px 18px 10px;
    flex-wrap: wrap;
  }
  .title {
    min-width: 0;
  }
  h2,
  .name-edit {
    margin: 0;
    font-size: 20px;
    font-weight: 680;
    letter-spacing: -0.015em;
    color: var(--fg);
    line-height: 1.2;
  }
  h2 {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: text;
  }
  .name-edit {
    width: min(420px, 100%);
    padding: 0 6px;
    margin-left: -7px;
    border: 1px solid var(--line-2);
    border-radius: 6px;
    background: var(--raised);
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 4px;
    font-size: 12px;
    color: var(--dim);
  }
  .livetag {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ok);
    font-weight: 620;
  }
  .actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .confirm {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 18px 8px;
    padding: 8px 10px 8px 14px;
    border: 1px solid color-mix(in srgb, var(--danger) 45%, var(--line));
    border-radius: 8px;
    background: color-mix(in srgb, var(--danger) 9%, var(--panel));
  }
  .confirm p {
    flex: 1;
    margin: 0;
    font-size: 12.5px;
    color: var(--text);
  }
  .btn.danger {
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
  }
  .ribbon {
    position: relative;
    margin: 4px 18px 6px;
    padding-bottom: 18px;
  }
  .lane {
    position: relative;
    height: 30px;
    border-radius: 7px;
    background: var(--raised);
    overflow: hidden;
  }
  .seg {
    position: absolute;
    top: 0;
    bottom: 0;
    display: flex;
    align-items: center;
    padding-left: 7px;
    background: linear-gradient(180deg, hsl(var(--h) var(--s) 58%), hsl(var(--h) var(--s) 44%));
    box-shadow: inset 1px 0 0 rgba(0, 0, 0, 0.45);
    transition: filter 120ms;
  }
  .seg span {
    font-size: 10.5px;
    font-weight: 700;
    color: rgba(0, 0, 0, 0.62);
    font-variant-numeric: tabular-nums;
  }
  .seg.hot {
    filter: brightness(1.25) saturate(1.1);
  }
  .blend {
    position: absolute;
    top: 0;
    bottom: 0;
    background: repeating-linear-gradient(135deg, hsl(var(--h) var(--s) 55% / 0.85) 0 3px, transparent 3px 7px);
    pointer-events: none;
  }
  .tease {
    position: absolute;
    bottom: 3px;
    width: 6px;
    height: 6px;
    margin-left: -3px;
    border-radius: 50%;
    background: hsl(var(--h) var(--s) 62%);
    box-shadow: 0 0 0 2px var(--raised);
  }
  .recspan {
    position: absolute;
    top: 33px;
    height: 3px;
    border-radius: 2px;
    background: var(--danger);
  }
  .ticks {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 14px;
    font-size: 10px;
    color: var(--faint);
  }
  .ticks span {
    position: absolute;
    transform: translateX(-50%);
  }
  .ticks span:first-child {
    transform: none;
  }
  .rows {
    flex: 1;
    overflow-y: auto;
    padding: 2px 10px 12px;
  }
  .entry {
    display: grid;
    grid-template-columns: 72px minmax(0, 1fr) minmax(0, 190px) 52px 52px;
    align-items: center;
    gap: 12px;
    min-height: 40px;
    padding: 4px 8px 4px 12px;
    border-radius: 7px;
    box-shadow: inset 3px 0 0 hsl(var(--h) var(--s) 55%);
  }
  .entry:hover {
    background: rgba(255, 255, 255, 0.045);
  }
  .entry.clickable {
    cursor: default;
  }
  .at {
    font-size: 15px;
    font-weight: 640;
    color: var(--fg);
  }
  .what {
    display: grid;
    min-width: 0;
  }
  .what .t {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 580;
    color: var(--text);
  }
  .what .a {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--dim);
  }
  .blendtxt {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 11.5px;
    color: var(--faint);
    white-space: nowrap;
    overflow: hidden;
  }
  .tag {
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 10.5px;
    font-weight: 620;
    color: var(--dim);
    background: var(--raised-3);
  }
  .tag.in {
    color: var(--ok);
    background: color-mix(in srgb, var(--ok) 16%, transparent);
  }
  .bpm {
    font-size: 12px;
    color: var(--dim);
    text-align: right;
  }
  .entry.teased {
    box-shadow: inset 3px 0 0 var(--line-2);
  }
  .entry.teased .at,
  .entry.teased .what,
  .entry.teased .key,
  .entry.teased .bpm {
    opacity: 0.42;
  }
  .empty,
  .empty-list p,
  .empty-detail p {
    margin: 0;
    font-size: 12.5px;
    color: var(--faint);
    line-height: 1.5;
  }
  .empty {
    padding: 18px 8px;
  }
  .empty-list {
    display: grid;
    gap: 8px;
    padding: 18px 10px;
    color: var(--faint);
  }
  .empty-detail {
    display: grid;
    place-items: center;
    align-content: center;
    gap: 10px;
    height: 100%;
    color: var(--faint);
  }
  @media (prefers-reduced-motion: reduce) {
    .pulse {
      animation: none;
    }
  }
</style>
