import { open } from '@tauri-apps/plugin-dialog';
import type { AnalysisProgress, ControllerAppAction, DeckIndex, Phrase, Playlist, PlaylistImportReport, TrackSummary } from '$lib/api';
import { EVENTS } from '$lib/api';
import * as be from '$lib/api/backend';
import { errorMessage } from '$lib/util/format';
import { engine } from './engine.svelte';
import { fxControl } from './fxcontrol.svelte';
import { toast } from './toast.svelte';
import { ui } from './ui.svelte';
import { waveforms } from '$lib/waveform/store';
import { effectiveKey } from '$lib/util/camelot';
import { visibleRows } from '$lib/util/playlistTree';

export type FocusPane = 'sidebar' | 'table';

export interface PhraseSet {
  token: number;
  phrases: Phrase[];
}

export interface ConfirmState {
  kind: 'load' | 'eject';
  deck: DeckIndex;
  trackId: number;
  title: string;
}

export interface ContextMenuState {
  x: number;
  y: number;
  trackId: number;
}

class Library {
  tracks = $state.raw<TrackSummary[]>([]);
  playlists = $state.raw<Playlist[]>([]);
  playlistId = $state<number | null>(null);
  query = $state('');
  selected = $state.raw<Set<number>>(new Set());
  cursorId = $state<number | null>(null);
  anchorId: number | null = null;
  focus = $state<FocusPane>('table');
  loading = $state(false);
  importing = $state(false);
  importReport = $state<PlaylistImportReport | null>(null);
  queued = $state(0);
  analysing = $state<number | null>(null);
  confirm = $state<ConfirmState | null>(null);
  menu = $state<ContextMenuState | null>(null);
  deckTracks = $state.raw<[TrackSummary | null, TrackSummary | null]>([null, null]);
  deckArt = $state.raw<[string | null, string | null]>([null, null]);
  deckPhrases = $state.raw<[PhraseSet | null, PhraseSet | null]>([null, null]);
  scrollToCursor = $state(0);

  rows = $derived(visibleRows(this.playlists, ui.collapsedFolders));

  private searchTimer: ReturnType<typeof setTimeout> | undefined;
  private deckTokens: [number, number] = [-1, -1];

  sorted = $derived.by(() => {
    const { key, dir } = ui.sort;
    const list = [...this.tracks];
    const val = (t: TrackSummary): string | number => {
      switch (key) {
        case 'title': return t.title.toLowerCase();
        case 'artist': return t.artist.toLowerCase();
        case 'bpm': return t.bpm ?? Number.POSITIVE_INFINITY;
        case 'key': return t.key ?? '~';
        case 'durationSecs': return t.durationSecs;
        case 'genre': return t.genre.toLowerCase();
        case 'fileType': return t.fileType;
        case 'analysis': return t.analysis;
        default: return t.addedAt;
      }
    };
    list.sort((a, b) => {
      const x = val(a), y = val(b);
      const c = typeof x === 'string' && typeof y === 'string' ? x.localeCompare(y, undefined, { numeric: true }) : (x as number) - (y as number);
      return (c || a.id - b.id) * dir;
    });
    return list;
  });

  indexById = $derived.by(() => {
    const m = new Map<number, number>();
    this.sorted.forEach((t, i) => m.set(t.id, i));
    return m;
  });

  masterKey = $derived.by(() => {
    const m = engine.snap.masterDeck;
    if (m === null) return null;
    const d = engine.snap.decks[m];
    const t = this.deckTracks[m];
    return d.loaded && t ? (effectiveKey(t.key, d.tempoRatio, d.keyLock)?.key ?? null) : null;
  });

  cursorTrack = $derived(this.cursorId === null ? null : (this.tracks.find((t) => t.id === this.cursorId) ?? null));

  async init(): Promise<() => void> {
    await Promise.all([this.refreshTracks(), this.refreshPlaylists()]);
    const offs = await Promise.all([
      be.on<null>(EVENTS.libraryChanged, () => {
        this.refreshTracks();
        this.refreshPlaylists();
      }),
      be.on<AnalysisProgress>(EVENTS.analysisProgress, (p) => this.onAnalysis(p)),
      be.on<ControllerAppAction>(EVENTS.controllerAction, (a) => this.onHardware(a)),
    ]);
    return () => offs.forEach((o) => o());
  }

  async refreshTracks() {
    try {
      this.loading = true;
      this.tracks = await be.backend.tracks(this.query, this.playlistId);
      if (this.cursorId === null && this.tracks.length) this.cursorId = this.sorted[0]?.id ?? null;
    } catch (e) {
      toast.error(`Could not load library: ${errorMessage(e)}`);
    } finally {
      this.loading = false;
    }
  }

  async refreshPlaylists() {
    try {
      this.playlists = await be.backend.playlists();
    } catch (e) {
      toast.error(`Could not load playlists: ${errorMessage(e)}`);
    }
  }

  setQuery(q: string) {
    this.query = q;
    clearTimeout(this.searchTimer);
    this.searchTimer = setTimeout(() => this.refreshTracks(), 180);
  }

  selectPlaylist(id: number | null) {
    this.playlistId = id;
    this.refreshTracks();
  }

  private onAnalysis(p: AnalysisProgress) {
    this.queued = p.queued;
    this.analysing = p.state === 'running' ? p.trackId : this.analysing === p.trackId ? null : this.analysing;
    const patch = (t: TrackSummary) => (t.id === p.trackId ? { ...t, analysis: p.state } : t);
    this.tracks = this.tracks.map(patch);
    const decks = this.deckTracks.map((t) => (t ? patch(t) : t)) as typeof this.deckTracks;
    this.deckTracks = decks;
    if (p.state === 'done') {
      waveforms.invalidate(p.trackId);
      ([0, 1] as const).forEach((deck) => {
        if (this.deckTokens[deck] === p.trackId) this.fetchPhrases(deck, p.trackId);
      });
      be.backend.track(p.trackId).then((fresh) => {
        this.tracks = this.tracks.map((t) => (t.id === fresh.id ? fresh : t));
        this.deckTracks = this.deckTracks.map((t) => (t?.id === fresh.id ? fresh : t)) as typeof this.deckTracks;
      }).catch(() => {});
    }
  }

  select(id: number, mode: 'single' | 'toggle' | 'range' = 'single') {
    this.focus = 'table';
    const list = this.sorted;
    if (mode === 'range' && this.anchorId !== null) {
      const a = this.indexById.get(this.anchorId) ?? 0;
      const b = this.indexById.get(id) ?? 0;
      const [lo, hi] = a < b ? [a, b] : [b, a];
      this.selected = new Set(list.slice(lo, hi + 1).map((t) => t.id));
    } else if (mode === 'toggle') {
      const next = new Set(this.selected);
      next.has(id) ? next.delete(id) : next.add(id);
      this.selected = next;
      this.anchorId = id;
    } else {
      this.selected = new Set([id]);
      this.anchorId = id;
    }
    this.cursorId = id;
  }

  moveCursor(delta: number) {
    const list = this.sorted;
    if (!list.length) return;
    const cur = this.cursorId === null ? -1 : (this.indexById.get(this.cursorId) ?? -1);
    const next = Math.min(list.length - 1, Math.max(0, cur + delta));
    this.select(list[next].id, 'single');
    this.scrollToCursor++;
  }

  moveSidebar(delta: number) {
    const lists = this.rows.filter((r) => !r.playlist.isFolder).map((r) => r.playlist.id);
    const ids: (number | null | 'spotify')[] = [null, ...lists, 'spotify'];
    const cur = ids.indexOf(ui.browserView === 'spotify' ? 'spotify' : this.playlistId);
    const next = Math.min(ids.length - 1, Math.max(0, cur + delta));
    if (next === cur) return;
    const target = ids[next];
    if (target === 'spotify') ui.browserView = 'spotify';
    else {
      ui.browserView = 'library';
      this.selectPlaylist(target);
    }
  }

  browse(delta: number) {
    this.focus === 'sidebar' ? this.moveSidebar(delta) : this.moveCursor(delta);
  }

  toggleFocus() {
    this.focus = this.focus === 'table' ? 'sidebar' : 'table';
  }

  private onHardware(a: ControllerAppAction) {
    if (a.type === 'browseScroll') this.browse(a.delta);
    else if (a.type === 'browseToggleFocus') this.toggleFocus();
    else if (a.type === 'loadSelected') this.loadSelected(a.deck);
    else if (a.type === 'hardwareKnob') engine.hardware = { ...engine.hardware, [a.name]: a.value };
    else if (a.type === 'fxControl') {
      const { type: _type, ...state } = a;
      fxControl.ingest(state);
    }
  }

  pickDeck(preferred?: DeckIndex): DeckIndex {
    if (preferred !== undefined) return preferred;
    const d = engine.snap.decks;
    if (!d[0].loaded) return 0;
    if (!d[1].loaded) return 1;
    if (!d[0].playing) return 0;
    if (!d[1].playing) return 1;
    return 0;
  }

  loadSelected(deck: DeckIndex) {
    if (this.cursorId === null) {
      toast.info('Select a track in the library first');
      return;
    }
    this.loadToDeck(deck, this.cursorId);
  }

  async loadToDeck(deck: DeckIndex, trackId: number, force = false) {
    try {
      await be.backend.loadTrack(deck, trackId, force);
      this.confirm = null;
    } catch (e) {
      const msg = errorMessage(e);
      if (msg.includes('DECK_PLAYING')) {
        const t = this.tracks.find((x) => x.id === trackId) ?? (await be.backend.track(trackId).catch(() => null));
        this.confirm = { kind: 'load', deck, trackId, title: t?.title ?? 'this track' };
      } else {
        toast.error(`Could not load track: ${msg}`);
      }
    }
  }

  confirmPending() {
    const c = this.confirm;
    if (!c) return;
    if (c.kind === 'eject') this.doEject(c.deck);
    else this.loadToDeck(c.deck, c.trackId, true);
  }

  eject(deck: DeckIndex) {
    const d = engine.snap.decks[deck];
    if (!d.loaded) return;
    if (d.playing) this.confirm = { kind: 'eject', deck, trackId: d.trackToken, title: this.deckTracks[deck]?.title ?? 'this track' };
    else this.doEject(deck);
  }

  private async doEject(deck: DeckIndex) {
    this.confirm = null;
    try {
      await be.backend.eject(deck);
    } catch (e) {
      toast.error(`Could not eject: ${errorMessage(e)}`);
    }
  }

  private async fetchPhrases(deck: DeckIndex, token: number) {
    try {
      const phrases = await be.backend.phrases(token);
      if (this.deckTokens[deck] !== token) return;
      const next = [...this.deckPhrases] as typeof this.deckPhrases;
      next[deck] = { token, phrases };
      this.deckPhrases = next;
    } catch {
      /* phrases are optional */
    }
  }

  async syncDeckInfo() {
    const snap = engine.snap;
    for (const deck of [0, 1] as const) {
      const token = snap.decks[deck].loaded ? snap.decks[deck].trackToken : 0;
      if (token === this.deckTokens[deck]) continue;
      this.deckTokens[deck] = token;
      const setTrack = (t: TrackSummary | null) => {
        const next = [...this.deckTracks] as typeof this.deckTracks;
        next[deck] = t;
        this.deckTracks = next;
      };
      const setArt = (u: string | null) => {
        const prev = this.deckArt[deck];
        if (prev && prev.startsWith('blob:')) URL.revokeObjectURL(prev);
        const next = [...this.deckArt] as typeof this.deckArt;
        next[deck] = u;
        this.deckArt = next;
      };
      const clearPhrases = () => {
        const next = [...this.deckPhrases] as typeof this.deckPhrases;
        next[deck] = null;
        this.deckPhrases = next;
      };
      clearPhrases();
      if (!token) {
        setTrack(null);
        setArt(null);
        continue;
      }
      this.fetchPhrases(deck, token);
      be.backend.track(token).then((t) => this.deckTokens[deck] === token && setTrack(t)).catch(() => {});
      be.backend.artwork(token).then((u) => (this.deckTokens[deck] === token ? setArt(u) : u && URL.revokeObjectURL(u))).catch(() => setArt(null));
    }
  }

  async importFolders(asPlaylists = false) {
    const title = asPlaylists ? 'Import folders as playlists' : 'Add music folders to the collection';
    const picked = await open({ directory: true, multiple: true, title }).catch(() => null);
    const paths = picked === null ? [] : Array.isArray(picked) ? picked : [picked];
    if (paths.length) await this.runImport(paths, asPlaylists);
  }

  async importPlaylistFile() {
    const picked = await open({
      multiple: true,
      title: 'Import playlist files',
      filters: [{ name: 'Playlists', extensions: ['m3u', 'm3u8', 'xml'] }],
    }).catch(() => null);
    const paths = picked === null ? [] : Array.isArray(picked) ? picked : [picked];
    if (paths.length) await this.runImport(paths, true);
  }

  private async runImport(paths: string[], asPlaylists: boolean) {
    const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;
    try {
      this.importing = true;
      if (asPlaylists) {
        const r = await be.backend.importPlaylists(paths);
        if (r.playlists.some((p) => p.missing.length)) this.importReport = r;
        else toast.ok(`Imported ${plural(r.playlists.length, 'playlist')} · ${plural(r.added, 'new track')}${r.failed ? ` · ${r.failed} failed` : ''}`);
      } else {
        const r = await be.backend.importPaths(paths);
        toast.ok(`Imported ${plural(r.added, 'track')}${r.skipped ? `, ${r.skipped} already in library` : ''}${r.failed ? `, ${r.failed} failed` : ''}`);
      }
      await this.refreshTracks();
      await this.refreshPlaylists();
    } catch (e) {
      toast.error(`Import failed: ${errorMessage(e)}`);
    } finally {
      this.importing = false;
    }
  }

  async createPlaylist(name: string, parentId: number | null = null, isFolder = false) {
    try {
      const p = await be.backend.createPlaylist(name, parentId, isFolder);
      await this.refreshPlaylists();
      return p;
    } catch (e) {
      toast.error(errorMessage(e));
      return null;
    }
  }

  async renamePlaylist(id: number, name: string) {
    await be.backend.renamePlaylist(id, name).catch((e) => toast.error(errorMessage(e)));
    await this.refreshPlaylists();
  }

  async deletePlaylist(id: number) {
    await be.backend.deletePlaylist(id).catch((e) => toast.error(errorMessage(e)));
    await this.refreshPlaylists();
    if (this.playlistId !== null && !this.playlists.some((p) => p.id === this.playlistId)) this.selectPlaylist(null);
  }

  async addToPlaylist(playlistId: number, trackIds: number[]) {
    try {
      await be.backend.addToPlaylist(playlistId, trackIds);
      const name = this.playlists.find((p) => p.id === playlistId)?.name ?? 'playlist';
      toast.ok(`Added ${trackIds.length} track${trackIds.length === 1 ? '' : 's'} to ${name}`);
      this.refreshPlaylists();
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }

  async removeFromCurrentPlaylist(trackId: number) {
    if (this.playlistId === null) return;
    await be.backend.removeFromPlaylist(this.playlistId, trackId).catch((e) => toast.error(errorMessage(e)));
    this.refreshTracks();
    this.refreshPlaylists();
  }

  reanalyze(trackId: number) {
    be.backend.reanalyze(trackId).then(() => toast.info('Analysis queued')).catch((e) => toast.error(errorMessage(e)));
  }
}

export const library = new Library();
