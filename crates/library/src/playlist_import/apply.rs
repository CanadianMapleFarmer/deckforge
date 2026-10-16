use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use super::{PlaylistImportSummary, Resolved};
use crate::unix_now;

pub(crate) fn apply(conn: &Mutex<Connection>, nodes: &[Resolved]) -> rusqlite::Result<Vec<PlaylistImportSummary>> {
    let mut conn = conn.lock();
    let tx = conn.transaction()?;
    let mut out = Vec::new();
    for node in nodes {
        upsert(&tx, node, None, &mut out)?;
    }
    tx.commit()?;
    Ok(out)
}

fn upsert(tx: &Transaction<'_>, node: &Resolved, parent: Option<i64>, out: &mut Vec<PlaylistImportSummary>) -> rusqlite::Result<()> {
    let (name, source, folder) = match node {
        Resolved::Folder { name, source, .. } => (name, source, true),
        Resolved::Playlist { name, source, .. } => (name, source, false),
    };
    let existing: Option<i64> =
        tx.query_row("SELECT id FROM playlists WHERE source = ?1", params![source], |r| r.get(0)).optional()?;
    let id = match existing {
        Some(id) => {
            tx.execute(
                "UPDATE playlists SET parent_id = COALESCE(?2, parent_id), is_folder = ?3,
                 name = CASE WHEN renamed THEN name ELSE ?4 END WHERE id = ?1",
                params![id, parent, folder, name],
            )?;
            id
        }
        None => {
            tx.execute(
                "INSERT INTO playlists (name, created_at, parent_id, is_folder, source) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![name, unix_now(), parent, folder, source],
            )?;
            tx.last_insert_rowid()
        }
    };
    match node {
        Resolved::Folder { children, .. } => {
            for child in children {
                upsert(tx, child, Some(id), out)?;
            }
        }
        Resolved::Playlist { tracks, total, missing, .. } => {
            tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", params![id])?;
            let mut position = 0i64;
            for track in tracks {
                position += tx.execute(
                    "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
                    params![id, track, position],
                )? as i64;
            }
            let name: String = tx.query_row("SELECT name FROM playlists WHERE id = ?1", params![id], |r| r.get(0))?;
            out.push(PlaylistImportSummary { id, name, found: tracks.len(), total: *total, missing: missing.clone() });
        }
    }
    Ok(())
}
