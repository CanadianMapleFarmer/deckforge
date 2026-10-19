use chrono::{Local, TimeZone};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::{Library, LibraryError, Result};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SetSummary {
    pub id: i64,
    pub name: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_secs: f64,
    pub track_count: i64,
    pub has_recording: bool,
    pub live: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SetEntry {
    pub id: i64,
    pub track_id: Option<i64>,
    pub title: String,
    pub artist: String,
    pub path: String,
    pub key: Option<String>,
    pub bpm: Option<f64>,
    pub audible_secs: f64,
    pub took_over_secs: Option<f64>,
    pub teased: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SetRecording {
    pub id: i64,
    pub path: String,
    pub started_secs: f64,
    pub stopped_secs: Option<f64>,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SetDetail {
    pub summary: SetSummary,
    pub entries: Vec<SetEntry>,
    pub recordings: Vec<SetRecording>,
}

/// Local "YYYY-MM-DD HH:MM" for a Unix-millisecond timestamp.
pub fn local_name(started_at_ms: i64) -> String {
    Local.timestamp_millis_opt(started_at_ms).single().map_or_else(String::new, |t| t.format("%Y-%m-%d %H:%M").to_string())
}

const SUMMARY_SQL: &str = "SELECT s.id, s.name, s.started_at, s.ended_at,
    (SELECT COUNT(*) FROM set_entries e WHERE e.set_id = s.id AND e.removed = 0 AND e.took_over_secs IS NOT NULL),
    EXISTS(SELECT 1 FROM recordings r WHERE r.set_id = s.id)
    FROM sets s";

fn summary_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SetSummary> {
    let started_at: i64 = r.get(2)?;
    let ended_at: Option<i64> = r.get(3)?;
    let now = crate::unix_now() * 1000;
    Ok(SetSummary {
        id: r.get(0)?,
        name: r.get::<_, Option<String>>(1)?.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| local_name(started_at)),
        started_at,
        ended_at,
        duration_secs: (ended_at.unwrap_or(now) - started_at).max(0) as f64 / 1000.0,
        track_count: r.get(4)?,
        has_recording: r.get(5)?,
        live: ended_at.is_none(),
    })
}

impl Library {
    pub fn begin_set(&self, started_at_ms: i64, clock_start: f64) -> Result<i64> {
        let conn = self.conn.lock();
        conn.execute("INSERT INTO sets (started_at, clock_start) VALUES (?1, ?2)", params![started_at_ms, clock_start])?;
        Ok(conn.last_insert_rowid())
    }

    /// Adds an entry, snapshotting the track's metadata so the history survives later library changes.
    pub fn add_set_entry(&self, set_id: i64, track_id: i64, audible_secs: f64) -> Result<i64> {
        let t = self.track(track_id).ok();
        let (title, artist, path, key, bpm) = match &t {
            Some(t) => (t.title.clone(), t.artist.clone(), t.path.clone(), t.key.clone(), t.bpm),
            None => (String::new(), String::new(), String::new(), None, None),
        };
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO set_entries (set_id, track_id, title, artist, path, key, bpm, audible_secs) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![set_id, track_id, title, artist, path, key, bpm, audible_secs],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn mark_took_over(&self, entry_id: i64, secs: f64) -> Result<()> {
        self.conn.lock().execute("UPDATE set_entries SET took_over_secs = ?2 WHERE id = ?1", params![entry_id, secs])?;
        Ok(())
    }

    pub fn end_set(&self, set_id: i64, ended_at_ms: i64) -> Result<()> {
        self.conn.lock().execute("UPDATE sets SET ended_at = ?2 WHERE id = ?1 AND ended_at IS NULL", params![set_id, ended_at_ms])?;
        Ok(())
    }

    /// Ends sets left open by a crash, at their last logged moment.
    pub fn close_open_sets(&self) -> Result<usize> {
        let n = self.conn.lock().execute(
            "UPDATE sets SET ended_at = started_at + CAST(1000 * COALESCE(
                (SELECT MAX(MAX(e.audible_secs, COALESCE(e.took_over_secs, 0))) FROM set_entries e WHERE e.set_id = sets.id), 0) AS INTEGER)
             WHERE ended_at IS NULL",
            [],
        )?;
        Ok(n)
    }

    pub fn sets(&self) -> Result<Vec<SetSummary>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!("{SUMMARY_SQL} ORDER BY s.started_at DESC, s.id DESC"))?;
        let rows = stmt.query_map([], summary_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_detail(&self, id: i64) -> Result<SetDetail> {
        let conn = self.conn.lock();
        let summary =
            conn.query_row(&format!("{SUMMARY_SQL} WHERE s.id = ?1"), params![id], summary_row).optional()?.ok_or(LibraryError::NotFound(id))?;
        let mut stmt = conn.prepare(
            "SELECT id, track_id, title, artist, path, key, bpm, audible_secs, took_over_secs
             FROM set_entries WHERE set_id = ?1 AND removed = 0 ORDER BY audible_secs, id",
        )?;
        let mut entries = stmt
            .query_map(params![id], |r| {
                Ok(SetEntry {
                    id: r.get(0)?,
                    track_id: r.get(1)?,
                    title: r.get(2)?,
                    artist: r.get(3)?,
                    path: r.get(4)?,
                    key: r.get(5)?,
                    bpm: r.get(6)?,
                    audible_secs: r.get(7)?,
                    took_over_secs: r.get(8)?,
                    teased: false,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let last_take_over = entries.iter().filter_map(|e| e.took_over_secs).fold(f64::NEG_INFINITY, f64::max);
        for e in &mut entries {
            e.teased = e.took_over_secs.is_none() && (!summary.live || last_take_over > e.audible_secs);
        }
        let mut stmt = conn.prepare("SELECT id, path, started_secs, stopped_secs, format FROM recordings WHERE set_id = ?1 ORDER BY started_secs")?;
        let recordings = stmt
            .query_map(params![id], |r| {
                Ok(SetRecording { id: r.get(0)?, path: r.get(1)?, started_secs: r.get(2)?, stopped_secs: r.get(3)?, format: r.get(4)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(SetDetail { summary, entries, recordings })
    }

    pub fn rename_set(&self, id: i64, name: &str) -> Result<()> {
        self.conn.lock().execute("UPDATE sets SET name = ?2 WHERE id = ?1", params![id, name.trim()])?;
        Ok(())
    }

    /// Removes the set and its rows; recording files on disk are never touched.
    pub fn delete_set(&self, id: i64) -> Result<()> {
        self.conn.lock().execute("DELETE FROM sets WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn remove_set_entry(&self, entry_id: i64) -> Result<()> {
        self.conn.lock().execute("UPDATE set_entries SET removed = 1 WHERE id = ?1", params![entry_id])?;
        Ok(())
    }

    pub fn set_to_playlist(&self, id: i64) -> Result<i64> {
        let detail = self.set_detail(id)?;
        let playlist = self.create_playlist(&detail.summary.name, None, false)?;
        let tracks: Vec<i64> = {
            let conn = self.conn.lock();
            detail
                .entries
                .iter()
                .filter(|e| e.took_over_secs.is_some())
                .filter_map(|e| e.track_id)
                .filter(|t| conn.query_row("SELECT 1 FROM tracks WHERE id = ?1", params![t], |_| Ok(())).optional().ok().flatten().is_some())
                .collect()
        };
        self.add_to_playlist(playlist.id, &tracks)?;
        Ok(playlist.id)
    }

    pub fn add_recording(&self, set_id: i64, path: &str, started_secs: f64, format: &str) -> Result<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO recordings (set_id, path, started_secs, format) VALUES (?1, ?2, ?3, ?4)",
            params![set_id, path, started_secs, format],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finish_recording(&self, id: i64, stopped_secs: f64) -> Result<()> {
        self.conn.lock().execute("UPDATE recordings SET stopped_secs = ?2 WHERE id = ?1", params![id, stopped_secs])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::{params, Connection};

    use crate::Library;

    fn track(lib: &Library, path: &str, title: &str, artist: &str) -> i64 {
        let conn = lib.conn.lock();
        conn.execute(
            "INSERT INTO tracks (path, title, artist, file_type, added_at, analysis_state) VALUES (?1, ?2, ?3, 'wav', 0, 'done')",
            params![path, title, artist],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn sample(lib: &Library) -> (i64, [i64; 3]) {
        let a = track(lib, "/m/a.wav", "Night Shift", "Kora Vale");
        let b = track(lib, "/m/b.wav", "Copper Skies", "Nuvo & Sable");
        let c = track(lib, "/m/c.wav", "Static Bloom", "Ilya Marsh");
        let set = lib.begin_set(1_700_000_000_000, 100.0).unwrap();
        let ea = lib.add_set_entry(set, a, 0.0).unwrap();
        lib.mark_took_over(ea, 0.0).unwrap();
        let eb = lib.add_set_entry(set, b, 240.0).unwrap();
        lib.mark_took_over(eb, 252.0).unwrap();
        let ec = lib.add_set_entry(set, c, 400.0).unwrap();
        (set, [ea, eb, ec])
    }

    #[test]
    fn a_set_round_trips_with_its_entries_in_order() {
        let lib = Library::open_in_memory().unwrap();
        let (set, _) = sample(&lib);
        let d = lib.set_detail(set).unwrap();
        let titles: Vec<_> = d.entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Night Shift", "Copper Skies", "Static Bloom"]);
        assert_eq!(d.entries[1].artist, "Nuvo & Sable");
        assert_eq!(d.entries[1].took_over_secs, Some(252.0));
        assert!(d.summary.live);
        assert!(!d.entries[2].teased, "a blend still in progress in a live set is not a tease");
        assert_eq!(d.summary.track_count, 2);
        lib.end_set(set, 1_700_000_000_000 + 600_000).unwrap();
        let d = lib.set_detail(set).unwrap();
        assert!(!d.summary.live);
        assert!(d.entries[2].teased);
        assert_eq!(d.summary.duration_secs, 600.0);
        assert_eq!(lib.sets().unwrap()[0].id, set);
    }

    #[test]
    fn entries_keep_their_snapshot_after_the_track_is_deleted() {
        let lib = Library::open_in_memory().unwrap();
        let (set, [ea, ..]) = sample(&lib);
        let id = lib.set_detail(set).unwrap().entries[0].track_id.unwrap();
        lib.conn.lock().execute("DELETE FROM tracks WHERE id = ?1", params![id]).unwrap();
        let d = lib.set_detail(set).unwrap();
        assert_eq!(d.entries[0].id, ea);
        assert_eq!(d.entries[0].title, "Night Shift");
        assert_eq!(d.entries[0].path, "/m/a.wav");
    }

    #[test]
    fn close_open_sets_ends_a_crashed_set_at_its_last_entry() {
        let lib = Library::open_in_memory().unwrap();
        let (set, _) = sample(&lib);
        assert_eq!(lib.close_open_sets().unwrap(), 1);
        let s = lib.set_detail(set).unwrap().summary;
        assert_eq!(s.ended_at, Some(1_700_000_000_000 + 400_000));
        assert_eq!(lib.close_open_sets().unwrap(), 0);
    }

    #[test]
    fn rename_delete_and_remove_entry() {
        let lib = Library::open_in_memory().unwrap();
        let (set, [_, eb, _]) = sample(&lib);
        assert_eq!(lib.set_detail(set).unwrap().summary.name.len(), "2023-11-14 22:13".len());
        lib.rename_set(set, "Beer fest 2026").unwrap();
        lib.remove_set_entry(eb).unwrap();
        let d = lib.set_detail(set).unwrap();
        assert_eq!(d.summary.name, "Beer fest 2026");
        assert_eq!(d.entries.len(), 2);
        lib.delete_set(set).unwrap();
        assert!(lib.sets().unwrap().is_empty());
        assert!(lib.set_detail(set).is_err());
    }

    #[test]
    fn set_to_playlist_uses_took_over_tracks_in_order() {
        let lib = Library::open_in_memory().unwrap();
        let (set, _) = sample(&lib);
        lib.rename_set(set, "Warm-up").unwrap();
        let pl = lib.set_to_playlist(set).unwrap();
        let titles: Vec<_> = lib.tracks("", Some(pl)).unwrap().into_iter().map(|t| t.title).collect();
        assert_eq!(titles, ["Night Shift", "Copper Skies"]);
        assert!(lib.playlists().unwrap().iter().any(|p| p.id == pl && p.name == "Warm-up"));
    }

    #[test]
    fn recordings_belong_to_their_set_and_go_with_it() {
        let lib = Library::open_in_memory().unwrap();
        let (set, _) = sample(&lib);
        let rec = lib.add_recording(set, "/r/2026-10-19 21.00.wav", 30.0, "wav").unwrap();
        lib.finish_recording(rec, 500.0).unwrap();
        let d = lib.set_detail(set).unwrap();
        assert!(d.summary.has_recording);
        assert_eq!(d.recordings[0].stopped_secs, Some(500.0));
        lib.delete_set(set).unwrap();
        let left: i64 = lib.conn.lock().query_row("SELECT COUNT(*) FROM recordings", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn migration_6_applies_to_an_existing_v5_database() {
        let conn = Connection::open_in_memory().unwrap();
        for sql in &crate::schema::MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
        crate::schema::migrate(&conn).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 6);
        conn.execute("INSERT INTO sets (started_at, clock_start) VALUES (1, 0.0)", []).unwrap();
    }
}
