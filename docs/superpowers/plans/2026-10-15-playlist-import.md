# Playlist Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Import folders, M3U/M3U8 files and rekordbox XML exports as playlists nested in playlist folders, updating in place on re-import.

**Architecture:** A new `playlist_import` module in `crates/library` turns each source into a `Node` tree (scan / m3u / rekordbox_xml), resolves entries to track ids (resolve), and upserts the tree by `source` key in one transaction (apply). The desktop app gets one new Tauri command plus a tree sidebar and an import menu.

**Tech Stack:** Rust 2024, rusqlite 0.40, quick-xml 0.42, unicode-normalization 0.1, Tauri 2, Svelte 5, node:test.

**Spec:** `docs/superpowers/specs/2026-10-15-playlist-import-design.md`

## Global Constraints

- Music files and playlist files are only ever read, never written.
- Hand-made playlists (`source IS NULL`) are never modified by an import.
- Zero code comments by default; one line only for a constraint the code cannot express. Never run `cargo fmt`.
- Folder collapse state persists in `localStorage` (`df.collapsedFolders`), like other UI prefs (spec deviation: not the `settings` table).
- Verification: `cargo test -p deckforge-library`, `cargo clippy --workspace --all-targets --release -- -D warnings`, `cd apps/desktop && pnpm check && pnpm test`.

## Review Focus

- macOS stores file names NFD while M3U exports are often NFC: the same file must not become two collection rows (Task 5 `nfc_path_matches_existing_track`).
- Re-importing after the user deleted an imported playlist must recreate it, not fail on the unique `source` (Task 5 `reimport_recreates_a_deleted_playlist`).
- An M3U listing the same track twice must not fail the `(playlist_id, track_id)` primary key (Task 5 `duplicate_entries_keep_first_position`).
- Deleting a folder containing the selected playlist must not leave the track table pointing at a dead id (Task 7 `deletePlaylist` reset).
- A rekordbox XML with an unknown `Key` must import the rest of the playlist and report the entry missing (Task 3 `unknown_track_id_is_kept_as_missing_entry`).

---

## File structure

| File | Responsibility |
|---|---|
| `crates/library/src/schema.rs` | migration 5 |
| `crates/library/src/lib.rs` | `Playlist` fields, folder-aware create/rename/add, `import_playlists` entry point, error variant |
| `crates/library/src/playlist_import/mod.rs` | `Node`, `PlaylistImportReport`, `import_playlists` orchestration |
| `crates/library/src/playlist_import/m3u.rs` | M3U/M3U8 parsing and path-entry normalisation |
| `crates/library/src/playlist_import/rekordbox_xml.rs` | rekordbox XML → `Node` tree |
| `crates/library/src/playlist_import/scan.rs` | directory → `Node` tree + audio file list |
| `crates/library/src/playlist_import/resolve.rs` | entry → track id |
| `crates/library/src/playlist_import/apply.rs` | upsert tree, build report |
| `crates/library/src/playlist_import/tests.rs` | integration tests over temp dirs |
| `apps/desktop/src-tauri/src/commands.rs`, `lib.rs`, `spotify.rs` | new command, create signature |
| `apps/desktop/src/lib/api/{types,index,mock}.ts` | types and calls |
| `apps/desktop/src/lib/util/playlistTree.ts` (+ test) | visible-row flattening |
| `apps/desktop/src/lib/state/library.svelte.ts` | collapse state, import actions, report |
| `apps/desktop/src/lib/components/Sidebar.svelte` | tree rendering, folder menu |
| `apps/desktop/src/lib/components/LibraryPanel.svelte` | import menu |
| `apps/desktop/src/lib/components/ImportReport.svelte` | missing-track dialog |

---

### Task 1: Schema and folder-aware playlists

**Files:** Modify `crates/library/src/schema.rs`, `crates/library/src/lib.rs`, `crates/library/src/tests.rs`, `apps/desktop/src-tauri/src/spotify.rs`, `apps/desktop/src-tauri/src/commands.rs`

**Interfaces — Produces:**
- `Playlist { id: i64, name: String, track_count: i64, parent_id: Option<i64>, is_folder: bool }` (serde camelCase).
- `Library::create_playlist(&self, name: &str, parent_id: Option<i64>, is_folder: bool) -> Result<Playlist>`
- `rename_playlist` sets `renamed = 1`; `add_to_playlist` ignores folder ids.

- [ ] **Step 1: failing tests** in `tests.rs`:

```rust
#[test]
fn folders_nest_and_cascade_without_touching_tracks() {
    let lib = Library::open_in_memory().unwrap();
    let t = insert_track(&lib, "a.wav", None);
    let folder = lib.create_playlist("Crates", None, true).unwrap();
    let inner = lib.create_playlist("Deep", Some(folder.id), false).unwrap();
    lib.add_to_playlist(inner.id, &[t]).unwrap();
    lib.add_to_playlist(folder.id, &[t]).unwrap();
    let all = lib.playlists().unwrap();
    let f = all.iter().find(|p| p.id == folder.id).unwrap();
    assert!(f.is_folder && f.track_count == 0);
    assert_eq!(all.iter().find(|p| p.id == inner.id).unwrap().parent_id, Some(folder.id));
    lib.delete_playlist(folder.id).unwrap();
    assert!(lib.playlists().unwrap().is_empty());
    assert_eq!(lib.track(t).unwrap().id, t);
}

#[test]
fn renaming_marks_the_playlist_renamed() {
    let lib = Library::open_in_memory().unwrap();
    let p = lib.create_playlist("x", None, false).unwrap();
    lib.rename_playlist(p.id, "y").unwrap();
    let renamed: bool = lib.conn.lock().query_row("SELECT renamed FROM playlists WHERE id = ?1", params![p.id], |r| r.get(0)).unwrap();
    assert!(renamed);
}
```

- [ ] **Step 2:** `cargo test -p deckforge-library folders_nest` → compile FAIL (signature).
- [ ] **Step 3: implement.** Append migration to `MIGRATIONS`:

```rust
r#"
ALTER TABLE playlists ADD COLUMN parent_id INTEGER REFERENCES playlists(id) ON DELETE CASCADE;
ALTER TABLE playlists ADD COLUMN is_folder INTEGER NOT NULL DEFAULT 0;
ALTER TABLE playlists ADD COLUMN source TEXT;
ALTER TABLE playlists ADD COLUMN renamed INTEGER NOT NULL DEFAULT 0;
CREATE UNIQUE INDEX playlists_source ON playlists(source);
"#
```

`playlists()` selects `p.parent_id, p.is_folder`; `create_playlist` inserts `parent_id, is_folder`; `rename_playlist` runs `UPDATE playlists SET name = ?2, renamed = 1 WHERE id = ?1`; `add_to_playlist` returns `Ok(())` early when `SELECT is_folder FROM playlists WHERE id = ?1` is true. Update callers: `spotify.rs` `create_playlist(name, None, false)`; `commands.rs` `playlist_create(app, state, name: String, parent_id: Option<i64>, is_folder: Option<bool>)` → `create_playlist(&name, parent_id, is_folder.unwrap_or(false))`.
- [ ] **Step 4:** `cargo test -p deckforge-library` → PASS (including `migrates_a_version_one_database`).
- [ ] **Step 5:** commit `feat(library): playlist folders`.

### Task 2: M3U parser

**Files:** Create `crates/library/src/playlist_import/{mod.rs,m3u.rs}`; modify `crates/library/src/lib.rs` (`mod playlist_import;`).

**Interfaces — Produces:**
- `pub(crate) struct M3u { pub name: Option<String>, pub entries: Vec<String> }`
- `pub(crate) fn parse_m3u(bytes: &[u8]) -> M3u`
- `pub(crate) fn entry_path(raw: &str) -> String` — strips `file://` / `file://localhost`, percent-decodes, drops the leading `/` before a drive letter (`/C:/x` → `C:/x`).
- `pub(crate) fn is_playlist_file(path: &Path) -> bool` — `.m3u` / `.m3u8`, case-insensitive.

- [ ] **Step 1: failing tests** (in `m3u.rs` `#[cfg(test)] mod tests`):

```rust
#[test]
fn parses_extended_m3u_with_bom_and_name() {
    let m = parse_m3u("\u{feff}#EXTM3U\n#PLAYLIST:Deep Sunset\n#EXTINF:300,A - B\n01 A.mp3\r\n\n/abs/02 B.flac\n".as_bytes());
    assert_eq!(m.name.as_deref(), Some("Deep Sunset"));
    assert_eq!(m.entries, vec!["01 A.mp3", "/abs/02 B.flac"]);
}
#[test]
fn latin1_falls_back_when_not_utf8() {
    assert_eq!(parse_m3u(b"Caf\xe9.mp3\n").entries, vec!["Café.mp3"]);
}
#[test]
fn entry_paths_decode_file_urls_and_drive_letters() {
    assert_eq!(entry_path("file://localhost/Users/me/My%20Song.mp3"), "/Users/me/My Song.mp3");
    assert_eq!(entry_path("file:///C:/Music/a%23b.mp3"), "C:/Music/a#b.mp3");
    assert_eq!(entry_path("Music\\a.mp3"), "Music\\a.mp3");
}
```

- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3: implement:**

```rust
pub(crate) fn parse_m3u(bytes: &[u8]) -> M3u {
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    };
    let mut m = M3u { name: None, entries: Vec::new() };
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("#PLAYLIST:") {
            m.name = Some(name.trim().to_string()).filter(|n| !n.is_empty());
        } else if !line.is_empty() && !line.starts_with('#') {
            m.entries.push(line.to_string());
        }
    }
    m
}

pub(crate) fn entry_path(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix("file://") else { return raw.to_string() };
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest);
    let b = decoded.as_bytes();
    if b.len() > 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' { decoded[1..].to_string() } else { decoded }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%' && i + 2 < b.len())
            .then(|| std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()))
            .flatten();
        match hex {
            Some(v) => { out.push(v); i += 3; }
            None => { out.push(b[i]); i += 1; }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
```

`mod.rs` for now: `pub(crate) mod m3u;`.
- [ ] **Step 4:** `cargo test -p deckforge-library m3u` → PASS.
- [ ] **Step 5:** commit `feat(library): m3u playlist parser`.

### Task 3: rekordbox XML parser

**Files:** Create `crates/library/src/playlist_import/rekordbox_xml.rs`; add `quick-xml = "0.42"` to `[workspace.dependencies]` and `quick-xml.workspace = true` to `crates/library/Cargo.toml`; define `Node` in `mod.rs`.

**Interfaces — Produces** (in `mod.rs`):

```rust
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Folder { name: String, source: String, children: Vec<Node> },
    Playlist { name: String, source: String, base_dir: Option<PathBuf>, entries: Vec<String> },
}
```

- `pub(crate) fn parse_rekordbox_xml(text: &str, source_prefix: &str) -> Result<Vec<Node>, String>` — children of `ROOT`; sources are `"{prefix}#{a/b/c}"`; entries are decoded absolute paths (via `entry_path`), or `"#missing:{key}"` for unknown TrackIDs.

- [ ] **Step 1: failing tests:**

```rust
const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="2">
<TRACK TrackID="1" Name="A" Location="file://localhost/Music/A%20B.mp3"/>
<TRACK TrackID="2" Name="C" Location="file://localhost/Music/C.flac"/>
</COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="2">
<NODE Type="0" Name="House" Count="1"><NODE Name="Deep" Type="1" KeyType="0" Entries="3">
<TRACK Key="2"/><TRACK Key="1"/><TRACK Key="99"/></NODE></NODE>
<NODE Name="ByPath" Type="1" KeyType="1" Entries="1"><TRACK Key="file://localhost/Music/C.flac"/></NODE>
</NODE></PLAYLISTS></DJ_PLAYLISTS>"#;

#[test]
fn builds_the_folder_tree_in_order() {
    let nodes = parse_rekordbox_xml(XML, "/x.xml").unwrap();
    let Node::Folder { name, source, children } = &nodes[0] else { panic!() };
    assert_eq!((name.as_str(), source.as_str()), ("House", "/x.xml#House"));
    let Node::Playlist { name, source, entries, .. } = &children[0] else { panic!() };
    assert_eq!((name.as_str(), source.as_str()), ("Deep", "/x.xml#House/Deep"));
    assert_eq!(entries[..2], ["/Music/C.flac", "/Music/A B.mp3"]);
}
#[test]
fn unknown_track_id_is_kept_as_missing_entry() {
    let nodes = parse_rekordbox_xml(XML, "/x.xml").unwrap();
    let Node::Folder { children, .. } = &nodes[0] else { panic!() };
    let Node::Playlist { entries, .. } = &children[0] else { panic!() };
    assert_eq!(entries[2], "#missing:99");
}
#[test]
fn key_type_one_uses_locations() {
    let nodes = parse_rekordbox_xml(XML, "/x.xml").unwrap();
    let Node::Playlist { entries, .. } = &nodes[1] else { panic!() };
    assert_eq!(entries, &["/Music/C.flac"]);
}
#[test]
fn rejects_non_rekordbox_xml() {
    assert!(parse_rekordbox_xml("<plist/>", "/x.xml").is_err());
}
```

- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3: implement** with `quick_xml::Reader::from_str`, `config_mut().trim_text(true)`. Walk events: on `COLLECTION/TRACK` record `TrackID → entry_path(Location)`; under `PLAYLISTS` keep a stack of `(name, Type, KeyType, children/entries)` frames for `NODE` (Start pushes, End pops into parent; Empty NODE pushes+pops), `TRACK` with `Key` appends to the top frame. Because playlists may precede nothing but the collection always precedes playlists in rekordbox exports, resolve keys at TRACK time; if `DJ_PLAYLISTS` was never seen, return `Err("not a rekordbox collection export")`. Attribute values via `attr.unescape_value()`. Path segments join with `/`; the `ROOT` frame contributes no segment and is not emitted.
- [ ] **Step 4:** `cargo test -p deckforge-library rekordbox` → PASS.
- [ ] **Step 5:** commit `feat(library): rekordbox xml playlist parser`.

### Task 4: Folder scan

**Files:** Create `crates/library/src/playlist_import/scan.rs`.

**Interfaces — Produces:**
- `pub(crate) struct Scan { pub nodes: Vec<Node>, pub audio: Vec<PathBuf> }`
- `pub(crate) fn scan_dir(dir: &Path) -> Scan` — rules from the spec "Folder rules". Sources: folder = `dir` string; loose-track playlist = `"{dir}#tracks"`; m3u playlist = m3u path string. Playlist name from `#PLAYLIST:` else file stem; loose playlist named after the dir; children ordered folders first then by name (case-insensitive); loose files sorted by file name. Hidden entries (name starting with `.`) skipped. Unreadable m3u logs a warning and is ignored.

- [ ] **Step 1: failing tests** (temp dirs under `std::env::temp_dir().join(format!("df-scan-{}", n))`, files created with `std::fs::write(path, b"")`, cleaned with `std::fs::remove_dir_all`):

```rust
#[test]
fn single_track_folder_collapses_to_one_playlist() { /* dir Deep/{b.mp3,a.mp3} → [Playlist "Deep", source "<dir>#tracks", entries [a, b]] */ }
#[test]
fn m3u8_alongside_tracks_names_and_orders_the_playlist() { /* Deep/{a.mp3,b.mp3,Deep Sunset.m3u8 = "b.mp3\na.mp3"} → [Playlist "Deep Sunset", entries ["b.mp3","a.mp3"], base_dir Some(Deep)], audio has both */ }
#[test]
fn subdirectories_become_a_folder_with_loose_tracks_first_class() { /* Afro/{x.mp3, Deep/a.mp3, Peak/b.mp3, warmup.m3u8} → [Folder "Afro" children [Playlist Deep, Playlist Peak, Playlist warmup]] (warmup.m3u8 present, so x.mp3 gets no loose playlist but is in audio); and Mix/{y.mp3, Sub/z.mp3} → [Folder Mix children [Playlist Sub, Playlist Mix (#tracks)]] */ }
#[test]
fn empty_and_hidden_directories_produce_nothing() { /* .hidden/a.mp3 and empty/ → nodes empty */ }
```

Write each body fully: build the tree, call `scan_dir`, assert the exact `Node` values with `assert_eq!`.
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3: implement** a recursive `fn scan_node(dir, audio: &mut Vec<PathBuf>) -> Vec<Node>` returning the dir's contribution: read entries, split into subdirs / audio (`deckforge_analysis::decode::is_supported`, skip `._` and `.` names) / playlist files; recurse subdirs; build playlists (m3u ones, else loose one if any audio); `children = folders-sorted + playlists`; if no subdir nodes and exactly one playlist → return `vec![that]`; if `children.is_empty()` → `vec![]`; else `vec![Folder]`. `scan_dir` returns `Scan { nodes: scan_node(dir), audio }`.
- [ ] **Step 4:** `cargo test -p deckforge-library scan` → PASS.
- [ ] **Step 5:** commit `feat(library): scan folders into playlist trees`.

### Task 5: Resolve, apply and `Library::import_playlists`

**Files:** Create `resolve.rs`, `apply.rs`, `playlist_import/tests.rs`; modify `mod.rs`, `lib.rs`, `crates/library/Cargo.toml` (`unicode-normalization.workspace = true`).

**Interfaces — Produces:**

```rust
#[derive(Debug, Clone, Default, Serialize)] #[serde(rename_all = "camelCase")]
pub struct PlaylistImportReport { pub added: usize, pub skipped: usize, pub failed: usize, pub playlists: Vec<PlaylistImportSummary> }
#[derive(Debug, Clone, Serialize)] #[serde(rename_all = "camelCase")]
pub struct PlaylistImportSummary { pub id: i64, pub name: String, pub found: usize, pub total: usize, pub missing: Vec<String> }

impl Library { pub fn import_playlists(&self, paths: &[PathBuf]) -> Result<PlaylistImportReport> }
```

`LibraryError::Import(String)` added. `paths` may mix directories, `.m3u/.m3u8` and `.xml` files.

Resolution (`resolve.rs`): `struct Index` built from `SELECT id, path FROM tracks` with `by_path: HashMap<String /*NFC*/, i64>` and `by_name: HashMap<String /*lowercase NFC file name*/, Vec<(i64, Vec<String> /*lowercase parent components*/)>>`. `fn candidates(entry, base_dir) -> Vec<PathBuf>` = `[entry_path(entry)]` plus `base_dir.join(..)` when relative (`\` replaced by `/` for the join). `fn on_disk(entry, base) -> Option<PathBuf>` = first candidate that `is_file()`. `fn by_name(&self, entry) -> Option<i64>`: split on `/` and `\`, look up the last component, choose the unique candidate with the longest shared suffix of parent components, `None` on a tie.

Orchestration (`mod.rs::import_playlists`):
1. Build nodes: dir → `scan_dir`; m3u → one `Node::Playlist` (name `#PLAYLIST` or stem, source = path, base_dir = parent); xml → `parse_rekordbox_xml(read_to_string, path)` (`Err` → `LibraryError::Import`).
2. Collect every on-disk entry path plus `scan.audio`; run existing `import::import_paths` once (gives added/skipped/failed); rebuild `Index`.
3. Map each entry: on-disk → `by_path[nfc(path)]`; else `by_name`; `#missing:` → missing with the key as label. Missing label = last path component.
4. `apply::apply(conn, &nodes, &resolved) -> Vec<PlaylistImportSummary>` in one transaction.

Apply (`apply.rs`): recursive `upsert(tx, node, parent: Option<i64>)`: `SELECT id, renamed FROM playlists WHERE source = ?1`; existing → `UPDATE playlists SET parent_id = ?2, is_folder = ?3, name = CASE WHEN renamed THEN name ELSE ?4 END WHERE id = ?1`; else `INSERT (name, created_at, parent_id, is_folder, source)`. Playlists: `DELETE FROM playlist_tracks WHERE playlist_id = ?1`, then `INSERT OR IGNORE` ids with increasing position (counter only advances on insert). Summary `name` is the stored name.

- [ ] **Step 1: failing tests** in `playlist_import/tests.rs` (temp dirs, real silent WAVs via `hound` written like existing `tests.rs` helpers, or empty `.mp3` files — `import_paths` accepts unreadable tags):

```rust
#[test] fn folder_import_creates_tree_and_collection() { /* Afro/{Deep/a.mp3, Peak/b.mp3} → playlists: Folder Afro, Deep(1), Peak(1); tracks 2; report.added 2 */ }
#[test] fn reimport_updates_in_place() { /* import; append c.mp3 to Deep; reorder m3u; rename Peak→"Mine"; import again → same ids, Deep has 2 tracks in new order, Peak name "Mine", hand-made playlist untouched */ }
#[test] fn reimport_recreates_a_deleted_playlist() { /* import, delete Deep, import → Deep exists again */ }
#[test] fn duplicate_entries_keep_first_position() { /* m3u "a.mp3\nb.mp3\na.mp3" → order [a, b] */ }
#[test] fn missing_entries_fall_back_to_file_name_then_report() { /* collection already has /lib/x/a.mp3; m3u lists "C:\\Old\\x\\a.mp3" and "gone.mp3" → found 1 total 2 missing ["gone.mp3"] */ }
#[test] fn ambiguous_file_name_counts_as_missing() { /* collection /p/a.mp3 and /q/a.mp3, entry "a.mp3" with no dirs → missing */ }
#[test] fn nfc_path_matches_existing_track() { /* track stored with NFD "Cafe\u{301}.mp3"; on-disk entry NFC "Caf\u{e9}.mp3" (same file on APFS; on other OSes create the NFD file and assert by_path lookup via nfc) → no new track row */ }
#[test] fn xml_import_reports_unknown_ids() { /* XML from Task 3 with files under a temp dir → Deep found 2 total 3 */ }
```

Write each body fully with concrete `assert_eq!`s.
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** implement as specified above.
- [ ] **Step 4:** `cargo test -p deckforge-library` → PASS; `cargo clippy -p deckforge-library --all-targets -- -D warnings` clean.
- [ ] **Step 5:** commit `feat(library): import playlists from folders, m3u and rekordbox xml`.

### Task 6: Tauri command and TS API

**Files:** Modify `apps/desktop/src-tauri/src/{commands.rs,lib.rs}`, `apps/desktop/src/lib/api/{types.ts,index.ts,mock.ts}`.

**Interfaces — Produces:**
- Command `library_import_playlists(paths: Vec<String>) -> PlaylistImportReport` (blocking thread, enqueue pending analysis, emit `library://changed`).
- TS: `Playlist` gains `parentId: number | null; isFolder: boolean`; `PlaylistImportReport`, `PlaylistImportSummary` interfaces; `backend.createPlaylist(name, parentId: number | null = null, isFolder = false)`; `backend.importPlaylists(paths: string[])`.

- [ ] **Step 1:** add the command mirroring `library_import`, register it in `lib.rs`.
- [ ] **Step 2:** types and calls; mock: playlists gain `parentId: null, isFolder: false`, `createPlaylist` accepts the new args, `importPlaylists` returns `{ added: 0, skipped: 0, failed: 0, playlists: [] }`.
- [ ] **Step 3:** `cargo clippy -p deckforge-desktop --release -- -D warnings` (package name per `apps/desktop/src-tauri/Cargo.toml`) and `pnpm check` → clean.
- [ ] **Step 4:** commit `feat(app): playlist import command`.

### Task 7: Playlist tree state

**Files:** Create `apps/desktop/src/lib/util/playlistTree.ts`, `playlistTree.test.ts`; modify `package.json` test script, `library.svelte.ts`.

**Interfaces — Produces:**

```ts
export interface TreeRow { playlist: Playlist; depth: number; }
export function visibleRows(playlists: Playlist[], collapsed: ReadonlySet<number>): TreeRow[];
```

Order per level: folders first, then case-insensitive name. Children of a collapsed folder are omitted. Orphans (parent id not present) render at depth 0.

- [ ] **Step 1: failing test:**

```ts
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { visibleRows } from './playlistTree.ts';
const p = (id: number, name: string, parentId: number | null, isFolder = false) => ({ id, name, trackCount: 0, parentId, isFolder });
const all = [p(1, 'zeta', null), p(2, 'Crates', null, true), p(3, 'b', 2), p(4, 'A', 2), p(5, 'orphan', 99)];
test('folders first, children indented, sorted', () => {
  assert.deepEqual(visibleRows(all, new Set()).map((r) => [r.playlist.id, r.depth]), [[2, 0], [4, 1], [3, 1], [5, 0], [1, 0]]);
});
test('collapsed folders hide children', () => {
  assert.deepEqual(visibleRows(all, new Set([2])).map((r) => r.playlist.id), [2, 5, 1]);
});
```

- [ ] **Step 2:** add the file to the `test` script; `pnpm test` → FAIL.
- [ ] **Step 3:** implement; in `library.svelte.ts` add `collapsed = $state(new Set<number>(loadCollapsed()))`, `toggleFolder(id)` (persist to `localStorage['df.collapsedFolders']`), `rows = $derived(visibleRows(this.playlists, this.collapsed))`; `moveSidebar` iterates `this.rows.filter((r) => !r.playlist.isFolder)`; `createPlaylist(name, parentId = null, isFolder = false)`; `deletePlaylist` resets selection when `playlistId` is no longer in the refreshed list; `importFolders(asPlaylists: boolean)` and `importPlaylistFile()` (dialog filter `['m3u', 'm3u8', 'xml']`) calling `importPlaylists`; `importReport = $state<PlaylistImportReport | null>(null)` set when any summary has `missing.length`, else toast `Imported N playlists · X new tracks`.
- [ ] **Step 4:** `pnpm test && pnpm check` → PASS.
- [ ] **Step 5:** commit `feat(app): playlist tree state and import actions`.

### Task 8: Sidebar tree, import menu, report dialog

**Files:** Modify `Sidebar.svelte`, `LibraryPanel.svelte`; create `ImportReport.svelte`; mount it next to `ConfirmDialog` (find with `grep -rn "<ConfirmDialog" apps/desktop/src`).

- [ ] **Step 1: Sidebar** — iterate `library.rows`; indent `padding-left: calc(8px + depth * 14px)`; folder rows show `ChevronRight` (rotated 90° when open) + `Folder` icon, click toggles, no `data-drop-playlist`; folder context menu: New playlist here, New folder here, Rename, Delete folder (`danger`). The inline create input carries `{ parentId, isFolder }` and renders under its parent (expand the parent on start). "+" keeps creating a top-level playlist.
- [ ] **Step 2: LibraryPanel** — the Import button opens `ContextMenu` at the button's bottom-left with: Add folders to collection / Import folders as playlists… / Import playlist file…
- [ ] **Step 3: ImportReport.svelte** — `Sheet` titled "Import finished"; one section per summary with `missing.length > 0`: `{name} — {found} of {total} found`, then the missing names in a scrollable list; footer note "Re-import later to fill in tracks once they're available." Close clears `library.importReport`.
- [ ] **Step 4:** `pnpm check && pnpm test`; `pnpm tauri dev`, import a test folder tree with an `.m3u8` and a rekordbox XML, verify tree, collapse persistence, re-import update, report dialog.
- [ ] **Step 5:** commit `feat(app): playlist folders sidebar and import menu`.

### Task 9: Docs and PR

- [ ] README Library bullet: "SQLite collection with folder, M3U/M3U8 and rekordbox XML playlist import, playlist folders, search and background analysis"; CHANGELOG `Unreleased` entry.
- [ ] Full verification (Global Constraints commands), push branch with the personal account, open PR.
