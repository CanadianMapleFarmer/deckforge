mod import;
pub mod history;
mod playlist_import;
mod schema;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use deckforge_analysis::{Key, KeyEstimate, Loudness, Phrase, PhraseKind};
use deckforge_common::{Beatgrid, HotCue, HOT_CUE_SLOTS};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub use import::ImportResult;
pub use playlist_import::{PlaylistImportReport, PlaylistImportSummary};

pub const ANALYSIS_VERSION: i64 = 4;
const MAIN_CUE_SLOT: i64 = -1;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("track {0} not found")]
    NotFound(i64),
    #[error("{0}")]
    Import(String),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisState {
    Pending,
    Running,
    Done,
    Failed,
}

impl AnalysisState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "done" => Self::Done,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSummary {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub duration_secs: f64,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub key_name: Option<String>,
    pub analysis: AnalysisState,
    pub has_artwork: bool,
    pub file_type: String,
    pub added_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub track_count: i64,
    pub parent_id: Option<i64>,
    pub is_folder: bool,
}

#[derive(Debug, Clone)]
pub struct StoredAnalysis {
    pub version: i64,
    /// Anchored on the first downbeat when the bar phase is known, so grid beat 0 starts a bar.
    pub grid: Beatgrid,
    /// Beats from the stored first beat to the first downbeat; `None` until analysed (pre-v4 rows).
    pub downbeat_beat: Option<i64>,
    pub confidence: f32,
    pub waveform: Vec<u8>,
    pub key: Option<KeyEstimate>,
    pub loudness: Option<Loudness>,
}

impl StoredAnalysis {
    pub fn is_current(&self) -> bool {
        self.version >= ANALYSIS_VERSION && !self.waveform.is_empty()
    }
}

/// Key, bar phase and phrase results stored alongside the beatgrid analysis.
#[derive(Debug, Clone, Default)]
pub struct MusicalAnalysis {
    pub key: Option<KeyEstimate>,
    pub phrases: Vec<Phrase>,
    /// First downbeat as a beat index of the grid the phrases were analysed on.
    pub downbeat_beat: Option<i64>,
    pub loudness: Option<Loudness>,
}

#[derive(Debug, Clone, Default)]
pub struct StoredCues {
    pub main_cue: Option<f64>,
    pub hot_cues: [Option<HotCue>; HOT_CUE_SLOTS],
}

fn anchored_grid(bpm: f64, first_beat_secs: f64, downbeat_beat: Option<i64>) -> Beatgrid {
    let shift = match downbeat_beat {
        Some(d) if bpm > 0.0 => d as f64 * 60.0 / bpm,
        _ => 0.0,
    };
    Beatgrid { bpm, first_beat_secs: first_beat_secs + shift }
}

pub(crate) fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// SQLite-backed collection. Safe to share across threads; every call holds the connection briefly.
pub struct Library {
    conn: Mutex<Connection>,
}

const SUMMARY_COLUMNS: &str = "t.id, t.path, t.title, t.artist, t.album, t.genre, t.duration_secs,
    COALESCE(a.bpm, t.tag_bpm), t.tag_key, t.analysis_state, EXISTS(SELECT 1 FROM artwork w WHERE w.track_id = t.id),
    t.file_type, t.added_at, a.key_tonic, a.key_minor";

fn summary_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TrackSummary> {
    let analysed_key = match (r.get::<_, Option<i64>>(13)?, r.get::<_, Option<bool>>(14)?) {
        (Some(tonic), Some(minor)) => Some(Key::new(tonic.rem_euclid(12) as u8, minor)),
        _ => None,
    };
    let key = analysed_key.or_else(|| r.get::<_, Option<String>>(8).ok().flatten().and_then(|t| Key::parse(&t)));
    Ok(TrackSummary {
        id: r.get(0)?,
        path: r.get(1)?,
        title: r.get(2)?,
        artist: r.get(3)?,
        album: r.get(4)?,
        genre: r.get(5)?,
        duration_secs: r.get(6)?,
        bpm: r.get(7)?,
        key: key.map(Key::camelot),
        key_name: key.map(|k| k.name().to_string()),
        analysis: AnalysisState::parse(&r.get::<_, String>(9)?),
        has_artwork: r.get(10)?,
        file_type: r.get(11)?,
        added_at: r.get(12)?,
    })
}

fn replace_phrases(tx: &rusqlite::Transaction<'_>, id: i64, phrases: &[Phrase]) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM phrases WHERE track_id = ?1", params![id])?;
    for (idx, p) in phrases.iter().enumerate() {
        tx.execute(
            "INSERT INTO phrases (track_id, idx, start_secs, end_secs, kind, bars, energy, fill_start_secs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, idx as i64, p.start_secs, p.end_secs, p.kind.as_str(), p.bars, f64::from(p.energy), p.fill_start_secs],
        )?;
    }
    Ok(())
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::with_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::with_connection(Connection::open_in_memory()?)
    }

    fn with_connection(conn: Connection) -> Result<Self> {
        schema::migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn import_paths(&self, paths: &[PathBuf]) -> ImportResult {
        import::import_paths(&self.conn, paths)
    }

    pub fn import_playlists(&self, paths: &[PathBuf]) -> Result<PlaylistImportReport> {
        playlist_import::import_playlists(&self.conn, paths)
    }

    pub fn tracks(&self, query: &str, playlist: Option<i64>) -> Result<Vec<TrackSummary>> {
        let conn = self.conn.lock();
        let pattern = format!("%{}%", query.trim());
        let rows = if let Some(pl) = playlist {
            let sql = format!(
                "SELECT {SUMMARY_COLUMNS} FROM tracks t LEFT JOIN analysis a ON a.track_id = t.id
                 JOIN playlist_tracks p ON p.track_id = t.id
                 WHERE p.playlist_id = ?1 AND (t.title LIKE ?2 OR t.artist LIKE ?2 OR t.album LIKE ?2 OR t.genre LIKE ?2)
                 ORDER BY p.position"
            );
            let mut st = conn.prepare(&sql)?;
            st.query_map(params![pl, pattern], summary_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            let sql = format!(
                "SELECT {SUMMARY_COLUMNS} FROM tracks t LEFT JOIN analysis a ON a.track_id = t.id
                 WHERE t.title LIKE ?1 OR t.artist LIKE ?1 OR t.album LIKE ?1 OR t.genre LIKE ?1
                 ORDER BY t.artist COLLATE NOCASE, t.title COLLATE NOCASE"
            );
            let mut st = conn.prepare(&sql)?;
            st.query_map(params![pattern], summary_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    pub fn track(&self, id: i64) -> Result<TrackSummary> {
        let conn = self.conn.lock();
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM tracks t LEFT JOIN analysis a ON a.track_id = t.id WHERE t.id = ?1"
        );
        conn.query_row(&sql, params![id], summary_from_row).optional()?.ok_or(LibraryError::NotFound(id))
    }

    pub fn track_path(&self, id: i64) -> Result<PathBuf> {
        let conn = self.conn.lock();
        conn.query_row("SELECT path FROM tracks WHERE id = ?1", params![id], |r| r.get::<_, String>(0))
            .optional()?
            .map(PathBuf::from)
            .ok_or(LibraryError::NotFound(id))
    }

    /// Tracks needing analysis (never analysed, stale version, or interrupted mid-run).
    pub fn pending_analysis(&self) -> Result<Vec<i64>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT t.id FROM tracks t LEFT JOIN analysis a ON a.track_id = t.id
             WHERE t.analysis_state IN ('pending', 'running') OR (a.version IS NOT NULL AND a.version < ?1)
             ORDER BY t.added_at DESC, t.id",
        )?;
        let ids = st.query_map(params![ANALYSIS_VERSION], |r| r.get(0))?.collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(ids)
    }

    pub fn set_analysis_state(&self, id: i64, state: AnalysisState, error: Option<&str>) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE tracks SET analysis_state = ?2, analysis_error = ?3 WHERE id = ?1",
            params![id, state.as_str(), error],
        )?;
        Ok(())
    }

    /// Stores beatgrid analysis only, clearing any stored key and phrases.
    pub fn save_analysis(&self, id: i64, grid: Option<Beatgrid>, confidence: f32, waveform: &[u8], duration_secs: f64) -> Result<()> {
        self.save_full_analysis(id, grid, confidence, waveform, duration_secs, &MusicalAnalysis::default())
    }

    /// Stores analysis results unless the user has manually locked the beatgrid.
    pub fn save_full_analysis(
        &self,
        id: i64,
        grid: Option<Beatgrid>,
        confidence: f32,
        waveform: &[u8],
        duration_secs: f64,
        musical: &MusicalAnalysis,
    ) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let locked: Option<(f64, f64)> = tx
            .query_row(
                "SELECT bpm, first_beat_secs FROM analysis WHERE track_id = ?1 AND grid_locked = 1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (bpm, first, locked_flag) = match (locked, grid) {
            (Some((b, f)), _) => (Some(b), Some(f), 1),
            (None, Some(g)) => (Some(g.bpm), Some(g.first_beat_secs), 0),
            (None, None) => (None, None, 0),
        };
        tx.execute(
            "INSERT INTO analysis (track_id, version, bpm, first_beat_secs, confidence, grid_locked, waveform, analyzed_at,
               key_tonic, key_minor, key_confidence, downbeat_beat, loudness_lufs, sample_peak)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(track_id) DO UPDATE SET version = excluded.version, bpm = excluded.bpm,
               first_beat_secs = excluded.first_beat_secs, confidence = excluded.confidence,
               grid_locked = excluded.grid_locked, waveform = excluded.waveform, analyzed_at = excluded.analyzed_at,
               key_tonic = excluded.key_tonic, key_minor = excluded.key_minor, key_confidence = excluded.key_confidence,
               downbeat_beat = CASE WHEN analysis.grid_locked = 1 AND analysis.downbeat_beat IS NOT NULL
                 THEN analysis.downbeat_beat ELSE excluded.downbeat_beat END,
               loudness_lufs = excluded.loudness_lufs, sample_peak = excluded.sample_peak",
            params![
                id,
                ANALYSIS_VERSION,
                bpm,
                first,
                confidence,
                locked_flag,
                waveform,
                unix_now(),
                musical.key.map(|k| i64::from(k.key.tonic)),
                musical.key.map(|k| k.key.minor),
                musical.key.map(|k| f64::from(k.confidence)),
                musical.downbeat_beat,
                musical.loudness.map(|l| l.integrated_lufs).filter(|v| v.is_finite()),
                musical.loudness.map(|l| l.sample_peak),
            ],
        )?;
        replace_phrases(&tx, id, &musical.phrases)?;
        tx.execute(
            "UPDATE tracks SET analysis_state = 'done', analysis_error = NULL,
               duration_secs = CASE WHEN ?2 > 0 THEN ?2 ELSE duration_secs END WHERE id = ?1",
            params![id, duration_secs],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn analysis(&self, id: i64) -> Result<Option<StoredAnalysis>> {
        let conn = self.conn.lock();
        let row = conn
            .query_row(
                "SELECT bpm, first_beat_secs, confidence, waveform, version, key_tonic, key_minor, key_confidence, downbeat_beat,
                   loudness_lufs, sample_peak
                 FROM analysis WHERE track_id = ?1",
                params![id],
                |r| {
                    Ok((
                        (r.get::<_, Option<f64>>(0)?, r.get::<_, Option<f64>>(1)?, r.get::<_, Option<i64>>(8)?),
                        r.get::<_, f64>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                        r.get::<_, Option<bool>>(6)?,
                        r.get::<_, Option<f64>>(7)?,
                        (r.get::<_, Option<f64>>(9)?, r.get::<_, Option<f64>>(10)?),
                    ))
                },
            )
            .optional()?;
        Ok(row.map(|((bpm, first, downbeat), confidence, waveform, version, tonic, minor, key_confidence, (lufs, peak))| StoredAnalysis {
            version,
            grid: anchored_grid(bpm.unwrap_or(0.0), first.unwrap_or(0.0), downbeat),
            downbeat_beat: downbeat,
            confidence: confidence as f32,
            waveform,
            key: tonic.zip(minor).map(|(t, m)| KeyEstimate {
                key: Key::new(t.rem_euclid(12) as u8, m),
                confidence: key_confidence.unwrap_or(0.0) as f32,
            }),
            loudness: lufs.zip(peak).map(|(integrated_lufs, sample_peak)| Loudness { integrated_lufs, sample_peak }),
        }))
    }

    /// The stored first downbeat as a beat index of the stored (unanchored) grid.
    pub fn downbeat_beat(&self, id: i64) -> Result<Option<i64>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row("SELECT downbeat_beat FROM analysis WHERE track_id = ?1", params![id], |r| r.get::<_, Option<i64>>(0))
            .optional()?
            .flatten())
    }

    pub fn phrases(&self, id: i64) -> Result<Vec<Phrase>> {
        let conn = self.conn.lock();
        let mut st = conn
            .prepare("SELECT start_secs, end_secs, kind, bars, energy, fill_start_secs FROM phrases WHERE track_id = ?1 ORDER BY idx")?;
        let rows = st
            .query_map(params![id], |r| {
                Ok((
                    (r.get::<_, f64>(0)?, r.get::<_, f64>(1)?),
                    r.get::<_, String>(2)?,
                    r.get::<_, u32>(3)?,
                    r.get::<_, f64>(4)?,
                    r.get::<_, Option<f64>>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|((start_secs, end_secs), kind, bars, energy, fill_start_secs)| {
                Some(Phrase { start_secs, end_secs, kind: PhraseKind::parse(&kind)?, bars, energy: energy as f32, fill_start_secs })
            })
            .collect())
    }

    /// The beatgrid only when the user has manually locked it.
    pub fn locked_beatgrid(&self, id: i64) -> Result<Option<Beatgrid>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT bpm, first_beat_secs FROM analysis WHERE track_id = ?1 AND grid_locked = 1",
                params![id],
                |r| Ok((r.get::<_, Option<f64>>(0)?, r.get::<_, Option<f64>>(1)?)),
            )
            .optional()?
            .and_then(|(bpm, first)| Some(Beatgrid { bpm: bpm?, first_beat_secs: first? }))
            .filter(|g| g.bpm > 0.0))
    }

    pub fn beatgrid(&self, id: i64) -> Result<Option<Beatgrid>> {
        Ok(self.analysis(id)?.map(|a| a.grid).filter(|g| g.bpm > 0.0))
    }

    /// Manual grid correction, locked so re-analysis does not overwrite it. Its first beat is taken as a downbeat.
    pub fn set_beatgrid(&self, id: i64, grid: Beatgrid) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE analysis SET bpm = ?2, first_beat_secs = ?3, grid_locked = 1, downbeat_beat = 0 WHERE track_id = ?1",
            params![id, grid.bpm, grid.first_beat_secs],
        )?;
        self.conn.lock().execute("DELETE FROM phrases WHERE track_id = ?1", params![id])?;
        if n == 0 {
            self.conn.lock().execute(
                "INSERT INTO analysis (track_id, version, bpm, first_beat_secs, confidence, grid_locked, waveform, analyzed_at, downbeat_beat)
                 VALUES (?1, 0, ?2, ?3, 0, 1, x'', ?4, 0)",
                params![id, grid.bpm, grid.first_beat_secs, unix_now()],
            )?;
        }
        Ok(())
    }

    pub fn cues(&self, id: i64) -> Result<StoredCues> {
        let conn = self.conn.lock();
        let mut st = conn.prepare("SELECT slot, position_secs, color FROM cues WHERE track_id = ?1")?;
        let mut cues = StoredCues::default();
        let rows = st.query_map(params![id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?)))?;
        for row in rows {
            let (slot, pos, color) = row?;
            if slot == MAIN_CUE_SLOT {
                cues.main_cue = Some(pos);
            } else if (0..HOT_CUE_SLOTS as i64).contains(&slot) {
                cues.hot_cues[slot as usize] = Some(HotCue { position_secs: pos, color: color as u32 });
            }
        }
        Ok(cues)
    }

    pub fn save_cues(&self, id: i64, cues: &StoredCues) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM cues WHERE track_id = ?1", params![id])?;
        if let Some(pos) = cues.main_cue {
            tx.execute(
                "INSERT INTO cues (track_id, slot, position_secs, color) VALUES (?1, ?2, ?3, 0)",
                params![id, MAIN_CUE_SLOT, pos],
            )?;
        }
        for (slot, cue) in cues.hot_cues.iter().enumerate() {
            if let Some(c) = cue {
                tx.execute(
                    "INSERT INTO cues (track_id, slot, position_secs, color) VALUES (?1, ?2, ?3, ?4)",
                    params![id, slot as i64, c.position_secs, c.color as i64],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn artwork(&self, id: i64) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn
            .lock()
            .query_row("SELECT data FROM artwork WHERE track_id = ?1", params![id], |r| r.get(0))
            .optional()?)
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        let conn = self.conn.lock();
        let mut st = conn.prepare(
            "SELECT p.id, p.name, (SELECT COUNT(*) FROM playlist_tracks pt WHERE pt.playlist_id = p.id), p.parent_id, p.is_folder
             FROM playlists p ORDER BY p.name COLLATE NOCASE",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok(Playlist { id: r.get(0)?, name: r.get(1)?, track_count: r.get(2)?, parent_id: r.get(3)?, is_folder: r.get(4)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn create_playlist(&self, name: &str, parent_id: Option<i64>, is_folder: bool) -> Result<Playlist> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO playlists (name, created_at, parent_id, is_folder) VALUES (?1, ?2, ?3, ?4)",
            params![name, unix_now(), parent_id, is_folder],
        )?;
        Ok(Playlist { id: conn.last_insert_rowid(), name: name.to_string(), track_count: 0, parent_id, is_folder })
    }

    pub fn rename_playlist(&self, id: i64, name: &str) -> Result<()> {
        self.conn.lock().execute("UPDATE playlists SET name = ?2, renamed = 1 WHERE id = ?1", params![id, name])?;
        Ok(())
    }

    pub fn delete_playlist(&self, id: i64) -> Result<()> {
        self.conn.lock().execute("DELETE FROM playlists WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn add_to_playlist(&self, id: i64, track_ids: &[i64]) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let is_folder: bool =
            tx.query_row("SELECT is_folder FROM playlists WHERE id = ?1", params![id], |r| r.get(0)).optional()?.unwrap_or(false);
        if is_folder {
            return Ok(());
        }
        let mut next: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        for t in track_ids {
            let n = tx.execute(
                "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
                params![id, t, next],
            )?;
            next += n as i64;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_from_playlist(&self, id: i64, track_id: i64) -> Result<()> {
        self.conn.lock().execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
            params![id, track_id],
        )?;
        Ok(())
    }

    pub fn setting<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        let json: Option<String> = self
            .conn
            .lock()
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        json.and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let json = serde_json::to_string(value).unwrap_or_default();
        self.conn.lock().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, json],
        )?;
        Ok(())
    }
}
