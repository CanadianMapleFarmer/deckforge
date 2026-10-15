<script lang="ts">
  import { library } from '$lib/state/library.svelte';
  import { engine } from '$lib/state/engine.svelte';
  import { ui } from '$lib/state/ui.svelte';
  import { startTrackDrag } from '$lib/state/dnd.svelte';
  import { toast } from '$lib/state/toast.svelte';
  import { fmtBpm, fmtDuration } from '$lib/util/format';
  import { keyCompat } from '$lib/util/camelot';
  import KeyBadge from './KeyBadge.svelte';
  import ArtThumb from './ArtThumb.svelte';
  import ContextMenu, { type MenuItem } from './ContextMenu.svelte';
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import ArrowDown from '@lucide/svelte/icons/arrow-down';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Check from '@lucide/svelte/icons/check';
  import Clock from '@lucide/svelte/icons/clock';
  import type { TrackSummary } from '$lib/api';

  const COLS = [
    { key: 'deck', label: '', w: '34px', sortable: false },
    { key: 'art', label: '', w: '34px', sortable: false },
    { key: 'title', label: 'Title', w: 'minmax(180px,2.4fr)' },
    { key: 'artist', label: 'Artist', w: 'minmax(120px,1.4fr)' },
    { key: 'bpm', label: 'BPM', w: '64px', right: true },
    { key: 'key', label: 'Key', w: '62px' },
    { key: 'durationSecs', label: 'Time', w: '60px', right: true },
    { key: 'genre', label: 'Genre', w: 'minmax(90px,1fr)' },
    { key: 'analysis', label: 'Analysis', w: '96px' },
  ] as const;
  const template = COLS.map((c) => c.w).join(' ');

  let scroller: HTMLDivElement;
  let scrollTop = $state(0);
  let viewH = $state(300);
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);

  const ROW = $derived(ui.compact ? 30 : 34);
  const rows = $derived(library.sorted);
  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - 6));
  const last = $derived(Math.min(rows.length, Math.ceil((scrollTop + viewH) / ROW) + 6));
  const visible = $derived(rows.slice(first, last));
  const loadedTokens = $derived([engine.snap.decks[0].loaded ? engine.snap.decks[0].trackToken : 0, engine.snap.decks[1].loaded ? engine.snap.decks[1].trackToken : 0]);

  $effect(() => {
    library.scrollToCursor;
    const id = library.cursorId;
    if (id === null || !scroller) return;
    const idx = library.indexById.get(id);
    if (idx === undefined) return;
    const top = idx * ROW;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (top + ROW > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = top + ROW - scroller.clientHeight;
  });

  function rowDown(e: PointerEvent, t: TrackSummary) {
    if (e.button === 2) {
      if (!library.selected.has(t.id)) library.select(t.id);
      return;
    }
    if (e.button !== 0) return;
    if (e.metaKey || e.ctrlKey) return library.select(t.id, 'toggle');
    if (e.shiftKey) return library.select(t.id, 'range');
    if (!library.selected.has(t.id)) library.select(t.id);
    else {
      library.cursorId = t.id;
      library.focus = 'table';
    }
    const ids = library.selected.has(t.id) ? [...library.selected] : [t.id];
    startTrackDrag(e, ids, ids.length > 1 ? `${ids.length} tracks` : t.title);
  }

  function rowClick(e: MouseEvent, t: TrackSummary) {
    if (e.metaKey || e.ctrlKey || e.shiftKey) return;
    library.select(t.id);
  }

  function dbl(e: MouseEvent, t: TrackSummary) {
    const deck = library.pickDeck(e.altKey ? 1 : e.shiftKey ? 0 : undefined);
    library.loadToDeck(deck, t.id);
  }

  function contextmenu(e: MouseEvent, t: TrackSummary) {
    e.preventDefault();
    const ids = library.selected.has(t.id) ? [...library.selected] : [t.id];
    const playlists = library.playlists;
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: 'Load to Deck A', hint: '←', onselect: () => library.loadToDeck(0, t.id) },
        { label: 'Load to Deck B', hint: '→', onselect: () => library.loadToDeck(1, t.id) },
        { divider: true },
        { label: ids.length > 1 ? `Add ${ids.length} tracks to playlist` : 'Add to playlist', heading: true },
        ...(playlists.length
          ? playlists.map((p) => ({ label: p.name, onselect: () => library.addToPlaylist(p.id, ids) }))
          : [{ label: 'No playlists yet', disabled: true }]),
        ...(library.playlistId !== null
          ? [{ divider: true }, { label: 'Remove from this playlist', danger: true, onselect: () => library.removeFromCurrentPlaylist(t.id) }]
          : []),
        { divider: true },
        { label: 'Re-analyse', onselect: () => library.reanalyze(t.id) },
        {
          label: 'Show path',
          onselect: () => {
            navigator.clipboard?.writeText(t.path).catch(() => {});
            toast.info(t.path);
          },
        },
      ],
    };
  }
</script>

<div class="table" style="--row:{ROW}px" class:focused={library.focus === 'table'} onpointerdown={() => (library.focus = 'table')} role="grid" tabindex="-1" aria-label="Tracks">
  <div class="head" style="grid-template-columns:{template}">
    {#each COLS as c (c.key)}
      {#if c.key === 'deck' || c.key === 'art'}
        <span></span>
      {:else}
        <button class:right={'right' in c && c.right} class:active={ui.sort.key === c.key} onclick={() => ui.setSort(c.key)}>
          {c.label}
          {#if ui.sort.key === c.key}{#if ui.sort.dir === 1}<ArrowUp size={11} strokeWidth={2.6} />{:else}<ArrowDown size={11} strokeWidth={2.6} />{/if}{/if}
        </button>
      {/if}
    {/each}
  </div>

  <div class="body" bind:this={scroller} bind:clientHeight={viewH} onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}>
    {#if rows.length === 0}
      <div class="empty">
        {#if library.loading}
          <LoaderCircle size={16} class="spin" /> Loading library…
        {:else if library.query}
          <b>No matches</b><span>Nothing in {library.playlistId === null ? 'your collection' : 'this playlist'} matches “{library.query}”.</span>
        {:else if library.playlistId !== null}
          <b>This playlist is empty</b><span>Drag tracks from the collection onto it in the sidebar.</span>
        {:else}
          <b>Your collection is empty</b><span>Use “Import folders” to add your music. Tracks are analysed for BPM, key and waveform in the background.</span>
        {/if}
      </div>
    {:else}
      <div class="spacer" style="height:{rows.length * ROW}px">
        {#each visible as t, i (t.id)}
          {@const idx = first + i}
          {@const compat = library.masterKey ? (keyCompat(library.masterKey, t.key) ?? 'dim') : null}
          {@const sel = library.selected.has(t.id)}
          <div
            class="row"
            class:sel
            class:cursor={library.cursorId === t.id}
            class:alt={idx % 2 === 1}
            style="top:{idx * ROW}px;grid-template-columns:{template}"
            role="row"
            tabindex="-1"
            aria-selected={sel}
            onpointerdown={(e) => rowDown(e, t)}
            onclick={(e) => rowClick(e, t)}
            ondblclick={(e) => dbl(e, t)}
            oncontextmenu={(e) => contextmenu(e, t)}
          >
            <span class="decks">
              {#if loadedTokens[0] === t.id}<b data-deck="0" class="badge">A</b>{/if}
              {#if loadedTokens[1] === t.id}<b data-deck="1" class="badge">B</b>{/if}
            </span>
            <span class="t art"><ArtThumb id={t.hasArtwork ? t.id : null} size={ui.compact ? 22 : 26} /></span>
            <span class="t title" title={t.title}>{t.title}</span>
            <span class="t dim" title={t.artist}>{t.artist}</span>
            <span class="t num right">{fmtBpm(t.bpm, 1)}</span>
            <span class="t"><KeyBadge value={t.key} name={t.keyName} size="sm" {compat} /></span>
            <span class="t num right dim">{fmtDuration(t.durationSecs)}</span>
            <span class="t dim" title={t.genre}>{t.genre || '—'}</span>
            <span class="t an" data-state={t.analysis}>
              {#if t.analysis === 'done'}<Check size={12} strokeWidth={2.6} />Ready
              {:else if t.analysis === 'running'}<LoaderCircle size={12} strokeWidth={2.6} class="spin" />Analysing
              {:else if t.analysis === 'failed'}<TriangleAlert size={12} strokeWidth={2.4} />Failed
              {:else}<Clock size={12} strokeWidth={2.4} />Queued{/if}
            </span>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .table {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    position: relative;
    background: var(--ink);
  }
  .table.focused::after {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
    box-shadow: inset 0 0 0 1px var(--line-2);
    z-index: 4;
  }
  .head {
    display: grid;
    align-items: center;
    height: 26px;
    flex: none;
    padding-right: 12px;
    background: var(--panel);
    border-bottom: 1px solid var(--line);
  }
  .head button {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 100%;
    padding: 0 8px;
    border: 0;
    background: transparent;
    font-size: 11px;
    font-weight: 560;
    color: var(--faint);
    text-align: left;
    transition: color var(--dur);
  }
  .head button.right {
    justify-content: flex-end;
  }
  .head button:hover,
  .head button.active {
    color: var(--fg);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    position: relative;
  }
  .spacer {
    position: relative;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    height: var(--row);
    display: grid;
    align-items: center;
    font-size: 12.5px;
    cursor: default;
    border-left: 2px solid transparent;
    border-bottom: 1px solid rgba(255, 255, 255, 0.03);
    transition: background var(--dur);
  }
  .row:hover {
    background: rgba(255, 255, 255, 0.04);
  }
  .row.sel {
    background: rgba(255, 255, 255, 0.07);
  }
  .row.cursor {
    background: rgba(255, 255, 255, 0.09);
    border-left-color: var(--dim);
  }
  .focused .row.cursor {
    z-index: 1;
    background: color-mix(in srgb, var(--fg) 17%, var(--ink));
    border-left-color: var(--fg);
    box-shadow: inset 2px 0 0 var(--fg), inset 0 0 0 1px rgba(255, 255, 255, 0.38);
  }
  .focused .row.cursor .t,
  .focused .row.cursor .dim {
    color: var(--fg);
  }
  .focused .row.cursor .title {
    font-weight: 680;
  }
  .t {
    padding: 0 8px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .art {
    padding: 0 4px;
    display: flex;
    align-items: center;
  }
  .title {
    font-weight: 560;
    color: var(--fg);
  }
  .dim {
    color: var(--dim);
  }
  .right {
    text-align: right;
  }
  .num {
    font-size: 12px;
  }
  .decks {
    display: flex;
    gap: 2px;
    padding-left: 4px;
  }
  .badge {
    width: 15px;
    height: 15px;
    border-radius: 4px;
    display: grid;
    place-items: center;
    font-size: 9.5px;
    font-weight: 750;
    color: #09090b;
    background: var(--deck-a);
  }
  .badge[data-deck='1'] {
    background: var(--deck-b);
  }
  .an {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    color: var(--faint);
  }
  .an[data-state='done'] {
    color: #6ee7a0;
  }
  .an[data-state='running'] {
    color: #93c5fd;
  }
  .an[data-state='failed'] {
    color: var(--danger);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    height: 100%;
    min-height: 100px;
    padding: 20px;
    text-align: center;
    color: var(--dim);
    font-size: 12.5px;
  }
  .empty b {
    color: var(--fg);
    font-size: 14px;
  }
  .empty span {
    max-width: 380px;
    line-height: 1.45;
  }
</style>
