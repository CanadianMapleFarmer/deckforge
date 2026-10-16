use std::path::PathBuf;

use deckforge_common::{
    Band, BeatFxKind, Beatgrid, ColorFxKind, CrossfaderCurve, EqCurve, FxBands, FxTarget, FxTrigger, PadFxAction, ReleaseFxKind, SmartCfxPreset, TempoRange,
    PAD_FX1_BANK, PAD_FX1_SHIFT_BANK,
};
use deckforge_controller::{ControllerStatus, MidiLogEntry};
use deckforge_engine::{
    list_output_devices, ActiveOutput, Command, EngineSnapshot, JogMode, LoadRequest, MixerParam, OutputDeviceInfo,
    OutputRequest, OutputStatus, Routing, SecondaryOutputRequest, JOG_TICKS_PER_REV,
};
use deckforge_library::{ImportResult, Playlist, PlaylistImportReport, StoredCues, TrackSummary};
use serde::{Deserialize, Serialize};
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, State};

use crate::state::{AppState, LoadedTrack};

pub type CmdResult<T> = Result<T, String>;

pub const SECONDARY_SETTING: &str = "audio.secondary";

pub fn err(e: impl ToString) -> String {
    e.to_string()
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UiCommand {
    Play { deck: usize },
    Pause { deck: usize },
    TogglePlay { deck: usize },
    CueDown { deck: usize },
    CueUp { deck: usize },
    Seek { deck: usize, secs: f64 },
    SetTempo { deck: usize, ratio: f64 },
    SetTempoRange { deck: usize, range: TempoRange },
    SetKeyLock { deck: usize, enabled: bool },
    SetVinylMode { deck: usize, enabled: bool },
    Nudge { deck: usize, amount: f64 },
    HotCue { deck: usize, slot: usize, pressed: bool },
    HotCueDelete { deck: usize, slot: usize },
    LoopIn { deck: usize },
    LoopOut { deck: usize },
    ReloopExit { deck: usize },
    BeatLoop { deck: usize, beats: f64 },
    LoopScale { deck: usize, factor: f64 },
    BeatJump { deck: usize, beats: f64 },
    Sync { deck: usize, enabled: bool },
    SetMasterDeck { deck: usize },
    Trim { channel: usize, value: f32 },
    Eq { channel: usize, band: Band, value: f32 },
    Filter { channel: usize, value: f32 },
    Fader { channel: usize, value: f32 },
    Pfl { channel: usize, enabled: bool },
    Crossfader { value: f32 },
    CrossfaderCurve { curve: CrossfaderCurve },
    EqCurve { curve: EqCurve },
    MasterLevel { value: f32 },
    HeadphoneMix { value: f32 },
    HeadphoneLevel { value: f32 },
    Quantize { enabled: bool },
    HardwarePhones { enabled: bool },
    BeatFxKind { kind: BeatFxKind },
    BeatFxTarget { target: FxTarget },
    BeatFxBeats { beats: f64 },
    BeatFxPercent { value: f64 },
    BeatFxLevel { value: f32 },
    BeatFxEnabled { enabled: bool },
    BeatFxBands { low: bool, mid: bool, high: bool },
    BeatFxParam { index: usize, value: f32 },
    BeatFxTrigger { action: FxTrigger },
    ColorFx { kind: ColorFxKind },
    ColorFxParam { index: usize, value: f32 },
    PadFx { deck: usize, slot: usize, shift: bool, pressed: bool },
    ReleaseFx,
    ReleaseFxConfig { kind: ReleaseFxKind, beats: f64 },
    SmartCfx { enabled: bool },
    SmartCfxPreset { preset: SmartCfxPreset },
}

/// A PAD FX 1 pad clicked on screen, resolved against the same bank as the hardware pads.
fn pad_fx_command(deck: usize, slot: usize, shift: bool, pressed: bool) -> Command {
    let bank = if shift { &PAD_FX1_SHIFT_BANK } else { &PAD_FX1_BANK };
    match (bank.get(slot).map(|s| s.action), pressed) {
        (Some(PadFxAction::Momentary { voice }), true) => Command::PadFx { deck, voice: Some(voice) },
        (Some(PadFxAction::OneShot { release, beats }), true) => Command::PadFxRelease { deck, kind: release, beats },
        _ => Command::PadFx { deck, voice: None },
    }
}

impl From<UiCommand> for Command {
    fn from(c: UiCommand) -> Self {
        use UiCommand as U;
        let mixer = |channel, param| Command::Mixer { channel, param };
        match c {
            U::Play { deck } => Command::Play { deck },
            U::Pause { deck } => Command::Pause { deck },
            U::TogglePlay { deck } => Command::TogglePlay { deck },
            U::CueDown { deck } => Command::CueDown { deck },
            U::CueUp { deck } => Command::CueUp { deck },
            U::Seek { deck, secs } => Command::SeekSecs { deck, secs },
            U::SetTempo { deck, ratio } => Command::SetTempo { deck, ratio },
            U::SetTempoRange { deck, range } => Command::SetTempoRange { deck, range },
            U::SetKeyLock { deck, enabled } => Command::SetKeyLock { deck, enabled },
            U::SetVinylMode { deck, enabled } => Command::SetVinylMode { deck, enabled },
            U::Nudge { deck, amount } => {
                Command::Jog { deck, ticks: (amount * JOG_TICKS_PER_REV).round() as i32, mode: JogMode::Bend }
            }
            U::HotCue { deck, slot, pressed } => Command::HotCue { deck, slot, pressed },
            U::HotCueDelete { deck, slot } => Command::HotCueDelete { deck, slot },
            U::LoopIn { deck } => Command::LoopIn { deck },
            U::LoopOut { deck } => Command::LoopOut { deck },
            U::ReloopExit { deck } => Command::ReloopExit { deck },
            U::BeatLoop { deck, beats } => Command::BeatLoop { deck, beats },
            U::LoopScale { deck, factor } => Command::LoopScale { deck, factor },
            U::BeatJump { deck, beats } => Command::BeatJump { deck, beats },
            U::Sync { deck, enabled } => Command::Sync { deck, enabled },
            U::SetMasterDeck { deck } => Command::SetMasterDeck { deck },
            U::Trim { channel, value } => mixer(channel, MixerParam::Trim(value)),
            U::Eq { channel, band, value } => mixer(channel, MixerParam::Eq(band, value)),
            U::Filter { channel, value } => mixer(channel, MixerParam::Filter(value)),
            U::Fader { channel, value } => mixer(channel, MixerParam::Fader(value)),
            U::Pfl { channel, enabled } => mixer(channel, MixerParam::Pfl(enabled)),
            U::Crossfader { value } => Command::Crossfader(value),
            U::CrossfaderCurve { curve } => Command::CrossfaderCurve(curve),
            U::EqCurve { curve } => Command::EqCurve(curve),
            U::MasterLevel { value } => Command::MasterLevel(value),
            U::HeadphoneMix { value } => Command::HeadphoneMix(value),
            U::HeadphoneLevel { value } => Command::HeadphoneLevel(value),
            U::Quantize { enabled } => Command::Quantize(enabled),
            U::HardwarePhones { enabled } => Command::HardwarePhones(enabled),
            U::BeatFxKind { kind } => Command::BeatFxKind(kind),
            U::BeatFxTarget { target } => Command::BeatFxTarget(target),
            U::BeatFxBeats { beats } => Command::BeatFxBeats(beats),
            U::BeatFxPercent { value } => Command::BeatFxPercent(value),
            U::BeatFxLevel { value } => Command::BeatFxLevel(value),
            U::BeatFxEnabled { enabled } => Command::BeatFxEnabled(enabled),
            U::BeatFxBands { low, mid, high } => Command::BeatFxBands(FxBands { low, mid, high }),
            U::BeatFxParam { index, value } => Command::BeatFxParam { index, value },
            U::BeatFxTrigger { action } => Command::BeatFxTrigger(action),
            U::ColorFx { kind } => Command::ColorFx(kind),
            U::ColorFxParam { index, value } => Command::ColorFxParam { index, value },
            U::PadFx { deck, slot, shift, pressed } => pad_fx_command(deck, slot, shift, pressed),
            U::ReleaseFx => Command::ReleaseFx,
            U::ReleaseFxConfig { kind, beats } => Command::ReleaseFxConfig { kind, beats },
            U::SmartCfx { enabled } => Command::SmartCfx(enabled),
            U::SmartCfxPreset { preset } => Command::SmartCfxPreset(preset),
        }
    }
}

#[cfg(test)]
mod ui_command_tests {
    use super::*;

    fn parse(json: &str) -> Command {
        serde_json::from_str::<UiCommand>(json).expect(json).into()
    }

    #[test]
    fn beat_and_color_fx_commands_deserialize_from_the_ts_contract() {
        assert!(matches!(parse(r#"{"type":"beatFxKind","kind":"pingPong"}"#), Command::BeatFxKind(BeatFxKind::PingPong)));
        assert!(matches!(parse(r#"{"type":"beatFxKind","kind":"filterSweep"}"#), Command::BeatFxKind(BeatFxKind::FilterSweep)));
        assert!(matches!(parse(r#"{"type":"beatFxTarget","target":"a"}"#), Command::BeatFxTarget(FxTarget::A)));
        assert!(matches!(parse(r#"{"type":"beatFxTarget","target":"b"}"#), Command::BeatFxTarget(FxTarget::B)));
        assert!(matches!(parse(r#"{"type":"beatFxTarget","target":"master"}"#), Command::BeatFxTarget(FxTarget::Master)));
        assert!(matches!(parse(r#"{"type":"beatFxBeats","beats":0.75}"#), Command::BeatFxBeats(b) if b == 0.75));
        assert!(matches!(parse(r#"{"type":"beatFxPercent","value":35}"#), Command::BeatFxPercent(p) if p == 35.0));
        assert!(matches!(parse(r#"{"type":"beatFxLevel","value":0.25}"#), Command::BeatFxLevel(v) if v == 0.25));
        assert!(matches!(parse(r#"{"type":"beatFxEnabled","enabled":true}"#), Command::BeatFxEnabled(true)));
        assert!(matches!(
            parse(r#"{"type":"beatFxBands","low":false,"mid":true,"high":true}"#),
            Command::BeatFxBands(FxBands { low: false, mid: true, high: true })
        ));
        assert!(matches!(parse(r#"{"type":"beatFxParam","index":3,"value":1}"#), Command::BeatFxParam { index: 3, value } if value == 1.0));
        for (name, want) in [
            ("throwOn", FxTrigger::ThrowOn),
            ("throwOff", FxTrigger::ThrowOff),
            ("freezeOn", FxTrigger::FreezeOn),
            ("freezeOff", FxTrigger::FreezeOff),
            ("release", FxTrigger::Release),
        ] {
            assert!(matches!(parse(&format!(r#"{{"type":"beatFxTrigger","action":"{name}"}}"#)), Command::BeatFxTrigger(got) if got == want));
        }
        assert!(matches!(parse(r#"{"type":"colorFx","kind":"dubEcho"}"#), Command::ColorFx(ColorFxKind::DubEcho)));
        assert!(matches!(parse(r#"{"type":"colorFxParam","index":0,"value":40}"#), Command::ColorFxParam { index: 0, value } if value == 40.0));
        for kind in ["filter", "space", "dubEcho", "crush", "noise"] {
            parse(&format!(r#"{{"type":"colorFx","kind":"{kind}"}}"#));
        }
        for kind in [
            "delay", "echo", "pingPong", "reverb", "flanger", "phaser", "roll", "trans", "filterSweep", "pulseVerb", "dubTape", "halo", "ascend", "backwash",
        ] {
            parse(&format!(r#"{{"type":"beatFxKind","kind":"{kind}"}}"#));
        }
    }

    #[test]
    fn pad_fx_release_and_smart_cfx_commands_resolve_against_the_banks() {
        assert!(matches!(
            parse(r#"{"type":"padFx","deck":1,"slot":0,"shift":false,"pressed":true}"#),
            Command::PadFx { deck: 1, voice: Some(v) } if v.kind == BeatFxKind::Echo
        ));
        assert!(matches!(parse(r#"{"type":"padFx","deck":0,"slot":0,"shift":false,"pressed":false}"#), Command::PadFx { deck: 0, voice: None }));
        assert!(matches!(
            parse(r#"{"type":"padFx","deck":0,"slot":6,"shift":false,"pressed":true}"#),
            Command::PadFxRelease { deck: 0, kind: ReleaseFxKind::VinylBrake, .. }
        ));
        assert!(matches!(
            parse(r#"{"type":"padFx","deck":0,"slot":2,"shift":true,"pressed":true}"#),
            Command::PadFx { voice: Some(v), .. } if v.kind == BeatFxKind::Trans
        ));
        assert!(matches!(parse(r#"{"type":"releaseFx"}"#), Command::ReleaseFx));
        assert!(matches!(
            parse(r#"{"type":"releaseFxConfig","kind":"backspin","beats":4}"#),
            Command::ReleaseFxConfig { kind: ReleaseFxKind::Backspin, beats } if beats == 4.0
        ));
        assert!(matches!(parse(r#"{"type":"smartCfx","enabled":true}"#), Command::SmartCfx(true)));
        assert!(matches!(parse(r#"{"type":"smartCfxPreset","preset":"riser"}"#), Command::SmartCfxPreset(SmartCfxPreset::Riser)));
    }

    #[test]
    fn unknown_fx_values_are_rejected() {
        assert!(serde_json::from_str::<UiCommand>(r#"{"type":"beatFxKind","kind":"gargle"}"#).is_err());
        assert!(serde_json::from_str::<UiCommand>(r#"{"type":"beatFxTarget","target":"c"}"#).is_err());
        assert!(serde_json::from_str::<UiCommand>(r#"{"type":"colorFx","kind":"dub-echo"}"#).is_err());
        assert!(serde_json::from_str::<UiCommand>(r#"{"type":"beatFxTrigger","action":"panic"}"#).is_err());
        assert!(serde_json::from_str::<UiCommand>(r#"{"type":"beatFxParam","index":-1,"value":1}"#).is_err());
        assert!(serde_json::from_str::<UiCommand>(r#"{"type":"colorFxParam","index":-1,"value":1}"#).is_err());
    }
}

#[tauri::command]
pub fn engine_command(state: State<'_, AppState>, cmd: UiCommand) -> CmdResult<()> {
    if let UiCommand::HardwarePhones { enabled } = &cmd {
        let _ = state.library.set_setting("audio.hardwarePhones", enabled);
    }
    if let UiCommand::CrossfaderCurve { curve } = &cmd {
        let _ = state.library.set_setting("mixer.crossfaderCurve", curve);
    }
    if let UiCommand::EqCurve { curve } = &cmd {
        let _ = state.library.set_setting("mixer.eqCurve", curve);
    }
    if state.engine.send(cmd.into()) { Ok(()) } else { Err("engine command queue full".into()) }
}

#[tauri::command]
pub fn engine_snapshot(state: State<'_, AppState>) -> EngineSnapshot {
    state.engine.snapshot()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadResult {
    deck: usize,
    track: TrackSummary,
}

#[tauri::command]
pub async fn load_track(state: State<'_, AppState>, deck: usize, track_id: i64, force: bool) -> CmdResult<LoadResult> {
    if deck > 1 {
        return Err("invalid deck".into());
    }
    if state.engine.snapshot().decks[deck].playing && !force {
        return Err("DECK_PLAYING".into());
    }
    let library = state.library.clone();
    let path = library.track_path(track_id).map_err(err)?;
    let audio = tauri::async_runtime::spawn_blocking(move || deckforge_analysis::decode_file(&path))
        .await
        .map_err(err)?
        .map_err(err)?;
    let grid = library.beatgrid(track_id).map_err(err)?;
    let cues = library.cues(track_id).map_err(err)?;
    if grid.is_none() {
        state.analysis.enqueue_urgent(track_id);
    }
    let request = LoadRequest {
        track: std::sync::Arc::new(audio),
        token: track_id as u64,
        grid,
        cue_secs: cues.main_cue,
        hot_cues: cues.hot_cues,
    };
    state.loaded.lock()[deck] = Some(LoadedTrack { track_id, persisted: cues });
    state.engine.send(Command::Load { deck, request: Box::new(request) });
    state.engine.send(phrase_starts_command(&library, deck, track_id));
    Ok(LoadResult { deck, track: library.track(track_id).map_err(err)? })
}

/// The deck's phrase anchors for a track, allocated here so the audio thread only swaps a box.
pub fn phrase_starts_command(library: &deckforge_library::Library, deck: usize, track_id: i64) -> Command {
    let starts: Vec<f64> = library.phrases(track_id).unwrap_or_default().iter().map(|p| p.start_secs).collect();
    Command::SetPhraseStarts { deck, starts: starts.into_boxed_slice() }
}

#[tauri::command]
pub fn eject(state: State<'_, AppState>, deck: usize) -> CmdResult<()> {
    if state.engine.snapshot().decks.get(deck).is_some_and(|d| d.playing) {
        return Err("DECK_PLAYING".into());
    }
    state.engine.send(Command::Eject { deck });
    if let Some(slot) = state.loaded.lock().get_mut(deck) {
        *slot = None;
    }
    Ok(())
}

#[tauri::command]
pub fn library_tracks(state: State<'_, AppState>, query: String, playlist_id: Option<i64>) -> CmdResult<Vec<TrackSummary>> {
    state.library.tracks(&query, playlist_id).map_err(err)
}

#[tauri::command]
pub fn library_track(state: State<'_, AppState>, id: i64) -> CmdResult<TrackSummary> {
    state.library.track(id).map_err(err)
}

#[tauri::command]
pub async fn library_import(app: AppHandle, state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<ImportResult> {
    let library = state.library.clone();
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let result = tauri::async_runtime::spawn_blocking(move || library.import_paths(&paths)).await.map_err(err)?;
    for id in state.library.pending_analysis().map_err(err)? {
        state.analysis.enqueue(id);
    }
    let _ = app.emit("library://changed", ());
    Ok(result)
}

#[tauri::command]
pub async fn library_import_playlists(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> CmdResult<PlaylistImportReport> {
    let library = state.library.clone();
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let report = tauri::async_runtime::spawn_blocking(move || library.import_playlists(&paths)).await.map_err(err)?.map_err(err)?;
    for id in state.library.pending_analysis().map_err(err)? {
        state.analysis.enqueue(id);
    }
    let _ = app.emit("library://changed", ());
    Ok(report)
}

#[tauri::command]
pub fn playlists_list(state: State<'_, AppState>) -> CmdResult<Vec<Playlist>> {
    state.library.playlists().map_err(err)
}

#[tauri::command]
pub fn playlist_create(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    parent_id: Option<i64>,
    is_folder: Option<bool>,
) -> CmdResult<Playlist> {
    let p = state.library.create_playlist(&name, parent_id, is_folder.unwrap_or(false)).map_err(err)?;
    let _ = app.emit("library://changed", ());
    Ok(p)
}

#[tauri::command]
pub fn playlist_rename(app: AppHandle, state: State<'_, AppState>, id: i64, name: String) -> CmdResult<()> {
    state.library.rename_playlist(id, &name).map_err(err)?;
    let _ = app.emit("library://changed", ());
    Ok(())
}

#[tauri::command]
pub fn playlist_delete(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.library.delete_playlist(id).map_err(err)?;
    let _ = app.emit("library://changed", ());
    Ok(())
}

#[tauri::command]
pub fn playlist_add_tracks(app: AppHandle, state: State<'_, AppState>, id: i64, track_ids: Vec<i64>) -> CmdResult<()> {
    state.library.add_to_playlist(id, &track_ids).map_err(err)?;
    let _ = app.emit("library://changed", ());
    Ok(())
}

#[tauri::command]
pub fn playlist_remove_track(app: AppHandle, state: State<'_, AppState>, id: i64, track_id: i64) -> CmdResult<()> {
    state.library.remove_from_playlist(id, track_id).map_err(err)?;
    let _ = app.emit("library://changed", ());
    Ok(())
}

#[tauri::command]
pub fn track_analyze(state: State<'_, AppState>, track_id: i64) -> CmdResult<()> {
    state.library.set_analysis_state(track_id, deckforge_library::AnalysisState::Pending, None).map_err(err)?;
    state.analysis.enqueue_urgent(track_id);
    Ok(())
}

#[tauri::command]
pub fn track_set_beatgrid(state: State<'_, AppState>, track_id: i64, bpm: f64, first_beat_secs: f64) -> CmdResult<()> {
    if !(20.0..=400.0).contains(&bpm) {
        return Err("BPM out of range".into());
    }
    let grid = Beatgrid { bpm, first_beat_secs };
    state.library.set_beatgrid(track_id, grid).map_err(err)?;
    for (deck, slot) in state.loaded.lock().iter().enumerate() {
        if slot.as_ref().is_some_and(|l| l.track_id == track_id) {
            state.engine.send(Command::SetBeatgrid { deck, grid: Some(grid) });
        }
    }
    Ok(())
}

#[tauri::command]
pub fn track_waveform(state: State<'_, AppState>, track_id: i64) -> CmdResult<Response> {
    let bytes = state.library.analysis(track_id).map_err(err)?.map(|a| a.waveform).unwrap_or_default();
    Ok(Response::new(bytes))
}

#[tauri::command]
pub fn track_artwork(state: State<'_, AppState>, track_id: i64) -> CmdResult<Response> {
    Ok(Response::new(state.library.artwork(track_id).map_err(err)?.unwrap_or_default()))
}

#[tauri::command]
pub fn audio_devices() -> Vec<OutputDeviceInfo> {
    list_output_devices()
}

#[tauri::command]
pub async fn audio_open(state: State<'_, AppState>, request: OutputRequest) -> CmdResult<ActiveOutput> {
    let active = state.audio.open(request.clone())?;
    let _ = state.library.set_setting("audio.output", &request);
    Ok(active)
}

#[tauri::command]
pub fn audio_status(state: State<'_, AppState>) -> OutputStatus {
    state.audio.status()
}

#[tauri::command]
pub async fn audio_set_secondary(
    state: State<'_, AppState>,
    request: Option<SecondaryOutputRequest>,
) -> CmdResult<OutputStatus> {
    state.audio.set_secondary(request.clone())?;
    let _ = state.library.set_setting(SECONDARY_SETTING, &request);
    Ok(state.audio.status())
}

#[tauri::command]
pub fn audio_set_secondary_level(state: State<'_, AppState>, level: f32) {
    state.audio.set_secondary_level(level);
    if let Some(mut saved) = state.library.setting::<SecondaryOutputRequest>(SECONDARY_SETTING) {
        saved.level = level.clamp(0.0, 1.0);
        let _ = state.library.set_setting(SECONDARY_SETTING, &Some(saved));
    }
}

#[tauri::command]
pub fn audio_set_routing(state: State<'_, AppState>, routing: Routing) {
    state.audio.set_routing(routing);
}

#[tauri::command]
pub fn controller_status(state: State<'_, AppState>) -> ControllerStatus {
    state.controller.status()
}

#[tauri::command]
pub fn midi_log(state: State<'_, AppState>) -> Vec<MidiLogEntry> {
    state.controller.log()
}

#[tauri::command]
pub fn midi_clear_log(state: State<'_, AppState>) {
    state.controller.clear_log();
}

#[tauri::command]
pub fn midi_inject(state: State<'_, AppState>, bytes: Vec<u8>) -> CmdResult<()> {
    if bytes.is_empty() || bytes.len() > 3 {
        return Err("expected a 1-3 byte MIDI message".into());
    }
    if state.controller.inject(bytes) { Ok(()) } else { Err("controller queue full".into()) }
}

#[tauri::command]
pub fn ui_report(message: String) {
    log::info!("UI: {message}");
}

#[tauri::command]
pub fn selftest_requested() -> bool {
    cfg!(debug_assertions) && std::env::var_os("DECKFORGE_SELFTEST").is_some()
}

pub fn persist_cues(state: &AppState) {
    let snap = state.engine.snapshot();
    let mut loaded = state.loaded.lock();
    for (deck, slot) in loaded.iter_mut().enumerate() {
        let Some(l) = slot else { continue };
        let d = &snap.decks[deck];
        if !d.loaded || d.track_token != l.track_id as u64 {
            continue;
        }
        let current = StoredCues { main_cue: Some(d.cue_secs), hot_cues: d.hot_cues };
        if current.main_cue != l.persisted.main_cue || current.hot_cues != l.persisted.hot_cues {
            if let Err(e) = state.library.save_cues(l.track_id, &current) {
                log::warn!("saving cues: {e}");
            } else {
                l.persisted = current;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPES_TS: &str = include_str!("../../src/lib/api/types.ts");

    fn sample(ts_type: &str) -> serde_json::Value {
        match ts_type {
            "DeckIndex" => 1.into(),
            "number" => 0.5.into(),
            "boolean" => true.into(),
            "BandName" => "Low".into(),
            "TempoRange" => "Ten".into(),
            "CrossfaderCurve" => "equal-power".into(),
            "EqCurve" => "isolator".into(),
            "BeatFxKind" => "pingPong".into(),
            "FxTarget" => "master".into(),
            "ColorFxKind" => "dubEcho".into(),
            "FxTriggerAction" => "freezeOn".into(),
            "ReleaseFxKind" => "vinylBrake".into(),
            "SmartCfxPreset" => "crushChop".into(),
            other => panic!("unmapped TS type {other}"),
        }
    }

    #[test]
    fn every_ui_command_variant_in_types_ts_deserializes() {
        let start = TYPES_TS.find("export type UiCommand =").unwrap();
        let block = &TYPES_TS[start..TYPES_TS[start..].find(";\n\n").unwrap() + start];
        let mut count = 0;
        for line in block.lines().filter(|l| l.trim_start().starts_with("| {")) {
            let inner = line.split('{').nth(1).unwrap().split('}').next().unwrap();
            let mut obj = serde_json::Map::new();
            for field in inner.split(';').map(str::trim).filter(|f| !f.is_empty()) {
                let (k, t) = field.split_once(':').unwrap();
                let t = t.trim();
                let v = match k.trim() {
                    "type" => t.trim_matches('\'').into(),
                    "slot" | "index" => 2.into(),
                    _ => sample(t),
                };
                obj.insert(k.trim().to_string(), v);
            }
            let json = serde_json::Value::Object(obj);
            let cmd: UiCommand = serde_json::from_value(json.clone()).unwrap_or_else(|e| panic!("{json}: {e}"));
            let _: Command = cmd.into();
            count += 1;
        }
        assert!(count >= 35, "parsed {count} variants");
    }
}
