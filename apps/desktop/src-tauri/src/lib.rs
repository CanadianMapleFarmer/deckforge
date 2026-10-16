mod analysis;
mod commands;
mod native_view;
pub mod spotify;
mod state;

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use deckforge_common::DECK_COUNT;
use deckforge_controller::{AppAction, ControllerService, Flx4};
use deckforge_engine::{
    create_engine, list_output_devices, AudioManager, Command, MixerParam, OutputRequest, SecondaryOutputRequest,
};
use deckforge_library::Library;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager, RunEvent};

use crate::analysis::AnalysisQueue;
use crate::state::AppState;

const STATE_INTERVAL: Duration = Duration::from_millis(16);
const ENGINE_RATE: u32 = 48_000;
const CONTROLLER_GRACE: Duration = Duration::from_secs(3);

fn data_dir() -> std::path::PathBuf {
    std::env::var_os("DECKFORGE_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("deckforge"))
}

/// A connected DDJ-FLX4 wins, then the saved device, then the system default.
/// Returns the request and whether it targets an FLX4.
fn initial_output(library: &Library) -> (OutputRequest, bool) {
    let devices = list_output_devices();
    if let Some(flx4) = devices.iter().find(|d| is_flx4(&d.name)) {
        return (flx4_request(library, &flx4.id), true);
    }
    (fallback_request(library, &devices), false)
}

fn is_flx4(name: &str) -> bool {
    name.to_ascii_uppercase().contains("FLX4")
}

fn flx4_request(library: &Library, device_id: &str) -> OutputRequest {
    library
        .setting::<OutputRequest>("audio.output")
        .filter(|saved| saved.device_id.as_deref() == Some(device_id))
        .unwrap_or(OutputRequest { device_id: Some(device_id.to_string()), ..Default::default() })
}

fn fallback_request(library: &Library, devices: &[deckforge_engine::OutputDeviceInfo]) -> OutputRequest {
    library
        .setting::<OutputRequest>("audio.output")
        .filter(|saved| {
            saved.device_id.as_deref().is_none_or(|id| devices.iter().any(|d| d.id == id && !is_flx4(&d.name)))
        })
        .unwrap_or_default()
}

/// Moves output to the FLX4 when it is plugged in and back to the previous device when it is removed.
fn follow_flx4(state: &AppState, was_present: &mut bool) {
    let devices = list_output_devices();
    let flx4 = devices.iter().find(|d| is_flx4(&d.name));
    let arrived = flx4.is_some() && !*was_present;
    let removed = flx4.is_none() && *was_present;
    *was_present = flx4.is_some();
    let on_flx4 = state.audio.status().active.as_ref().is_some_and(|a| is_flx4(&a.device_name));
    let request = match flx4 {
        Some(device) if arrived && !on_flx4 => {
            log::info!("DDJ-FLX4 audio detected; switching output to {}", device.name);
            if state.library.setting::<bool>("audio.hardwarePhones").is_none() {
                state.engine.send(Command::HardwarePhones(true));
            }
            flx4_request(&state.library, &device.id)
        }
        None if removed => {
            log::info!("DDJ-FLX4 audio removed; returning to the previous output");
            fallback_request(&state.library, &devices)
        }
        _ => return,
    };
    if let Err(e) = state.audio.open(request) {
        log::warn!("switching output failed: {e}");
    }
}

fn restore_secondary(library: &Library, audio: &AudioManager) {
    let Some(saved) = library.setting::<SecondaryOutputRequest>(commands::SECONDARY_SETTING) else { return };
    if !list_output_devices().iter().any(|d| d.id == saved.device_id) {
        log::info!("saved additional output is not connected; leaving it off");
        return;
    }
    if let Err(e) = audio.set_secondary(Some(saved)) {
        log::warn!("additional output unavailable at startup: {e}");
    }
}

fn spawn_state_pump(app: AppHandle) {
    thread::Builder::new()
        .name("deckforge-state-pump".into())
        .spawn(move || {
            let started = Instant::now();
            let mut last_status = Instant::now() - Duration::from_secs(5);
            let mut faders_initialised = false;
            let mut flx4_present = list_output_devices().iter().any(|d| is_flx4(&d.name));
            let mut native = native_view::PublishState::default();
            let mut next_ui = Instant::now();
            loop {
                let state = app.state::<AppState>();
                let snapshot = state.engine.snapshot();
                let poll = native_view::publish(&app, &snapshot, &mut native);
                if Instant::now() >= next_ui {
                    next_ui = (next_ui + STATE_INTERVAL).max(Instant::now());
                    let _ = app.emit("engine://state", snapshot);
                    state.engine.collect_garbage();
                    commands::persist_cues(&state);
                }
                if last_status.elapsed() >= Duration::from_secs(1) {
                    last_status = Instant::now();
                    let controller = state.controller.status();
                    if !faders_initialised && started.elapsed() >= CONTROLLER_GRACE {
                        faders_initialised = true;
                        if !controller.connected {
                            for channel in 0..DECK_COUNT {
                                state.engine.send(Command::Mixer { channel, param: MixerParam::Fader(1.0) });
                            }
                        }
                    }
                    follow_flx4(&state, &mut flx4_present);
                    let _ = app.emit("controller://status", controller);
                    let _ = app.emit("audio://status", state.audio.status());
                }
                thread::sleep(poll);
            }
        })
        .expect("spawn state pump");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = data_dir();
            log::info!("data directory: {}", dir.display());
            let library = Arc::new(Library::open(&dir.join("library.sqlite"))?);
            let (handle, rt) = create_engine(ENGINE_RATE);
            let engine = Arc::new(handle);
            if let Some(hw) = library.setting::<bool>("audio.hardwarePhones") {
                engine.send(Command::HardwarePhones(hw));
            }
            if let Some(curve) = library.setting("mixer.crossfaderCurve") {
                engine.send(Command::CrossfaderCurve(curve));
            }
            if let Some(curve) = library.setting("mixer.eqCurve") {
                engine.send(Command::EqCurve(curve));
            }
            let audio = AudioManager::spawn(rt);
            let (request, targets_flx4) = initial_output(&library);
            if targets_flx4 && library.setting::<bool>("audio.hardwarePhones").is_none() {
                engine.send(Command::HardwarePhones(true));
            }
            if let Err(e) = audio.open(request) {
                log::warn!("audio output unavailable at startup: {e}");
            }
            restore_secondary(&library, &audio);

            Box::leak(Box::new(deckforge_controller::watch_device_changes()));
            let handle_for_actions = app.handle().clone();
            let controller = ControllerService::spawn(
                Box::new(Flx4::new()),
                engine.clone(),
                Box::new(move |action: AppAction| {
                    let _ = handle_for_actions.emit("controller://app-action", action);
                }),
            );
            let loaded = Arc::new(Mutex::new([None, None]));
            let analysis = AnalysisQueue::new();
            analysis.spawn_workers(app.handle().clone(), library.clone(), engine.clone(), loaded.clone());
            for id in library.pending_analysis().unwrap_or_default() {
                analysis.enqueue(id);
            }
            app.manage(spotify::SpotifyState::new(&library)?);
            app.manage(AppState { engine, audio, library, controller, analysis, loaded });
            spawn_state_pump(app.handle().clone());
            native_view::setup(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::engine_command,
            commands::engine_snapshot,
            commands::load_track,
            commands::eject,
            commands::library_tracks,
            commands::library_track,
            commands::library_import,
            commands::library_import_playlists,
            commands::playlists_list,
            commands::playlist_create,
            commands::playlist_rename,
            commands::playlist_delete,
            commands::playlist_add_tracks,
            commands::playlist_remove_track,
            commands::track_analyze,
            commands::track_set_beatgrid,
            commands::track_waveform,
            commands::track_artwork,
            analysis::track_phrases,
            commands::audio_devices,
            commands::audio_open,
            commands::audio_status,
            commands::audio_set_routing,
            commands::audio_set_secondary,
            commands::audio_set_secondary_level,
            commands::controller_status,
            commands::midi_log,
            commands::midi_clear_log,
            commands::midi_inject,
            commands::ui_report,
            native_view::native_waveforms_available,
            native_view::native_set_rects,
            native_view::native_set_style,
            native_view::native_set_zoom,
            native_view::native_drag_preview,
            native_view::native_set_latency,
            native_view::native_reload_track,
            native_view::native_frame_stats,
            commands::selftest_requested,
            spotify::spotify_status,
            spotify::spotify_set_client_id,
            spotify::spotify_login,
            spotify::spotify_logout,
            spotify::spotify_playlists,
            spotify::spotify_playlist_tracks,
            spotify::spotify_import_playlist,
        ])
        .build(tauri::generate_context!())
        .expect("error while building deckforge")
        .run(|app, event| {
            if let RunEvent::ExitRequested { .. } = event
                && let Some(state) = app.try_state::<AppState>()
            {
                commands::persist_cues(&state);
                state.controller.shutdown();
                state.audio.close();
            }
        });
}
