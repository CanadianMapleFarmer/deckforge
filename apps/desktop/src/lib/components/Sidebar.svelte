<script lang="ts">
  import { library } from '$lib/state/library.svelte';
  import { dropTargets } from '$lib/state/dnd.svelte';
  import { ui } from '$lib/state/ui.svelte';
  import { spotify } from '$lib/state/spotify.svelte';
  import ContextMenu, { type MenuItem } from './ContextMenu.svelte';
  import Library from '@lucide/svelte/icons/library';
  import ListMusic from '@lucide/svelte/icons/list-music';
  import Folder from '@lucide/svelte/icons/folder';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import Plus from '@lucide/svelte/icons/plus';
  import AudioLines from '@lucide/svelte/icons/audio-lines';
  import History from '@lucide/svelte/icons/history';
  import { history } from '$lib/state/history.svelte';
  import type { Playlist } from '$lib/api';
  import { tick } from 'svelte';

  let creating = $state<{ parentId: number | null; isFolder: boolean } | null>(null);
  let renamingId = $state<number | null>(null);
  let draft = $state('');
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
  let inputEl = $state<HTMLInputElement | null>(null);

  const inLibrary = $derived(ui.browserView === 'library');
  const createDepth = $derived.by(() => {
    if (!creating || creating.parentId === null) return 0;
    const parent = library.rows.find((r) => r.playlist.id === creating?.parentId);
    return parent ? parent.depth + 1 : 0;
  });

  function openLibrary(id: number | null) {
    ui.browserView = 'library';
    library.selectPlaylist(id);
  }
  function openHistory() {
    ui.browserView = 'history';
    library.focus = 'sidebar';
    void history.refresh();
  }
  function openSpotify() {
    ui.browserView = 'spotify';
    library.focus = 'sidebar';
    if (!spotify.statusLoaded) void spotify.refreshStatus();
  }

  async function startCreate(parentId: number | null = null, isFolder = false) {
    if (parentId !== null) ui.toggleFolder(parentId, true);
    creating = { parentId, isFolder };
    draft = '';
    await tick();
    inputEl?.focus();
  }
  async function startRename(id: number, name: string) {
    renamingId = id;
    draft = name;
    await tick();
    inputEl?.focus();
    inputEl?.select();
  }
  async function commit() {
    const name = draft.trim();
    if (creating) {
      const { parentId, isFolder } = creating;
      creating = null;
      if (name) {
        const p = await library.createPlaylist(name, parentId, isFolder);
        if (p && !isFolder) openLibrary(p.id);
      }
    } else if (renamingId !== null) {
      const id = renamingId;
      renamingId = null;
      if (name) await library.renamePlaylist(id, name);
    }
  }
  function cancel() {
    creating = null;
    renamingId = null;
  }
  function onkey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === 'Enter') commit();
    else if (e.key === 'Escape') cancel();
  }
  function openMenu(e: MouseEvent, p: Playlist) {
    e.preventDefault();
    const items: MenuItem[] = p.isFolder
      ? [
          { label: 'New playlist here', onselect: () => startCreate(p.id) },
          { label: 'New folder here', onselect: () => startCreate(p.id, true) },
          { divider: true },
          { label: 'Rename', onselect: () => startRename(p.id, p.name) },
          { divider: true },
          { label: 'Delete folder', hint: 'tracks stay in the collection', danger: true, onselect: () => library.deletePlaylist(p.id) },
        ]
      : [
          { label: 'Rename', onselect: () => startRename(p.id, p.name) },
          { divider: true },
          { label: 'Delete playlist', danger: true, onselect: () => library.deletePlaylist(p.id) },
        ];
    menu = { x: e.clientX, y: e.clientY, items };
  }
</script>

{#snippet createInput(depth: number)}
  <input
    class="edit"
    style="margin-left:{depth * 14}px"
    bind:this={inputEl}
    bind:value={draft}
    onkeydown={onkey}
    onblur={commit}
    placeholder={creating?.isFolder ? 'Folder name' : 'Playlist name'}
    maxlength="80"
    aria-label={creating?.isFolder ? 'New folder name' : 'New playlist name'}
  />
{/snippet}

<nav class="side" class:focused={library.focus === 'sidebar'} aria-label="Library sections" onpointerdown={() => (library.focus = 'sidebar')}>
  <button class="item" class:sel={inLibrary && library.playlistId === null} onclick={() => openLibrary(null)}>
    <Library size={14} strokeWidth={1.9} /><span class="name">Collection</span>
  </button>
  <button class="item" class:sel={ui.browserView === 'spotify'} onclick={openSpotify}>
    <AudioLines size={14} strokeWidth={1.9} /><span class="name">Spotify</span>
    <i class="led" data-state={spotify.connected ? 'ok' : 'off'} title={spotify.connected ? 'Connected' : 'Not connected'}></i>
  </button>
  <button class="item" class:sel={ui.browserView === 'history'} onclick={openHistory}>
    <History size={14} strokeWidth={1.9} /><span class="name">History</span>
    {#if history.recording}<i class="led" data-state="bad" title="Recording"></i>{:else if history.sets.some((s) => s.live)}<i class="led" data-state="ok" title="A set is being logged"></i>{/if}
  </button>
  <div class="group">
    <span class="tiny">Playlists</span>
    <button class="add" onclick={() => startCreate()} aria-label="New playlist" title="New playlist"><Plus size={14} strokeWidth={2.2} /></button>
  </div>
  <div class="list" role="tree" aria-label="Playlists">
    {#each library.rows as { playlist: p, depth } (p.id)}
      {@const open = p.isFolder && !ui.collapsedFolders.has(p.id)}
      {#if renamingId === p.id}
        <input class="edit" style="margin-left:{depth * 14}px" bind:this={inputEl} bind:value={draft} onkeydown={onkey} onblur={commit} maxlength="80" aria-label="Name" />
      {:else if p.isFolder}
        <button
          class="item folder"
          style="padding-left:{4 + depth * 14}px"
          role="treeitem"
          aria-selected="false"
          aria-expanded={open}
          onclick={() => ui.toggleFolder(p.id)}
          ondblclick={() => startRename(p.id, p.name)}
          oncontextmenu={(e) => openMenu(e, p)}
        >
          <span class="chev" class:open><ChevronRight size={12} strokeWidth={2.4} /></span>
          {#if open}<FolderOpen size={14} strokeWidth={1.9} />{:else}<Folder size={14} strokeWidth={1.9} />{/if}
          <span class="name">{p.name}</span>
        </button>
      {:else}
        <button
          class="item"
          style="padding-left:{20 + depth * 14}px"
          role="treeitem"
          aria-selected={inLibrary && library.playlistId === p.id}
          class:sel={inLibrary && library.playlistId === p.id}
          class:drop={dropTargets.playlist === p.id}
          data-drop-playlist={p.id}
          onclick={() => openLibrary(p.id)}
          ondblclick={() => startRename(p.id, p.name)}
          oncontextmenu={(e) => openMenu(e, p)}
        >
          <ListMusic size={14} strokeWidth={1.9} /><span class="name">{p.name}</span><span class="count num">{p.trackCount}</span>
        </button>
      {/if}
      {#if creating && creating.parentId === p.id}{@render createInput(createDepth)}{/if}
    {/each}
    {#if creating && creating.parentId === null}
      {@render createInput(0)}
    {:else if library.playlists.length === 0 && !creating}
      <p class="hint">Create a playlist and drag tracks onto it, or import a folder or playlist file.</p>
    {/if}
  </div>
</nav>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .side {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 8px 6px;
    min-height: 0;
    overflow-y: auto;
    border-right: 1px solid var(--line);
    background: var(--panel);
    position: relative;
  }
  .side.focused::after {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
    box-shadow: inset 0 0 0 1px var(--line-2);
  }
  .group {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 8px 4px;
  }
  .add {
    width: 22px;
    height: 22px;
    border-radius: 5px;
    border: 0;
    background: transparent;
    color: var(--dim);
    display: grid;
    place-items: center;
    transition: background var(--dur), color var(--dur);
  }
  .add:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--fg);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: none;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--dim);
    font-size: 12.5px;
    text-align: left;
    flex: none;
    transition: background var(--dur), color var(--dur);
  }
  .item:hover {
    background: rgba(255, 255, 255, 0.05);
    color: var(--fg);
  }
  .item.sel {
    background: var(--raised-3);
    color: var(--fg);
    font-weight: 560;
  }
  .focused .item.sel {
    background: color-mix(in srgb, var(--fg) 17%, var(--panel));
    box-shadow: inset 4px 0 0 var(--fg), inset 0 0 0 1px rgba(255, 255, 255, 0.38);
    font-weight: 660;
  }
  .item.drop {
    background: color-mix(in srgb, var(--ok) 18%, transparent);
    box-shadow: inset 0 0 0 1px var(--ok);
    color: var(--fg);
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .folder {
    gap: 6px;
  }
  .chev {
    display: grid;
    place-items: center;
    width: 12px;
    color: var(--faint);
    transition: transform var(--dur);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .count {
    font-size: 11px;
    color: var(--faint);
  }
  .edit {
    height: 28px;
    margin: 1px 0;
    padding: 0 8px;
    border-radius: var(--radius);
    border: 1px solid var(--ring);
    background: #09090b;
    outline: none;
    font-size: 12.5px;
  }
  .hint {
    margin: 6px 8px;
    font-size: 11.5px;
    line-height: 1.4;
    color: var(--faint);
  }
</style>
