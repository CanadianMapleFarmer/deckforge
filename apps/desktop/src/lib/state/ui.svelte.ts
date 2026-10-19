export const ZOOM_LEVELS = [24, 40, 64, 100, 160, 256] as const;

function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}
function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage unavailable */
  }
}

export type SettingsTab = 'audio' | 'controller' | 'recording';

class UiStore {
  zoomIndex = $state(load('df.zoom', 2));
  libraryHeight = $state(load('df.libraryHeight', 280));
  sidebarWidth = $state(load('df.sidebarWidth', 210));
  timeMode = $state<['elapsed' | 'remaining', 'elapsed' | 'remaining']>(load('df.timeMode', ['remaining', 'remaining']));
  jumpBeats = $state<[number, number]>(load('df.jumpBeats', [4, 4]));
  sort = $state<{ key: string; dir: 1 | -1 }>(load('df.sort', { key: 'addedAt', dir: -1 }));
  settingsOpen = $state(false);
  settingsTab = $state<SettingsTab>('audio');
  helpOpen = $state(false);
  browserView = $state<'library' | 'spotify' | 'history'>('library');
  collapsedFolders = $state.raw<ReadonlySet<number>>(new Set(load<number[]>('df.collapsedFolders', [])));

  compact = $state(false);

  watchViewport(): () => void {
    const mq = window.matchMedia('(max-height: 820px)');
    const sync = () => (this.compact = mq.matches);
    sync();
    mq.addEventListener('change', sync);
    return () => mq.removeEventListener('change', sync);
  }

  get pps(): number {
    return ZOOM_LEVELS[this.zoomIndex];
  }

  zoom(delta: number) {
    const i = Math.min(ZOOM_LEVELS.length - 1, Math.max(0, this.zoomIndex + delta));
    if (i !== this.zoomIndex) {
      this.zoomIndex = i;
      save('df.zoom', i);
    }
  }
  setLibraryHeight(h: number) {
    this.libraryHeight = h;
    save('df.libraryHeight', h);
  }
  toggleFolder(id: number, open?: boolean) {
    const next = new Set(this.collapsedFolders);
    if (open ?? next.has(id)) next.delete(id);
    else next.add(id);
    this.collapsedFolders = next;
    save('df.collapsedFolders', [...next]);
  }
  setSidebarWidth(w: number) {
    this.sidebarWidth = w;
    save('df.sidebarWidth', w);
  }
  toggleTime(deck: 0 | 1) {
    const next: ['elapsed' | 'remaining', 'elapsed' | 'remaining'] = [...this.timeMode] as typeof this.timeMode;
    next[deck] = next[deck] === 'elapsed' ? 'remaining' : 'elapsed';
    this.timeMode = next;
    save('df.timeMode', next);
  }
  setJump(deck: 0 | 1, beats: number) {
    const next: [number, number] = [...this.jumpBeats] as [number, number];
    next[deck] = beats;
    this.jumpBeats = next;
    save('df.jumpBeats', next);
  }
  setSort(key: string) {
    this.sort = this.sort.key === key ? { key, dir: this.sort.dir === 1 ? -1 : 1 } : { key, dir: 1 };
    save('df.sort', this.sort);
  }
  openSettings(tab: SettingsTab = 'audio') {
    this.settingsTab = tab;
    this.settingsOpen = true;
  }
}

export const ui = new UiStore();
