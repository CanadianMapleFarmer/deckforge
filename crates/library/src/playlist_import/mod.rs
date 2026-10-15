mod apply;
pub(crate) mod m3u;
pub(crate) mod rekordbox_xml;
mod resolve;
pub(crate) mod scan;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use rusqlite::Connection;
use serde::Serialize;

use crate::{import, LibraryError, Result};
use m3u::is_playlist_file;
use rekordbox_xml::{parse_rekordbox_xml, MISSING_PREFIX};
use resolve::{file_label, on_disk, Index};
use scan::{m3u_node, scan_dir};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Folder { name: String, source: String, children: Vec<Node> },
    Playlist { name: String, source: String, base_dir: Option<PathBuf>, entries: Vec<String> },
}

pub(crate) enum Resolved {
    Folder { name: String, source: String, children: Vec<Resolved> },
    Playlist { name: String, source: String, tracks: Vec<i64>, total: usize, missing: Vec<String> },
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistImportReport {
    pub added: usize,
    pub skipped: usize,
    pub failed: usize,
    pub playlists: Vec<PlaylistImportSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistImportSummary {
    pub id: i64,
    pub name: String,
    pub found: usize,
    pub total: usize,
    pub missing: Vec<String>,
}

fn import_error(path: &Path, e: impl std::fmt::Display) -> LibraryError {
    LibraryError::Import(format!("{}: {e}", path.display()))
}

fn read_sources(paths: &[PathBuf]) -> Result<(Vec<Node>, Vec<PathBuf>)> {
    let (mut nodes, mut audio) = (Vec::new(), Vec::new());
    for path in paths {
        if path.is_dir() {
            let scan = scan_dir(path);
            nodes.extend(scan.nodes);
            audio.extend(scan.audio);
        } else if is_playlist_file(path) {
            nodes.push(m3u_node(path).map_err(|e| import_error(path, e))?);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml")) {
            let text = std::fs::read_to_string(path).map_err(|e| import_error(path, e))?;
            nodes.extend(parse_rekordbox_xml(&text, &path.to_string_lossy()).map_err(|e| import_error(path, e))?);
        } else {
            return Err(import_error(path, "not a folder, M3U or rekordbox XML file"));
        }
    }
    Ok((nodes, audio))
}

fn on_disk_entries(nodes: &[Node], out: &mut Vec<PathBuf>) {
    for node in nodes {
        match node {
            Node::Folder { children, .. } => on_disk_entries(children, out),
            Node::Playlist { base_dir, entries, .. } => {
                out.extend(entries.iter().filter_map(|e| on_disk(e, base_dir.as_deref())));
            }
        }
    }
}

fn load_index(conn: &Mutex<Connection>) -> Result<Index> {
    let conn = conn.lock();
    let mut st = conn.prepare("SELECT id, path FROM tracks")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<(i64, String)>>>()?;
    Ok(Index::new(rows))
}

fn resolve_nodes(nodes: &[Node], index: &Index) -> Vec<Resolved> {
    nodes
        .iter()
        .map(|node| match node {
            Node::Folder { name, source, children } => {
                Resolved::Folder { name: name.clone(), source: source.clone(), children: resolve_nodes(children, index) }
            }
            Node::Playlist { name, source, base_dir, entries } => {
                let (mut tracks, mut missing) = (Vec::new(), Vec::new());
                for entry in entries {
                    if let Some(key) = entry.strip_prefix(MISSING_PREFIX) {
                        missing.push(format!("Track ID {key}"));
                        continue;
                    }
                    let found = on_disk(entry, base_dir.as_deref())
                        .and_then(|p| index.by_path(&p))
                        .or_else(|| index.by_name(&m3u::entry_path(entry)));
                    match found {
                        Some(id) => tracks.push(id),
                        None => missing.push(file_label(&m3u::entry_path(entry))),
                    }
                }
                Resolved::Playlist { name: name.clone(), source: source.clone(), tracks, total: entries.len(), missing }
            }
        })
        .collect()
}

pub(crate) fn import_playlists(conn: &Mutex<Connection>, paths: &[PathBuf]) -> Result<PlaylistImportReport> {
    let (nodes, mut files) = read_sources(paths)?;
    on_disk_entries(&nodes, &mut files);
    files.sort();
    files.dedup();
    let index = load_index(conn)?;
    let total = files.len();
    files.retain(|f| index.by_path(f).is_none());
    let imported = import::import_paths(conn, &files);
    let index = load_index(conn)?;
    let playlists = apply::apply(conn, &resolve_nodes(&nodes, &index))?;
    Ok(PlaylistImportReport {
        added: imported.added,
        skipped: imported.skipped + (total - files.len()),
        failed: imported.failed,
        playlists,
    })
}
