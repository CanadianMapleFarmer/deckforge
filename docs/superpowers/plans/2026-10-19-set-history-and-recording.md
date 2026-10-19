# Set History and Set Recording Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Log every set (which tracks took over, when) independent of recording, and record the mix losslessly with a tracklist and editor markers beside the file.

**Architecture:** A pure `SetTracker` in the library crate turns per-snapshot deck observations into set events, which the desktop state pump stores in new SQLite tables. The engine gains a media clock and a record tap that hands the pre-master-volume, limited mix to a lock-free ring through the command queue; a new `deckforge-recorder` crate drains it on a writer thread into WAV/RF64 or FLAC. Export formats are pure functions over a set's entries, shared by History export and recording side files.

**Tech Stack:** Rust 2024, rusqlite 0.40, rtrb 0.4, flacenc 0.5, rubato 0.16, fs4 0.13, Tauri 2, Svelte 5.

**Spec:** `docs/superpowers/specs/2026-10-19-set-history-and-recording-design.md`

## Global Constraints

- History never depends on recording; recording uses the set's history.
- Take-over: the moment every other deck is fully faded out (gain to master below −60 dB, or paused) while the new deck is audible, whatever its level. Bars and loops play no part.
- A deck counts as audible again only after staying audible for one beat at its BPM, or 0.5 s without a grid.
- A set ends on app close or 30 min with nothing audible; never while recording.
- Recording = mix after trim/EQ/filter/faders/crossfader/FX, before the master volume knob, through `soft_limit`.
- Formats: WAV (default) or FLAC; 16 or 24-bit (default 24); 44.1 or 48 kHz (default 48). Default folder `<Music>/deckforge Recordings`. File name `YYYY-MM-DD HH.MM.wav|.flac`.
- The audio thread never blocks, allocates, locks or touches disk for recording; taps are installed through the command queue, not `rt.lock()`.
- Recordings never stop at 4 GB (RF64 promotion).
- Zero code comments by default; never run `cargo fmt`; music files are only read; commit trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

- App closed or crashed mid-set and mid-recording: the set is closed on next start, the audio file is playable up to the last header patch (≤2 s lost).
- Recording folder unwritable or disk filling up: start fails with a message or recording stops cleanly; live audio unaffected.
- Output device switched to 44.1 kHz mid-recording: the file stays at its chosen rate and length, track times stay aligned.
- Track with empty artist/title, or removed from the library after the set: history keeps its snapshot; tracklist falls back to the file name.
- Record pressed before anything is audible and stopped mid-blend: a set exists from the press, the playing track is at 00:00, an unfinished blend that took over is listed.

---

### Task 1: SetTracker (pure take-over logic)

**Files:**
- Create: `crates/library/src/history/mod.rs`, `crates/library/src/history/tracker.rs`
- Modify: `crates/library/src/lib.rs` (add `pub mod history;`)
- Test: `crates/library/src/history/tracker.rs` (`#[cfg(test)] mod tests`)

**Interfaces — Produces:**
```rust
pub struct DeckObservation { pub track_id: i64 /* 0 = empty */, pub playing: bool, pub gain: f32 /* linear gain to master */, pub bpm: Option<f64> }
pub struct Observation { pub clock_secs: f64, pub decks: [DeckObservation; 2] }
pub enum TrackerEvent {
    SetStarted { at: f64 },
    Audible { deck: usize, track_id: i64, at: f64 },       // a new entry (new load or flip-back) became audible
    TookOver { deck: usize, track_id: i64, at: f64 },      // that entry took over
    SetEnded { at: f64 },
}
pub struct SetTracker; // new(), observe(&Observation) -> impl Iterator<Item = TrackerEvent> (SmallVec/array, no alloc not required), force_start(at), end(at) -> Option<TrackerEvent>, in_set() -> bool, set_recording(bool)
pub const SILENT_GAIN: f32 = 0.001; // −60 dB
pub const SET_IDLE_SECS: f64 = 1800.0;
```
Entry identity: one entry per (deck, load). A load ends when `track_id` changes. After a take-over, the deck that was replaced starts a new entry if it becomes audible again (flip-back). Re-audible hold: one beat (`60/bpm`) or 0.5 s.

- [ ] **Step 1: Write the failing tests**
  - `a_straight_blend_takes_over_when_the_old_deck_is_down`: A audible at 0; B audible at 60 (gain 0.3), A gain → 0 at 75 ⇒ events `SetStarted@0, Audible A@0, TookOver A@0, Audible B@60, TookOver B@75`.
  - `a_quiet_fader_still_takes_over`: B at gain 0.5 (fader ~70 %) when A pauses at 90 ⇒ `TookOver B@90`.
  - `looping_or_jumping_within_a_load_adds_nothing`: track_id constant, gain constant for 300 s ⇒ no extra events.
  - `a_tease_then_real_mix_is_one_entry`: B audible 10–12, silent, audible again 20, A down at 25 ⇒ one `Audible B@10`, `TookOver B@25`.
  - `a_flip_back_is_a_new_entry`: B took over at 30; A (same load) audible again at 40 for > 1 beat and takes over at 50 ⇒ `Audible A@40, TookOver A@50`.
  - `a_wobble_shorter_than_a_beat_is_ignored`: after `TookOver B`, A's gain blips above silent for 0.2 s at 128 BPM ⇒ no event.
  - `headphone_preview_never_appears`: deck playing with gain 0 ⇒ no events, no set.
  - `thirty_minutes_of_silence_ends_the_set_but_not_while_recording`: silence from 100 to 1901 ⇒ `SetEnded@100`... (the end time is the last audible moment); with `set_recording(true)` ⇒ no end.
  - `force_start_opens_a_set_before_anything_is_audible`: `force_start(5)` ⇒ `SetStarted@5`; first audible track at 9 then has `Audible@9, TookOver@9`.
- [ ] **Step 2:** `cargo test -p deckforge-library history::tracker` → FAIL (module missing).
- [ ] **Step 3:** Implement `SetTracker` in `tracker.rs`.
- [ ] **Step 4:** Same command → PASS.
- [ ] **Step 5:** Commit `feat(library): add the set tracker`.

### Task 2: History storage

**Files:**
- Modify: `crates/library/src/schema.rs` (migration 6), `crates/library/src/lib.rs` (methods, re-exports)
- Create: `crates/library/src/history/store.rs`
- Test: `crates/library/src/history/store.rs` tests

Migration 6:
```sql
CREATE TABLE sets (id INTEGER PRIMARY KEY, name TEXT, started_at INTEGER NOT NULL, ended_at INTEGER, clock_start REAL NOT NULL);
CREATE TABLE set_entries (id INTEGER PRIMARY KEY, set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
  track_id INTEGER, title TEXT NOT NULL, artist TEXT NOT NULL, path TEXT NOT NULL, key TEXT, bpm REAL,
  audible_secs REAL NOT NULL, took_over_secs REAL, removed INTEGER NOT NULL DEFAULT 0);
CREATE INDEX set_entries_set ON set_entries(set_id, audible_secs);
CREATE TABLE recordings (id INTEGER PRIMARY KEY, set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
  path TEXT NOT NULL, started_secs REAL NOT NULL, stopped_secs REAL, format TEXT NOT NULL);
```
`started_at`/`ended_at` are Unix ms; `*_secs` are set-relative seconds (`clock - clock_start`). `track_id` uses no FK so history survives library deletes.

**Interfaces — Produces (on `Library`):**
```rust
pub fn begin_set(&self, started_at_ms: i64, clock_start: f64) -> Result<i64>;
pub fn add_set_entry(&self, set_id: i64, track_id: i64, audible_secs: f64) -> Result<i64>; // snapshots title/artist/path/key/bpm
pub fn mark_took_over(&self, entry_id: i64, secs: f64) -> Result<()>;
pub fn end_set(&self, set_id: i64, ended_at_ms: i64) -> Result<()>;
pub fn close_open_sets(&self) -> Result<usize>; // ended_at = started_at + last entry secs
pub fn sets(&self) -> Result<Vec<SetSummary>>;           // newest first
pub fn set_detail(&self, id: i64) -> Result<SetDetail>;
pub fn rename_set(&self, id: i64, name: &str) -> Result<()>;
pub fn delete_set(&self, id: i64) -> Result<()>;
pub fn remove_set_entry(&self, entry_id: i64) -> Result<()>;
pub fn set_to_playlist(&self, id: i64) -> Result<i64>;   // took-over entries with a track_id, in order
pub fn add_recording(&self, set_id: i64, path: &str, started_secs: f64, format: &str) -> Result<i64>;
pub fn finish_recording(&self, id: i64, stopped_secs: f64) -> Result<()>;
// serde camelCase:
pub struct SetSummary { id, name: String /* default "YYYY-MM-DD HH:MM" */, started_at: i64, ended_at: Option<i64>, duration_secs: f64, track_count: i64, has_recording: bool, live: bool }
pub struct SetEntry { id, track_id: Option<i64>, title, artist, path, key: Option<String>, bpm: Option<f64>, audible_secs: f64, took_over_secs: Option<f64>, teased: bool }
pub struct SetRecording { id, path, started_secs: f64, stopped_secs: Option<f64>, format: String }
pub struct SetDetail { summary: SetSummary, entries: Vec<SetEntry>, recordings: Vec<SetRecording> }
```
`teased` = `took_over_secs IS NULL` and the set has ended or a later entry took over. Removed entries are excluded everywhere. Default name uses local time.

- [ ] **Step 1: Write the failing tests:** `a_set_round_trips_with_its_entries_in_order`, `entries_keep_their_snapshot_after_the_track_is_deleted`, `close_open_sets_ends_a_crashed_set_at_its_last_entry`, `rename_delete_and_remove_entry`, `set_to_playlist_uses_took_over_tracks_in_order`, `deleting_a_set_keeps_nothing_but_never_touches_files` (recordings rows gone; no fs access), `migration_6_applies_to_an_existing_v5_database`.
- [ ] **Step 2:** `cargo test -p deckforge-library history::store` → FAIL.
- [ ] **Step 3:** Implement the migration and methods.
- [ ] **Step 4:** → PASS; `cargo test -p deckforge-library` all green.
- [ ] **Step 5:** Commit `feat(library): store sets, their entries and recordings`.

### Task 3: Tracklist and marker exports

**Files:**
- Create: `crates/library/src/history/export.rs`
- Test: same file

**Interfaces — Produces:**
```rust
pub enum ExportFormat { Text, Csv, Cue, FcpXml }
pub struct ExportItem { pub at_secs: f64, pub blend_secs: Option<f64>, pub artist: String, pub title: String, pub length_secs: Option<f64> }
/// Took-over entries relative to `origin_secs`, ending at `end_secs`: the entry playing at the origin is clamped to 0, entries fully replaced before it are dropped.
pub fn export_items(entries: &[SetEntry], origin_secs: f64, end_secs: f64) -> Vec<ExportItem>;
pub fn render(format: ExportFormat, items: &[ExportItem], title: &str, audio_file: Option<&str>, frame_rate: u32) -> String;
pub fn format_time(secs: f64) -> String; // "04:12", "1:02:03"
```
Text line: `MM:SS Artist – Title` (en dash; `H:MM:SS` past an hour; empty artist ⇒ title only; empty both ⇒ file stem). CSV header `time,artist,title,blend_start,length`. CUE: `PERFORMER`, `TITLE`, `FILE "<audio>" WAVE`, `TRACK nn AUDIO`, `INDEX 01 MM:SS:FF` (75 fps). FCP XML: FCP7 `xmeml` v5 sequence with one `<marker>` per item (`<in>` in frames at `frame_rate`, `<name>Artist – Title</name>`), importable by DaVinci Resolve and Premiere Pro.

- [ ] **Step 1: Write the failing tests:** `text_tracklist_counts_from_zero` (exact string for three items incl. a 1:02:03 entry), `the_track_playing_at_the_origin_is_listed_at_zero`, `entries_replaced_before_the_origin_are_dropped`, `missing_metadata_falls_back_to_the_file_name`, `csv_quotes_commas_and_quotes`, `cue_indexes_use_75_frames_per_second`, `fcp_xml_is_well_formed_with_one_marker_per_item` (parse with quick-xml, check marker `in` = round(secs × 25)).
- [ ] **Step 2:** `cargo test -p deckforge-library history::export` → FAIL.
- [ ] **Step 3:** Implement; XML text escaped.
- [ ] **Step 4:** → PASS.
- [ ] **Step 5:** Commit `feat(library): export set tracklists and editor markers`.

### Task 4: Engine media clock and record tap

**Files:**
- Create: `crates/engine/src/record.rs`
- Modify: `crates/engine/src/{lib.rs,engine.rs,mixer.rs,command.rs,snapshot.rs}`
- Test: `crates/engine/tests/record.rs`

**Interfaces — Produces:**
```rust
pub enum RecordFrame { Audio([f32; 2]), Gap(u32), Rate(u32) }
pub struct RecordTap; // producer + Arc<RecordShared>
pub struct RecordShared; // start_clock() -> Option<f64>, dropped() -> u64
pub fn record_channel(capacity: usize) -> (RecordTap, rtrb::Consumer<RecordFrame>, Arc<RecordShared>);
pub const RECORD_RING_FRAMES: usize = 4 * 48_000;
Command::SetRecordTap(Option<Box<RecordTap>>)       // old tap goes out as Garbage::RecordTap
EngineSnapshot.clock_secs: f64                      // accumulated callback durations, survives rate changes
```
Mixer keeps `rec_l/rec_r` = `soft_limit(master sum)` before the master gain. The tap pushes `Rate(sr)` first and after any `reconfigure`, `Gap(n)` before resuming after drops, records `start_clock` at its first frame.

- [ ] **Step 1: Write the failing tests:** `the_recording_ignores_the_master_knob` (master 0.2 vs 1.0 ⇒ identical recorded frames), `the_recording_includes_fader_eq_and_crossfader`, `two_loud_decks_are_limited_not_clipped` (all |x| ≤ 1), `installing_a_tap_takes_no_lock_and_reports_its_start_clock` (start_clock == snapshot clock at the first recorded block), `a_full_ring_reports_a_gap_instead_of_blocking`, `a_rate_change_is_marked_in_the_stream`, `clock_secs_survives_a_rate_change`.
- [ ] **Step 2:** `cargo test -p deckforge-engine --test record` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** → PASS; `cargo test -p deckforge-engine` green (soak included).
- [ ] **Step 5:** Commit `feat(engine): add a media clock and a lock-free record tap`.

### Task 5: Recorder crate (writers and writer thread)

**Files:**
- Create: `crates/recorder/{Cargo.toml,src/lib.rs,src/wav.rs,src/flac.rs,src/writer.rs}`; add to workspace and `deckforge-recorder` workspace dep
- Test: unit tests in each file

**Interfaces — Produces:**
```rust
#[serde(rename_all = "camelCase")] pub struct RecordingSettings { pub folder: PathBuf, pub format: AudioFormat /* Wav|Flac */, pub bits: u16 /* 16|24 */, pub sample_rate: u32 /* 44100|48000 */, pub markers: MarkerFormats { csv: bool, cue: bool, fcp_xml: bool } }
impl Default for RecordingSettings // <Music>/deckforge Recordings, Wav, 24, 48000, all markers off
pub struct Recorder; // start(settings, consumer, shared, now_local) -> Result<Recorder, RecordError>; status() -> RecorderStatus; stop() -> Result<RecordingResult>
#[serde(rename_all = "camelCase")] pub struct RecorderStatus { pub path: String, pub elapsed_secs: f64, pub bytes: u64, pub dropped_secs: f64, pub warning: Option<String> }
pub struct RecordingResult { pub path: PathBuf, pub start_clock: f64, pub duration_secs: f64 }
pub enum RecordError { Folder(String), DiskSpace { free_minutes: u64 }, Io(String) }
pub fn min_free_bytes_for_an_hour(settings: &RecordingSettings) -> u64;
```
WAV writer: RIFF header with a 28-byte `JUNK` chunk reserved for `ds64`; patches sizes every 2 s; promotes to RF64 when data > `u32::MAX − 1 MiB` (threshold injectable for tests). FLAC: `flacenc` fixed 4096-frame blocks streamed, STREAMINFO patched on stop. `Gap(n)` writes n frames of silence; `Rate` (≠ target) resamples with `rubato::SincFixedIn`. Free space via `fs4::available_space`: refuse to start under an hour; stop cleanly with a warning under 200 MiB.

- [ ] **Step 1: Write the failing tests:** `wav_round_trips_24_and_16_bit` (decode with hound), `the_header_is_valid_while_recording` (read file mid-recording ⇒ hound opens it with ≥ the frames patched), `rf64_promotion_past_the_limit` (threshold 64 KiB ⇒ file starts `RF64`, `ds64` sizes correct, symphonia decodes all frames), `flac_round_trips_bit_exact` (symphonia), `gaps_are_filled_with_silence_to_keep_sync`, `a_44k1_stream_is_resampled_to_48k_with_the_right_length` (±1 block), `an_unwritable_folder_fails_to_start`, `file_name_is_the_local_date_and_time`.
- [ ] **Step 2:** `cargo test -p deckforge-recorder` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** → PASS; clippy clean.
- [ ] **Step 5:** Commit `feat(recorder): write recordings to WAV/RF64 and FLAC off the audio thread`.

### Task 6: Desktop wiring (history service, recording, commands)

**Files:**
- Create: `apps/desktop/src-tauri/src/history.rs`, `apps/desktop/src-tauri/src/recording.rs`
- Modify: `apps/desktop/src-tauri/src/{lib.rs,commands.rs}`, `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tests/contract.rs`
- Test: `apps/desktop/src-tauri/src/history.rs` unit tests, `tests/contract.rs`

Behaviour:
- `HistoryService::observe(&EngineSnapshot, &Library)` builds `Observation` (gain = `channel_fader_gain(fader) × crossfader_gains(xf, curve)[side]` for the deck's side; playing = `playing || jog_touched`), feeds the tracker, persists events, emits `history://changed`. Called from the state pump each poll; `close_open_sets()` at startup; on app exit a running recording is stopped (files written) and the set ended.
- `RecordingService`: `start` → `force_start` the set if none, `record_channel`, `Command::SetRecordTap(Some)`, `Recorder::start`, `add_recording`; `stop` → `SetRecordTap(None)`, `Recorder::stop`, `finish_recording`, write `.txt` and enabled marker files next to the audio (origin = recording `start_clock` − set `clock_start`, title = set name); status event `recording://status` once a second.
- Commands: `history_sets`, `history_set(id)`, `history_rename`, `history_delete`, `history_remove_entry`, `history_tracklist(id) -> String`, `history_export(id, format, path)`, `history_to_playlist(id)`, `history_rewrite_recording_files(id)`, `recording_start`, `recording_stop`, `recording_status`, `recording_settings`, `recording_set_settings` (persisted as setting `recording`), `open_path(path)` reusing the `open` crate.

- [ ] **Step 1: Write the failing tests:** `observation_uses_the_fader_curve_and_crossfader_side`, `a_snapshot_sequence_produces_a_stored_set` (in-memory library, synthetic snapshots), contract checks for `SetSummary`, `SetDetail`, `SetEntry`, `SetRecording`, `RecorderStatus`, `RecordingSettings` against `types.ts`.
- [ ] **Step 2:** `cargo test -p deckforge-desktop` → FAIL.
- [ ] **Step 3:** Implement and register commands.
- [ ] **Step 4:** → PASS (contract test needs Task 7 types; add them in this task's commit).
- [ ] **Step 5:** Commit `feat(app): log sets and record the mix from the desktop app`.

### Task 7: History screen, record button and recording settings

**Files:**
- Create: `apps/desktop/src/lib/components/HistoryPanel.svelte`, `apps/desktop/src/lib/components/RecordingSettings.svelte`, `apps/desktop/src/lib/components/RecordButton.svelte`, `apps/desktop/src/lib/state/history.svelte.ts`, `apps/desktop/src/lib/util/duration.ts` + `.test.ts`
- Modify: `lib/api/{types.ts,index.ts,mock.ts}`, `lib/state/ui.svelte.ts` (`browserView` adds `'history'`), `lib/components/{Sidebar.svelte,LibraryPanel.svelte,TopBar.svelte,SettingsSheet.svelte}`, `package.json` test list

UI (frontend-design and ui-ux-pro-max skills apply; bar per the design-taste memory):
- Sidebar **History** item under Spotify → `browserView = 'history'`; LibraryPanel shows HistoryPanel: set list (name, date, length, tracks, ● REC badge, live set pinned with a pulse) and the opened set: rows of time, artist, title, key, BPM, blend window (`mixed in from 41:20`), teased rows dimmed; clicking a row with a library track selects it in the collection; header actions Copy tracklist, Export ▾ (Text/CSV/CUE/FCP XML), Make playlist, Open recording folder, Rename, Delete (ConfirmDialog).
- TopBar **REC** button: idle grey dot; recording = red pulsing dot, elapsed `H:MM:SS`, size; click toggles; error and warnings via Toasts.
- Settings: third tab **Recording**: folder (dialog picker), format, bit depth, sample rate, marker toggles.
- `formatDuration(secs)` in `duration.ts` mirrors the Rust `format_time`.

- [ ] **Step 1: Write the failing test:** `duration.test.ts` — `formatDuration(252) === '04:12'`, `formatDuration(3723) === '1:02:03'`, `formatDuration(0) === '00:00'`.
- [ ] **Step 2:** `pnpm test` → FAIL.
- [ ] **Step 3:** Implement components, state and mock data (two past sets, one live).
- [ ] **Step 4:** `pnpm check` 0 errors, `pnpm test` green; headless screenshot of History and REC on mock data reviewed.
- [ ] **Step 5:** Commit `feat(app): history screen, record button and recording settings`.

### Task 8: Docs and full verification

- [ ] README Library/Recording bullets; CHANGELOG Unreleased Added entries.
- [ ] `cargo test --workspace --release`, `cargo clippy --workspace --all-targets --release -- -D warnings`, `pnpm check && pnpm test`; an offline 10-minute record run through the engine with two tracks and a blend, checking file length, a take-over time within one block, and the `.txt`.
- [ ] Commit `docs: describe set history and recording`.
