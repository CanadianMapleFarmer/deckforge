# Playlist import and playlist folders

## Goal

Importing music should bring its grouping along. A folder of tracks (optionally with a `.m3u8`), a standalone `.m3u`/`.m3u8`, or a rekordbox XML export becomes playlists — nested in playlist folders — so sets don't have to be rebuilt by hand. Music files stay read-only.

## Decisions

| Question | Decision |
|---|---|
| Nesting | rekordbox-style playlist folders in the sidebar; imports keep their tree |
| Formats | Folders on disk, M3U/M3U8, rekordbox collection XML |
| Re-import | Update in place, keyed by source; hand-made playlists never touched |
| Missing tracks | Path as written → relative to playlist file → unique file-name match in collection; otherwise skip and report |

## Data model

Migration 5 adds to `playlists`:

- `parent_id INTEGER REFERENCES playlists(id) ON DELETE CASCADE` — containing folder, `NULL` at top level.
- `is_folder INTEGER NOT NULL DEFAULT 0`
- `source TEXT UNIQUE` — origin key: a canonical folder or file path, or `<xml path>#<node path>` for rekordbox nodes. `NULL` for hand-made items.
- `renamed INTEGER NOT NULL DEFAULT 0` — set when the user renames an imported item; re-import then keeps the user's name.

`Playlist` gains `parentId`, `isFolder`. Children sort folders first, then by name (case-insensitive).

## Sidebar and import menu

- Playlists render as a tree under Collection/Spotify. Folders expand/collapse (state persisted in `settings`), and clicking a folder only toggles it.
- Folder context menu: New playlist here, New folder here, Rename, Delete folder (cascades to contained playlists; tracks stay in the collection). Playlist context menu unchanged.
- The Import button becomes a menu:
  - **Add folders to collection** — today's behaviour.
  - **Import folders as playlists…**
  - **Import playlist file…** — `.m3u`, `.m3u8`, `.xml`.

Moving playlists between folders by drag is out of scope.

## Folder rules (`scan`)

For each picked directory, recursively:

- A directory with playlist files: each `.m3u`/`.m3u8` becomes a playlist named after its `#PLAYLIST:` tag or file stem, in its order.
- A directory with audio files and no playlist file: one playlist named after the directory, ordered by file name.
- A directory with subdirectories becomes a folder; its own loose-track playlist (if any) and playlist-file playlists sit inside it.
- A directory with exactly one resulting playlist and no subdirectories collapses to that playlist (no wrapper folder). A single `.m3u8` in a track folder therefore yields one playlist named after the file.
- Every audio file under the root is added to the collection, listed in a playlist file or not.
- Empty directories produce nothing.

## Parsers

- **M3U/M3U8:** strip UTF-8 BOM; skip `#EXTM3U`/`#EXTINF`/other `#` lines; `#PLAYLIST:` sets the name; accept `file://` URLs (percent-decoded) and `\` separators; non-UTF-8 input decodes as Latin-1.
- **rekordbox XML** (`quick-xml`, no entity expansion): `COLLECTION/TRACK` maps `TrackID` → `Location` (`file://localhost/…`, percent-encoded); `PLAYLISTS/NODE` with `Type="0"` is a folder, `Type="1"` a playlist whose `TRACK` children reference `Key` by TrackID (`KeyType="0"`) or by location (`KeyType="1"`). The `ROOT` node is not itself imported. Cues, grids and ratings are ignored.

## Resolution

Each entry tries in order: the path as written; the path joined to the playlist file's directory; a collection track with the same file name (case-insensitive) — among several, the one sharing the most trailing parent-directory names wins, and a tie counts as missing. Windows paths are compared by their `\`-split components. A resolved file not yet in the collection is imported with the existing tag reader. Duplicate entries within one playlist keep their first position.

## Apply

One transaction per import. Nodes are upserted by `source`: existing rows get their track list replaced in source order and their name refreshed unless `renamed`; their `parent_id` follows the source tree. Items no longer present in the source are left alone.

## Result and UI

`PlaylistImportReport { added, skipped, failed, playlists: [{ id, name, found, total, missing: [string] }] }`. Command `library_import_playlists(paths)` runs on a blocking thread, enqueues new tracks for analysis and emits `library://changed`. A clean import shows a toast; any missing tracks open a dialog listing them per playlist.

## Testing

- Unit tests per parser with fixture strings: M3U (BOM, `#EXTINF`, relative, `file://`, Windows separators, Latin-1), rekordbox XML (nested folders, both `KeyType`s, percent-encoding, unknown track IDs).
- Scan tests on temp directories covering each folder rule, including collapse and the `.m3u8`-alongside-tracks case.
- Resolution tests: exact, relative, file-name fallback, ambiguous tie → missing.
- Library tests: migration on an existing database, import creates the tree, re-import updates in place (order change, new track, renamed name kept, hand-made playlist untouched), folder delete cascades without deleting tracks.
- Frontend: `pnpm check`, sidebar tree state tests where the existing test setup allows; manual check in the app.
