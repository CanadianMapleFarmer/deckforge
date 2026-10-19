import { save } from '@tauri-apps/plugin-dialog';
import type { ExportFormat, RecorderStatus, RecordingSettings, SetDetail, SetSummary } from '$lib/api';
import { EVENTS } from '$lib/api';
import * as be from '$lib/api/backend';
import { errorMessage } from '$lib/util/format';
import { library } from './library.svelte';
import { toast } from './toast.svelte';
import { ui } from './ui.svelte';

export const EXPORT_FORMATS: { format: ExportFormat; label: string; ext: string }[] = [
  { format: 'text', label: 'Tracklist (.txt)', ext: 'txt' },
  { format: 'csv', label: 'Spreadsheet (.csv)', ext: 'csv' },
  { format: 'cue', label: 'CUE sheet (.cue)', ext: 'cue' },
  { format: 'fcpXml', label: 'Video editor markers (.xml)', ext: 'xml' },
];

export function folderOf(path: string): string {
  return path.replace(/[\\/][^\\/]*$/, '');
}

class HistoryStore {
  sets = $state<SetSummary[]>([]);
  openId = $state<number | null>(null);
  detail = $state<SetDetail | null>(null);
  recording = $state<RecorderStatus | null>(null);
  settings = $state<RecordingSettings | null>(null);
  busy = $state(false);

  async init(): Promise<() => void> {
    await Promise.all([this.refresh(), this.refreshRecording()]);
    const offs = await Promise.all([
      be.on<null>(EVENTS.historyChanged, () => void this.refresh()),
      be.on<RecorderStatus | null>(EVENTS.recordingStatus, (s) => this.onStatus(s)),
    ]);
    return () => offs.forEach((o) => o());
  }

  private onStatus(s: RecorderStatus | null) {
    if (s?.warning && s.warning !== this.recording?.warning) toast.error(s.warning);
    this.recording = s && !s.finished ? s : null;
  }

  async refresh() {
    try {
      this.sets = await be.backend.historySets();
      if (this.openId === null || !this.sets.some((s) => s.id === this.openId)) this.openId = this.sets[0]?.id ?? null;
      this.detail = this.openId === null ? null : await be.backend.historySet(this.openId);
    } catch (e) {
      toast.error(`Couldn't load the set history: ${errorMessage(e)}`);
    }
  }

  async refreshRecording() {
    this.recording = await be.backend.recordingStatus().catch(() => null);
  }

  async open(id: number) {
    this.openId = id;
    this.detail = await be.backend.historySet(id).catch(() => null);
  }

  async rename(id: number, name: string) {
    await this.run(() => be.backend.historyRename(id, name.trim()));
  }

  async remove(id: number) {
    if (await this.run(() => be.backend.historyDelete(id))) toast.ok('Set deleted. Recordings stay on disk.');
  }

  async removeEntry(entryId: number) {
    await this.run(() => be.backend.historyRemoveEntry(entryId));
  }

  async copyTracklist(id: number) {
    try {
      const text = await be.backend.historyTracklist(id);
      if (!text) return toast.info('No tracks took over in this set yet.');
      await navigator.clipboard.writeText(text);
      toast.ok('Tracklist copied');
    } catch (e) {
      toast.error(`Couldn't copy the tracklist: ${errorMessage(e)}`);
    }
  }

  async export(id: number, format: ExportFormat) {
    const spec = EXPORT_FORMATS.find((f) => f.format === format)!;
    const name = this.sets.find((s) => s.id === id)?.name.replace(/[\\/:*?"<>|]/g, '.') ?? 'set';
    const path = be.inTauri ? await save({ defaultPath: `${name}.${spec.ext}`, filters: [{ name: spec.label, extensions: [spec.ext] }] }).catch(() => null) : `${name}.${spec.ext}`;
    if (!path) return;
    if (await this.run(() => be.backend.historyExport(id, format, path))) toast.ok(`Exported ${spec.label.split(' (')[0].toLowerCase()}`);
  }

  async toPlaylist(id: number) {
    const playlistId = await this.run(() => be.backend.historyToPlaylist(id));
    if (playlistId === undefined) return;
    toast.ok('Playlist created from the set');
    ui.browserView = 'library';
    library.selectPlaylist(playlistId);
  }

  async rewriteFiles(id: number) {
    const n = await this.run(() => be.backend.historyRewriteRecordingFiles(id));
    if (n !== undefined) toast.ok(n ? `Rewrote the tracklist beside ${n} recording${n === 1 ? '' : 's'}` : 'No finished recordings to update');
  }

  async openFolder(path: string) {
    await this.run(() => be.backend.openPath(folderOf(path)));
  }

  showTrack(trackId: number) {
    ui.browserView = 'library';
    library.setQuery('');
    library.selectPlaylist(null);
    library.select(trackId);
  }

  async toggleRecording() {
    if (this.busy) return;
    this.busy = true;
    try {
      if (this.recording) {
        const path = await be.backend.recordingStop();
        this.recording = null;
        if (path) toast.ok(`Recording saved to ${folderOf(path)}`);
      } else {
        this.recording = await be.backend.recordingStart();
        if (this.recording.warning) toast.info(this.recording.warning);
      }
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      this.busy = false;
    }
  }

  async loadSettings() {
    this.settings = await be.backend.recordingSettings().catch(() => null);
  }

  async saveSettings(next: RecordingSettings) {
    this.settings = next;
    const saved = await this.run(() => be.backend.recordingSetSettings(next));
    if (saved) this.settings = saved;
  }

  private async run<T>(f: () => Promise<T>): Promise<T | undefined> {
    try {
      return await f();
    } catch (e) {
      toast.error(errorMessage(e));
      return undefined;
    }
  }
}

export const history = new HistoryStore();
