import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  ActiveOutput, AnalysisProgress, ControllerAppAction, ControllerStatus, DeckIndex, EngineSnapshot,
  ImportResult, LoadResult, PlaylistImportReport, PlaylistImportSummary, MidiLogEntry, OutputDeviceInfo, OutputRequest, OutputStatus, Playlist,
  Phrase, RecorderStatus, RecordingSettings, Routing, SecondaryOutputRequest, SetDetail, SetSummary, ExportFormat, SecondaryOutputStatus, SpotifyImportResult, SpotifyPlaylist, SpotifyStatus, SpotifyTrack, TrackSummary, UiCommand,
  Waveform,
} from './types';

export * from './types';

export const EVENTS = {
  engineState: 'engine://state', // EngineSnapshot, ~60 Hz
  libraryChanged: 'library://changed', // null
  analysisProgress: 'analysis://progress', // AnalysisProgress
  controllerStatus: 'controller://status', // ControllerStatus, 1 Hz
  controllerAction: 'controller://app-action', // ControllerAppAction
  audioStatus: 'audio://status', // OutputStatus, 1 Hz
  historyChanged: 'history://changed', // null
  recordingStatus: 'recording://status', // RecorderStatus | null, 1 Hz
} as const;

export const api = {
  engine: (cmd: UiCommand) => invoke<void>('engine_command', { cmd }),
  snapshot: () => invoke<EngineSnapshot>('engine_snapshot'),

  /** Rejects with "DECK_PLAYING" when the deck is playing and `force` is not set. */
  loadTrack: (deck: DeckIndex, trackId: number, force = false) =>
    invoke<LoadResult>('load_track', { deck, trackId, force }),
  eject: (deck: DeckIndex) => invoke<void>('eject', { deck }),

  tracks: (query = '', playlistId: number | null = null) =>
    invoke<TrackSummary[]>('library_tracks', { query, playlistId }),
  track: (id: number) => invoke<TrackSummary>('library_track', { id }),
  importPaths: (paths: string[]) => invoke<ImportResult>('library_import', { paths }),
  importPlaylists: (paths: string[]) => invoke<PlaylistImportReport>('library_import_playlists', { paths }),
  playlists: () => invoke<Playlist[]>('playlists_list'),
  createPlaylist: (name: string, parentId: number | null = null, isFolder = false) =>
    invoke<Playlist>('playlist_create', { name, parentId, isFolder }),
  renamePlaylist: (id: number, name: string) => invoke<void>('playlist_rename', { id, name }),
  deletePlaylist: (id: number) => invoke<void>('playlist_delete', { id }),
  addToPlaylist: (id: number, trackIds: number[]) => invoke<void>('playlist_add_tracks', { id, trackIds }),
  removeFromPlaylist: (id: number, trackId: number) => invoke<void>('playlist_remove_track', { id, trackId }),
  reanalyze: (trackId: number) => invoke<void>('track_analyze', { trackId }),
  phrases: (trackId: number) => invoke<Phrase[]>('track_phrases', { trackId }),
  setBeatgrid: (trackId: number, bpm: number, firstBeatSecs: number) =>
    invoke<void>('track_set_beatgrid', { trackId, bpm, firstBeatSecs }),

  async waveform(trackId: number): Promise<Waveform | null> {
    const buf = await invoke<ArrayBuffer | null>('track_waveform', { trackId });
    return buf && buf.byteLength >= 12 ? decodeWaveform(buf) : null;
  },
  async artwork(trackId: number): Promise<string | null> {
    const buf = await invoke<ArrayBuffer | null>('track_artwork', { trackId });
    return buf && buf.byteLength ? URL.createObjectURL(new Blob([buf])) : null;
  },

  audioDevices: () => invoke<OutputDeviceInfo[]>('audio_devices'),
  audioOpen: (request: OutputRequest) => invoke<ActiveOutput>('audio_open', { request }),
  audioStatus: () => invoke<OutputStatus>('audio_status'),
  audioSetRouting: (routing: Routing) => invoke<void>('audio_set_routing', { routing }),
  /** `null` turns the additional output off. Rejects when the device is the main output. */
  audioSetSecondary: (request: SecondaryOutputRequest | null) =>
    invoke<OutputStatus>('audio_set_secondary', { request }),
  audioSetSecondaryLevel: (level: number) => invoke<void>('audio_set_secondary_level', { level }),

  controllerStatus: () => invoke<ControllerStatus>('controller_status'),
  midiLog: () => invoke<MidiLogEntry[]>('midi_log'),
  midiClearLog: () => invoke<void>('midi_clear_log'),
  /** Simulate controller input (diagnostics panel / testing without hardware). */
  midiInject: (bytes: number[]) => invoke<void>('midi_inject', { bytes }),

  spotifyStatus: () => invoke<SpotifyStatus>('spotify_status'),
  spotifySetClientId: (clientId: string) => invoke<SpotifyStatus>('spotify_set_client_id', { clientId }),
  /** Opens the system browser for Spotify OAuth (PKCE) and resolves once the callback completes. */
  spotifyLogin: () => invoke<SpotifyStatus>('spotify_login'),
  spotifyLogout: () => invoke<SpotifyStatus>('spotify_logout'),
  spotifyPlaylists: () => invoke<SpotifyPlaylist[]>('spotify_playlists'),
  /** `playlistId` = "liked" for the user's saved tracks. */
  spotifyPlaylistTracks: (playlistId: string) => invoke<SpotifyTrack[]>('spotify_playlist_tracks', { playlistId }),
  spotifyImportPlaylist: (playlistId: string) =>
    invoke<SpotifyImportResult>('spotify_import_playlist', { playlistId }),

  historySets: () => invoke<SetSummary[]>('history_sets'),
  historySet: (id: number) => invoke<SetDetail>('history_set', { id }),
  historyRename: (id: number, name: string) => invoke<void>('history_rename', { id, name }),
  /** Rejects while the set is being recorded. Never deletes recordings from disk. */
  historyDelete: (id: number) => invoke<void>('history_delete', { id }),
  historyRemoveEntry: (entryId: number) => invoke<void>('history_remove_entry', { entryId }),
  historyTracklist: (id: number) => invoke<string>('history_tracklist', { id }),
  historyExport: (id: number, format: ExportFormat, path: string) => invoke<void>('history_export', { id, format, path }),
  historyToPlaylist: (id: number) => invoke<number>('history_to_playlist', { id }),
  /** Returns how many recordings had their tracklist and marker files rewritten. */
  historyRewriteRecordingFiles: (id: number) => invoke<number>('history_rewrite_recording_files', { id }),
  recordingStart: () => invoke<RecorderStatus>('recording_start'),
  /** Resolves with the finished file's path, or null when nothing was recording. */
  recordingStop: () => invoke<string | null>('recording_stop'),
  recordingStatus: () => invoke<RecorderStatus | null>('recording_status'),
  recordingSettings: () => invoke<RecordingSettings>('recording_settings'),
  recordingSetSettings: (settings: RecordingSettings) => invoke<RecordingSettings>('recording_set_settings', { settings }),
  openPath: (path: string) => invoke<void>('open_path', { path }),
};

export function decodeWaveform(buf: ArrayBuffer): Waveform {
  const view = new DataView(buf);
  const n = view.getUint32(0, true);
  const pointsPerSec = view.getFloat64(4, true);
  const body = new Uint8Array(buf, 12);
  return {
    pointsPerSec,
    peak: body.subarray(0, n),
    low: body.subarray(n, 2 * n),
    mid: body.subarray(2 * n, 3 * n),
    high: body.subarray(3 * n, 4 * n),
  };
}

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}

export type {
  ActiveOutput, AnalysisProgress, ControllerAppAction, ControllerStatus, EngineSnapshot, ImportResult,
  LoadResult, MidiLogEntry, OutputDeviceInfo, OutputStatus, Playlist, PlaylistImportReport, PlaylistImportSummary, SecondaryOutputRequest,
  SecondaryOutputStatus, TrackSummary,
};
