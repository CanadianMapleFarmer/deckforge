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
    if let Some(e) = &result.error {
        log::warn!("the recording writer stopped early: {e}");
    }
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

/// Finishes recordings a crash left open, timed from what their file headers declare, and writes their tracklists.
pub fn recover(library: &Library) -> usize {
    let settings = settings(library);
    let mut n = 0;
    for (set_id, r) in library.open_recordings().unwrap_or_default() {
        let audio = Path::new(&r.path);
        let Some(duration) = deckforge_recorder::recorded_duration(audio) else { continue };
        let stopped = r.started_secs + duration;
        if library.finish_recording(r.id, stopped).is_err() {
            continue;
        }
        n += 1;
        match library.set_detail(set_id) {
            Ok(detail) => {
                if let Err(e) = write_side_files(&detail, audio, r.started_secs, stopped, &settings) {
                    log::warn!("writing the tracklist beside {} failed: {e}", audio.display());
                }
            }
            Err(e) => log::warn!("recovering {}: {e}", audio.display()),
        }
    }
    n
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

    #[test]
    fn a_recording_left_open_by_a_crash_gets_its_tracklist_on_recovery() {
        let dir = std::env::temp_dir().join(format!("deckforge-recover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lib = Library::open_in_memory().unwrap();
        let set = lib.begin_set(0, 0.0).unwrap();
        let entry = lib.add_set_entry(set, 99, 0.0).unwrap();
        lib.mark_took_over(entry, 0.0).unwrap();
        let audio = dir.join("2026-10-19 22.00.wav");
        let mut w = deckforge_recorder::WavWriter::create(&audio, 48_000, 24).unwrap();
        w.write(&vec![[0.0; 2]; 48_000 * 3]).unwrap();
        w.checkpoint().unwrap();
        drop(w);
        let id = lib.add_recording(set, &audio.display().to_string(), 10.0, "wav").unwrap();
        lib.close_open_sets().unwrap();
        assert_eq!(recover(&lib), 1);
        let r = lib.set_detail(set).unwrap().recordings.into_iter().find(|r| r.id == id).unwrap();
        assert_eq!(r.stopped_secs, Some(13.0));
        assert!(audio.with_extension("txt").exists());
        assert_eq!(recover(&lib), 0, "a recovered recording is not touched again");
    }

    #[test]
    #[ignore = "renders ten minutes of audio; run with --ignored"]
    fn a_ten_minute_offline_recording_matches_its_history() {
        use std::sync::Arc;
        use deckforge_common::{TrackAudio, HOT_CUE_SLOTS};
        use deckforge_engine::{create_engine, LoadRequest, MixerParam, Routing};
        use deckforge_recorder::{AudioFormat, WavWriter};
        use crate::history::HistoryService;

        const SR: u32 = 48_000;
        const SRC: u32 = 8_000;
        const BLOCK: usize = 512;
        let dir = std::env::temp_dir().join(format!("deckforge-offline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lib = Library::open_in_memory().unwrap();
        let mut files = Vec::new();
        for name in ["Ilya Marsh - Static Bloom.wav", "Peregrine - Golden Hour.wav"] {
            let path = dir.join(name);
            let mut w = WavWriter::create(&path, 44_100, 16).unwrap();
            w.write(&[[0.0; 2]; 4410]).unwrap();
            w.finish().unwrap();
            files.push(path);
        }
        assert_eq!(lib.import_paths(&files).added, 2);
        let ids: Vec<i64> = lib.tracks("", None).unwrap().iter().map(|t| t.id).collect();
        let (a, b) = {
            let find = |n: &str| lib.tracks(n, None).unwrap()[0].id;
            (find("Static"), find("Golden"))
        };
        assert_eq!(ids.len(), 2);

        let (h, mut rt) = create_engine(SR);
        rt.reconfigure(SR, Routing::for_channel_count(2));
        let tone = |freq: f64| {
            let n = SRC as usize * 7 * 60;
            Arc::new(TrackAudio::new(SRC, (0..n).flat_map(|i| { let x = (std::f64::consts::TAU * freq * i as f64 / SRC as f64).sin() as f32 * 0.3; [x, x] }).collect()))
        };
        for (deck, id, freq) in [(0, a, 220.0), (1, b, 330.0)] {
            h.send(Command::Load { deck, request: Box::new(LoadRequest { track: tone(freq), token: id as u64, grid: None, cue_secs: Some(0.0), hot_cues: [None; HOT_CUE_SLOTS] }) });
        }
        h.send(Command::Mixer { channel: 0, param: MixerParam::Fader(1.0) });
        h.send(Command::Mixer { channel: 1, param: MixerParam::Fader(0.0) });
        h.send(Command::Play { deck: 0 });

        let mut history = HistoryService::default();
        let mut out = vec![0.0f32; BLOCK * 2];
        let mut active: Option<(Recorder, Arc<deckforge_engine::RecordShared>)> = None;
        let mut set = None;
        let blocks = |secs: f64| (secs * SR as f64 / BLOCK as f64).round() as usize;
        let mut rendered_while_recording = 0usize;
        let settings = RecordingSettings { folder: dir.clone(), format: AudioFormat::Wav, ..RecordingSettings::default() };
        for k in 0..blocks(631.0) {
            if k == blocks(30.0) {
                set = history.force_start(h.snapshot().clock_secs, &lib);
                history.set_recording(true);
                let (tap, rx, shared) = record_channel(RECORD_RING_FRAMES);
                active = Some((Recorder::start(&settings, rx, shared.clone(), chrono::Local::now().naive_local()).unwrap(), shared));
                h.send(Command::SetRecordTap(RecordTapSlot::new(Some(tap))));
            }
            if k == blocks(240.0) {
                h.send(Command::Play { deck: 1 });
                h.send(Command::Mixer { channel: 1, param: MixerParam::Fader(1.0) });
            }
            if k == blocks(300.0) {
                h.send(Command::Mixer { channel: 0, param: MixerParam::Fader(0.0) });
            }
            if k == blocks(630.0) {
                h.send(Command::SetRecordTap(RecordTapSlot::new(None)));
            }
            rt.process(&mut out, 2);
            if active.is_some() && k >= blocks(30.0) && k < blocks(630.0) {
                rendered_while_recording += BLOCK;
            }
            history.observe(&h.snapshot(), &lib);
            h.collect_garbage();
            if k % 64 == 0 {
                std::thread::sleep(std::time::Duration::from_micros(300));
            }
        }
        let (recorder, shared) = active.unwrap();
        let set = set.unwrap();
        let result = recorder.stop().unwrap();
        assert_eq!(shared.dropped(), 0, "the writer kept up");
        assert_eq!((result.duration_secs * SR as f64).round() as usize, rendered_while_recording);
        let origin = result.start_clock - set.clock_start;
        let recording_id = lib.add_recording(set.id, &result.path.display().to_string(), origin, "wav").unwrap();
        lib.finish_recording(recording_id, origin + result.duration_secs).unwrap();
        let detail = lib.set_detail(set.id).unwrap();
        let took_b = detail.entries.iter().find(|e| e.track_id == Some(b)).and_then(|e| e.took_over_secs).unwrap() + set.clock_start;
        assert!((took_b - 300.0).abs() <= BLOCK as f64 / SR as f64 + 1e-9, "B took over at {took_b}");
        let written = write_side_files(&detail, &result.path, origin, origin + result.duration_secs, &settings).unwrap();
        let txt = std::fs::read_to_string(&written[0]).unwrap();
        assert_eq!(txt, "00:00 Ilya Marsh - Static Bloom\n04:30 Peregrine - Golden Hour\n", "{txt}");
        let mut wav = hound::WavReader::open(&result.path).unwrap();
        assert_eq!(wav.duration() as usize, rendered_while_recording);
        let peak = wav.samples::<i32>().skip(SR as usize * 120).take(SR as usize).map(|s| s.unwrap().abs()).max().unwrap();
        let centre = deckforge_dsp::crossfader_gains(0.5, deckforge_common::CrossfaderCurve::default()).0 as f64;
        assert!((peak as f64 / 8_388_607.0 - 0.3 * centre).abs() < 0.01, "deck A at full fader through a centred crossfader, peak {peak}");
        eprintln!("recorded {} s to {}", result.duration_secs, result.path.display());
    }
}
