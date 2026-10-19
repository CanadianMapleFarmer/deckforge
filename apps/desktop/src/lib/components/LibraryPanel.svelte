<script lang="ts">
  import { library } from '$lib/state/library.svelte';
  import { ui } from '$lib/state/ui.svelte';
  import { engine } from '$lib/state/engine.svelte';
  import Sidebar from './Sidebar.svelte';
  import SpotifyPanel from './SpotifyPanel.svelte';
  import HistoryPanel from './HistoryPanel.svelte';
  import KeyBadge from './KeyBadge.svelte';
  import TrackTable from './TrackTable.svelte';
  import Search from '@lucide/svelte/icons/search';
  import FolderPlus from '@lucide/svelte/icons/folder-plus';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import X from '@lucide/svelte/icons/x';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import ContextMenu, { type MenuItem } from './ContextMenu.svelte';

  let searchEl = $state<HTMLInputElement | null>(null);
  let importMenu = $state<{ x: number; y: number } | null>(null);
  const importItems: MenuItem[] = [
    { label: 'Add folders to collection', onselect: () => library.importFolders(false) },
    { label: 'Import folders as playlists…', onselect: () => library.importFolders(true) },
    { label: 'Import playlist file…', hint: 'M3U · XML', onselect: () => library.importPlaylistFile() },
  ];
  function openImportMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    importMenu = { x: r.left, y: r.bottom + 4 };
  }
  export function focusSearch() {
    searchEl?.focus();
    searchEl?.select();
  }

  function startResize(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const startY = e.clientY;
    const startH = ui.libraryHeight;
    const move = (ev: PointerEvent) => ui.setLibraryHeight(Math.max(120, Math.min(window.innerHeight - (window.innerHeight <= 820 ? 556 : 608), startH + (startY - ev.clientY))));
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  }
  function startSideResize(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const startX = e.clientX;
    const startW = ui.sidebarWidth;
    const move = (ev: PointerEvent) => ui.setSidebarWidth(Math.max(150, Math.min(380, startW + ev.clientX - startX)));
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  }
  const busy = $derived(library.queued > 0 || library.analysing !== null);
  const masterIndex = $derived(engine.snap.masterDeck);
  const inSpotify = $derived(ui.browserView === 'spotify');
  const inHistory = $derived(ui.browserView === 'history');
</script>

<section class="lib" style="--lib-h:{ui.libraryHeight}px">
  <div class="handle" onpointerdown={startResize} role="separator" aria-orientation="horizontal" aria-label="Resize library" tabindex="-1"><i></i></div>
  {#if !inSpotify && !inHistory}
  <div class="bar">
    <div class="search">
        <Search size={14} strokeWidth={2} />
        <input
          bind:this={searchEl}
          type="search"
          placeholder="Search title, artist, genre, key"
          value={library.query}
          oninput={(e) => library.setQuery(e.currentTarget.value)}
          onkeydown={(e) => {
            e.stopPropagation();
            if (e.key === 'Escape') {
              library.setQuery('');
              e.currentTarget.blur();
            }
          }}
          aria-label="Search library"
        />
        {#if library.query}<button class="clear" onclick={() => library.setQuery('')} aria-label="Clear search"><X size={13} /></button>{:else}<kbd>/</kbd>{/if}
      </div>
      <span class="count num">{library.sorted.length} track{library.sorted.length === 1 ? '' : 's'}{library.playlistId !== null ? ' in playlist' : ''}</span>
      {#if library.masterKey && masterIndex !== null}
        <div class="match" title="Keys that mix with the master deck: same key, one step either way, and the relative major or minor">
          <span>Mixes with {masterIndex === 0 ? 'A' : 'B'}</span>
          <KeyBadge value={library.masterKey} size="sm" compat="same" />
        </div>
      {/if}
    <div class="grow"></div>
    {#if busy}
      <span class="pill busy"><LoaderCircle size={13} class="spin" strokeWidth={2.4} />Analysing{library.queued > 0 ? ` · ${library.queued} queued` : ''}</span>
    {/if}
    <button class="btn btn-primary" onclick={openImportMenu} disabled={library.importing} aria-haspopup="menu" aria-expanded={importMenu !== null}>
        {#if library.importing}<LoaderCircle size={14} class="spin" strokeWidth={2.4} />Importing…{:else}<FolderPlus size={14} strokeWidth={2.1} />Import<ChevronDown size={13} strokeWidth={2.2} />{/if}
    </button>
    {#if importMenu}
      <ContextMenu x={importMenu.x} y={importMenu.y} items={importItems} onclose={() => (importMenu = null)} />
    {/if}
  </div>
  {/if}
  <div class="main" style="grid-template-columns:{ui.sidebarWidth}px 1fr">
    <div class="sidew">
      <Sidebar />
      <div class="vsplit" onpointerdown={startSideResize} role="separator" aria-orientation="vertical" tabindex="-1"></div>
    </div>
    {#if inSpotify}
      <SpotifyPanel />
    {:else if inHistory}
      <HistoryPanel />
    {:else}
      <TrackTable />
    {/if}
  </div>
</section>

<style>
  .lib {
    --chrome: 608px;
    position: relative;
    display: flex;
    flex-direction: column;
    flex: none;
    height: max(120px, min(var(--lib-h), calc(100vh - var(--chrome))));
    border-top: 1px solid var(--line);
    background: var(--panel);
    min-height: 0;
  }
  @media (max-height: 820px) {
    .lib {
      --chrome: 563px;
    }
  }
  .handle {
    position: absolute;
    top: -5px;
    left: 0;
    right: 0;
    height: 10px;
    cursor: ns-resize;
    z-index: 6;
    display: grid;
    place-items: center;
    touch-action: none;
  }
  .handle i {
    width: 44px;
    height: 3px;
    border-radius: 2px;
    background: var(--line-2);
    transition: background var(--dur);
  }
  .handle:hover i {
    background: var(--dim);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 42px;
    padding: 0 10px;
    flex: none;
    border-bottom: 1px solid var(--line);
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    gap: 7px;
    width: 320px;
    height: 28px;
    padding: 0 8px 0 9px;
    border-radius: var(--radius);
    border: 1px solid var(--line);
    background: var(--ink);
    color: var(--faint);
    transition: border-color var(--dur), box-shadow var(--dur);
  }
  .search:hover {
    border-color: var(--line-2);
  }
  .search:focus-within {
    border-color: var(--ring);
    box-shadow: 0 0 0 3px rgba(161, 161, 170, 0.15);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: transparent;
    font-size: 12.5px;
    color: var(--fg);
  }
  .search input::placeholder {
    color: var(--faint);
  }
  .search input::-webkit-search-cancel-button {
    display: none;
  }
  kbd {
    font: 600 10.5px var(--font-mono);
    min-width: 18px;
    height: 18px;
    display: grid;
    place-items: center;
    border-radius: 4px;
    border: 1px solid var(--line-2);
    color: var(--faint);
  }
  .clear {
    border: 0;
    background: transparent;
    color: var(--dim);
    display: grid;
    place-items: center;
  }
  .count {
    font-size: 11.5px;
    color: var(--faint);
  }
  .match {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: var(--faint);
    padding-left: 10px;
    border-left: 1px solid var(--line);
  }
  .grow {
    flex: 1;
  }
  @media (max-height: 820px) {
    .bar {
      height: 38px;
    }
  }
  .busy {
    color: #93c5fd;
    border-color: color-mix(in srgb, #3b82f6 35%, var(--line));
    background: color-mix(in srgb, #3b82f6 8%, transparent);
  }
  .main {
    flex: 1;
    min-height: 0;
    display: grid;
  }
  .sidew {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .sidew :global(.side) {
    flex: 1;
  }
  .vsplit {
    position: absolute;
    top: 0;
    bottom: 0;
    right: -3px;
    width: 6px;
    cursor: ew-resize;
    z-index: 5;
  }
  .vsplit:hover {
    background: var(--line-2);
  }
</style>
