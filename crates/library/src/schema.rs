use rusqlite::Connection;

pub(crate) const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE tracks (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    artist TEXT NOT NULL DEFAULT '',
    album TEXT NOT NULL DEFAULT '',
    genre TEXT NOT NULL DEFAULT '',
    duration_secs REAL NOT NULL DEFAULT 0,
    tag_bpm REAL,
    tag_key TEXT,
    file_type TEXT NOT NULL,
    file_size INTEGER,
    mtime INTEGER,
    added_at INTEGER NOT NULL,
    analysis_state TEXT NOT NULL DEFAULT 'pending',
    analysis_error TEXT
);
CREATE INDEX tracks_artist ON tracks(artist COLLATE NOCASE);
CREATE TABLE analysis (
    track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    bpm REAL,
    first_beat_secs REAL,
    confidence REAL NOT NULL DEFAULT 0,
    grid_locked INTEGER NOT NULL DEFAULT 0,
    waveform BLOB NOT NULL,
    analyzed_at INTEGER NOT NULL
);
-- slot -1 is the main cue, 0..7 hot cues; memory cues/saved loops will use further slots.
CREATE TABLE cues (
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    slot INTEGER NOT NULL,
    position_secs REAL NOT NULL,
    color INTEGER NOT NULL DEFAULT 0,
    label TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (track_id, slot)
);
CREATE TABLE artwork (
    track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    mime TEXT,
    data BLOB NOT NULL
);
CREATE TABLE playlists (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, track_id)
);
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#, r#"
ALTER TABLE analysis ADD COLUMN key_tonic INTEGER;
ALTER TABLE analysis ADD COLUMN key_minor INTEGER;
ALTER TABLE analysis ADD COLUMN key_confidence REAL;
CREATE TABLE phrases (
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    idx INTEGER NOT NULL,
    start_secs REAL NOT NULL,
    end_secs REAL NOT NULL,
    kind TEXT NOT NULL,
    bars INTEGER NOT NULL,
    energy REAL NOT NULL,
    PRIMARY KEY (track_id, idx)
);
"#, r#"
ALTER TABLE analysis ADD COLUMN downbeat_beat INTEGER;
ALTER TABLE phrases ADD COLUMN fill_start_secs REAL;
"#, r#"
ALTER TABLE analysis ADD COLUMN loudness_lufs REAL;
ALTER TABLE analysis ADD COLUMN sample_peak REAL;
"#, r#"
ALTER TABLE playlists ADD COLUMN parent_id INTEGER REFERENCES playlists(id) ON DELETE CASCADE;
ALTER TABLE playlists ADD COLUMN is_folder INTEGER NOT NULL DEFAULT 0;
ALTER TABLE playlists ADD COLUMN source TEXT;
ALTER TABLE playlists ADD COLUMN renamed INTEGER NOT NULL DEFAULT 0;
CREATE UNIQUE INDEX playlists_source ON playlists(source);
"#, r#"
CREATE TABLE sets (
    id INTEGER PRIMARY KEY,
    name TEXT,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    clock_start REAL NOT NULL
);
CREATE TABLE set_entries (
    id INTEGER PRIMARY KEY,
    set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
    track_id INTEGER,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    path TEXT NOT NULL,
    key TEXT,
    bpm REAL,
    audible_secs REAL NOT NULL,
    took_over_secs REAL,
    removed INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX set_entries_set ON set_entries(set_id, audible_secs);
CREATE TABLE recordings (
    id INTEGER PRIMARY KEY,
    set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    started_secs REAL NOT NULL,
    stopped_secs REAL,
    format TEXT NOT NULL
);
"#];

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        conn.execute_batch(&format!("BEGIN; {sql} PRAGMA user_version = {}; COMMIT;", i + 1))?;
    }
    Ok(())
}
