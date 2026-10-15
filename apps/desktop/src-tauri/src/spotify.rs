use std::sync::Arc;

use deckforge_library::{Library, Playlist};
use deckforge_spotify::{Endpoints, KeyringStore, LibraryIndex, LocalTrack, Session, SpotifyPlaylist, SpotifyStatus, SpotifyTrack, system_clock};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::commands::{err, CmdResult};
use crate::state::AppState;

const CLIENT_ID_SETTING: &str = "spotify.clientId";
const KEYCHAIN_SERVICE: &str = "deckforge";
const KEYCHAIN_ACCOUNT: &str = "spotify-refresh-token";

pub struct SpotifyState {
    session: Arc<Session>,
}

impl SpotifyState {
    pub fn new(library: &Library) -> Result<Self, String> {
        let client_id = library.setting::<String>(CLIENT_ID_SETTING).filter(|id| !id.is_empty());
        let store = Arc::new(KeyringStore::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT));
        let session = Session::new(client_id, store, Endpoints::spotify(), system_clock()).map_err(err)?;
        Ok(Self { session: Arc::new(session) })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyImportResult {
    pub playlist: Playlist,
    pub matched: usize,
    pub unmatched: usize,
}

async fn match_against_library(library: Arc<Library>, mut tracks: Vec<SpotifyTrack>) -> CmdResult<Vec<SpotifyTrack>> {
    tauri::async_runtime::spawn_blocking(move || -> CmdResult<Vec<SpotifyTrack>> {
        let local: Vec<LocalTrack> = library
            .tracks("", None)
            .map_err(err)?
            .into_iter()
            .map(|t| LocalTrack { id: t.id, title: t.title, artist: t.artist, duration_secs: t.duration_secs })
            .collect();
        LibraryIndex::new(&local).annotate(&mut tracks);
        Ok(tracks)
    })
    .await
    .map_err(err)?
}

fn find_or_create_playlist(library: &Library, name: &str) -> CmdResult<Playlist> {
    let existing = library.playlists().map_err(err)?.into_iter().find(|p| p.name.eq_ignore_ascii_case(name));
    match existing {
        Some(p) => Ok(p),
        None => library.create_playlist(name, None, false).map_err(err),
    }
}

#[tauri::command]
pub async fn spotify_status(spotify: State<'_, SpotifyState>) -> CmdResult<SpotifyStatus> {
    Ok(spotify.session.status().await)
}

#[tauri::command]
pub async fn spotify_set_client_id(
    state: State<'_, AppState>,
    spotify: State<'_, SpotifyState>,
    client_id: String,
) -> CmdResult<SpotifyStatus> {
    let client_id = client_id.trim().to_string();
    state.library.set_setting(CLIENT_ID_SETTING, &client_id).map_err(err)?;
    spotify.session.set_client_id(Some(client_id)).await.map_err(err)?;
    Ok(spotify.session.status().await)
}

#[tauri::command]
pub async fn spotify_login(spotify: State<'_, SpotifyState>) -> CmdResult<SpotifyStatus> {
    spotify
        .session
        .login(|url| open::that_detached(url).map_err(|e| e.to_string()))
        .await
        .map_err(err)?;
    Ok(spotify.session.status().await)
}

#[tauri::command]
pub async fn spotify_logout(spotify: State<'_, SpotifyState>) -> CmdResult<SpotifyStatus> {
    spotify.session.logout().await.map_err(err)?;
    Ok(spotify.session.status().await)
}

#[tauri::command]
pub async fn spotify_playlists(spotify: State<'_, SpotifyState>) -> CmdResult<Vec<SpotifyPlaylist>> {
    spotify.session.playlists().await.map_err(err)
}

#[tauri::command]
pub async fn spotify_playlist_tracks(
    state: State<'_, AppState>,
    spotify: State<'_, SpotifyState>,
    playlist_id: String,
) -> CmdResult<Vec<SpotifyTrack>> {
    let tracks = spotify.session.playlist_tracks(&playlist_id).await.map_err(err)?;
    match_against_library(state.library.clone(), tracks).await
}

#[tauri::command]
pub async fn spotify_import_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    spotify: State<'_, SpotifyState>,
    playlist_id: String,
) -> CmdResult<SpotifyImportResult> {
    let name = spotify.session.playlist_name(&playlist_id).await.map_err(err)?;
    let tracks = spotify.session.playlist_tracks(&playlist_id).await.map_err(err)?;
    let tracks = match_against_library(state.library.clone(), tracks).await?;
    let local_ids: Vec<i64> = tracks.iter().filter_map(|t| t.local_track_id).collect();

    let target = find_or_create_playlist(&state.library, &name)?;
    state.library.add_to_playlist(target.id, &local_ids).map_err(err)?;
    let playlist = state
        .library
        .playlists()
        .map_err(err)?
        .into_iter()
        .find(|p| p.id == target.id)
        .unwrap_or(target);
    let _ = app.emit("library://changed", ());
    Ok(SpotifyImportResult { playlist, matched: local_ids.len(), unmatched: tracks.len() - local_ids.len() })
}
