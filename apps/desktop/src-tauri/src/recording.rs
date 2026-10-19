use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use deckforge_engine::{record_channel, Command, RecordTapSlot, RECORD_RING_FRAMES};
use deckforge_library::history::{export_items, render, ExportFormat, SetDetail};
use deckforge_library::Library;
use deckforge_recorder::{Recorder, RecorderStatus, RecordingSettings};

use crate::state::AppState;

pub const SETTING: &str = "recording";
const MARKER_FRAME_RATE: u32 = 25;
const TAP_WAIT: Duration = Duration::from_millis(500);

pub struct ActiveRecording {
    recorder: Recorder,
    recording_id: i64,
    set_id: i64,
    started_secs: f64,
    settings: RecordingSettings,
}

impl ActiveRecording {
    pub fn set_id(&self) -> i64 {
        self.set_id
    }
}

pub fn settings(library: &Library) -> RecordingSettings {
    library.setting::<RecordingSettings>(SETTING).unwrap_or_default().sanitized()
}

fn wait_for(deadline: Duration, mut done: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + deadline;
    while Instant::now() < until {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

pub fn status(state: &AppState) -> Option<RecorderStatus> {
    state.recording.lock().as_ref().map(|r| r.recorder.status())
}

/// Non-blocking status for the state pump; `None` outer means a start or stop holds the lock.
pub fn try_status(state: &AppState) -> Option<Option<RecorderStatus>> {
    state.recording.try_lock().map(|r| r.as_ref().map(|r| r.recorder.status()))
}

pub fn start(state: &AppState) -> Result<RecorderStatus, String> {
    let mut slot = state.recording.lock();
    if let Some(active) = slot.as_ref() {
        return Ok(active.recorder.status());
    }
    let settings = settings(&state.library);
    let clock = state.engine.snapshot().clock_secs;
    let set = state.history.lock().force_start(clock, &state.library).ok_or("could not start a set")?;
    let (tap, rx, shared) = record_channel(RECORD_RING_FRAMES);
    let recorder = Recorder::start(&settings, rx, shared.clone(), chrono::Local::now().naive_local()).map_err(|e| e.to_string())?;
    if !state.engine.send(Command::SetRecordTap(RecordTapSlot::new(Some(tap)))) {
        let _ = recorder.stop();
        return Err("the audio engine is busy; try again".into());
    }
    state.history.lock().set_recording(true);
    wait_for(TAP_WAIT, || shared.start_clock().is_some());
    let started_secs = (shared.start_clock().unwrap_or(clock) - set.clock_start).max(0.0);
    let path = recorder.path().display().to_string();
    let recording_id = match state.library.add_recording(set.id, &path, started_secs, settings.format.extension()) {
        Ok(id) => id,
        Err(e) => {
            state.engine.send(Command::SetRecordTap(RecordTapSlot::new(None)));
            state.history.lock().set_recording(false);
            return Err(e.to_string());
        }
    };
    let status = recorder.status();
    *slot = Some(ActiveRecording { recorder, recording_id, set_id: set.id, started_secs, settings });
    Ok(status)
}

/// Stops the recording, finalizes the file and writes the tracklist and markers beside it.
pub fn stop(state: &AppState) -> Result<Option<PathBuf>, String> {
    let Some(active) = state.recording.lock().take() else { return Ok(None) };
    let before = state.engine.snapshot().frames_processed;
    state.engine.send(Command::SetRecordTap(RecordTapSlot::new(None)));
    wait_for(TAP_WAIT, || state.engine.snapshot().frames_processed > before);
    state.history.lock().set_recording(false);
    let result = active.recorder.stop().map_err(|e| e.to_string())?;
    let stopped = active.started_secs + result.duration_secs;
    state.library.finish_recording(active.recording_id, stopped).map_err(|e| e.to_string())?;
    let detail = state.library.set_detail(active.set_id).map_err(|e| e.to_string())?;
    if let Err(e) = write_side_files(&detail, &result.path, active.started_secs, stopped, &active.settings) {
        log::warn!("writing the tracklist beside {} failed: {e}", result.path.display());
    }
    Ok(Some(result.path))
}

fn side_path(audio: &Path, format: ExportFormat) -> PathBuf {
    audio.with_extension(format.extension())
}

pub fn write_side_files(detail: &SetDetail, audio: &Path, origin: f64, end: f64, settings: &RecordingSettings) -> std::io::Result<Vec<PathBuf>> {
    let items = export_items(&detail.entries, origin, end);
    let file_name = audio.file_name().map(|n| n.to_string_lossy().into_owned());
    let mut formats = vec![ExportFormat::Text];
    let m = settings.markers;
    formats.extend([(m.csv, ExportFormat::Csv), (m.cue, ExportFormat::Cue), (m.fcp_xml, ExportFormat::FcpXml)].into_iter().filter(|(on, _)| *on).map(|(_, f)| f));
    let mut written = Vec::new();
    for format in formats {
        let path = side_path(audio, format);
        std::fs::write(&path, render(format, &items, &detail.summary.name, file_name.as_deref(), MARKER_FRAME_RATE))?;
        written.push(path);
    }
    Ok(written)
}

/// Rewrites the files beside every finished recording of a set from its current entries.
pub fn rewrite_files(library: &Library, set_id: i64) -> Result<usize, String> {
    let detail = library.set_detail(set_id).map_err(|e| e.to_string())?;
    let settings = settings(library);
    let mut n = 0;
    for r in &detail.recordings {
        let (Some(stopped), audio) = (r.stopped_secs, Path::new(&r.path)) else { continue };
        if audio.exists() {
            write_side_files(&detail, audio, r.started_secs, stopped, &settings).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckforge_library::history::{SetEntry, SetSummary};
    use deckforge_recorder::MarkerFormats;

    fn entry(artist: &str, title: &str, audible: f64, took: f64) -> SetEntry {
        SetEntry { id: 0, track_id: None, title: title.into(), artist: artist.into(), path: String::new(), key: None, bpm: None, audible_secs: audible, took_over_secs: Some(took), teased: false }
    }

    #[test]
    fn side_files_are_timed_from_the_recording_and_follow_the_toggles() {
        let dir = std::env::temp_dir().join(format!("deckforge-side-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("2026-10-19 21.05.wav");
        let detail = SetDetail {
            summary: SetSummary { id: 1, name: "Friday".into(), started_at: 0, ended_at: None, duration_secs: 0.0, track_count: 2, has_recording: true, live: true },
            entries: vec![entry("A", "One", 0.0, 0.0), entry("B", "Two", 290.0, 312.0)],
            recordings: vec![],
        };
        let settings = RecordingSettings { markers: MarkerFormats { csv: false, cue: true, fcp_xml: false }, ..RecordingSettings::default() };
        let written = write_side_files(&detail, &audio, 60.0, 600.0, &settings).unwrap();
        assert_eq!(written, vec![dir.join("2026-10-19 21.05.txt"), dir.join("2026-10-19 21.05.cue")]);
        assert_eq!(std::fs::read_to_string(&written[0]).unwrap(), "00:00 A – One\n04:12 B – Two\n");
        assert!(std::fs::read_to_string(&written[1]).unwrap().contains("FILE \"2026-10-19 21.05.wav\" WAVE"));
    }
}
