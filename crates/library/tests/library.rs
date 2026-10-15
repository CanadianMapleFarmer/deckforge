use deckforge_common::{Beatgrid, HotCue};
use deckforge_library::{AnalysisState, Library, StoredCues};

fn write_wav(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let spec = hound::WavSpec { channels: 2, sample_rate: 44_100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for i in 0..44_100 {
        let s = ((i as f32 * 0.05).sin() * 8000.0) as i16;
        w.write_sample(s).unwrap();
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("deckforge-lib-test-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn import_query_analysis_cues_and_playlists() {
    let dir = temp_dir("all");
    write_wav(&dir, "Alpha Track.wav");
    write_wav(&dir, "Beta Track.wav");
    std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
    let lib = Library::open_in_memory().unwrap();

    let r = lib.import_paths(std::slice::from_ref(&dir));
    assert_eq!((r.added, r.skipped, r.failed), (2, 0, 0));
    let again = lib.import_paths(std::slice::from_ref(&dir));
    assert_eq!(again.skipped, 2, "re-import skips existing paths");

    let all = lib.tracks("", None).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().all(|t| t.analysis == AnalysisState::Pending && t.file_type == "wav"));
    assert!((all[0].duration_secs - 1.0).abs() < 0.01);
    let alpha = lib.tracks("alpha", None).unwrap();
    assert_eq!(alpha.len(), 1);
    let id = alpha[0].id;
    assert_eq!(lib.pending_analysis().unwrap().len(), 2);

    let grid = Beatgrid { bpm: 124.0, first_beat_secs: 0.1 };
    lib.save_analysis(id, Some(grid), 0.8, &[1, 2, 3], 1.0).unwrap();
    assert_eq!(lib.track(id).unwrap().bpm, Some(124.0));
    assert_eq!(lib.track(id).unwrap().analysis, AnalysisState::Done);
    assert_eq!(lib.analysis(id).unwrap().unwrap().waveform, vec![1, 2, 3]);
    assert_eq!(lib.pending_analysis().unwrap().len(), 1);

    lib.set_beatgrid(id, Beatgrid { bpm: 125.0, first_beat_secs: 0.2 }).unwrap();
    lib.save_analysis(id, Some(grid), 0.8, &[4], 1.0).unwrap();
    assert_eq!(lib.beatgrid(id).unwrap().unwrap().bpm, 125.0, "manual grid survives re-analysis");

    let mut cues = StoredCues { main_cue: Some(0.5), ..Default::default() };
    cues.hot_cues[3] = Some(HotCue { position_secs: 0.75, color: 0xFF0000 });
    lib.save_cues(id, &cues).unwrap();
    let back = lib.cues(id).unwrap();
    assert_eq!(back.main_cue, Some(0.5));
    assert_eq!(back.hot_cues[3].unwrap().color, 0xFF0000);
    assert!(back.hot_cues[0].is_none());

    let pl = lib.create_playlist("Warmup", None, false).unwrap();
    let ids: Vec<i64> = all.iter().map(|t| t.id).collect();
    lib.add_to_playlist(pl.id, &ids).unwrap();
    lib.add_to_playlist(pl.id, &ids).unwrap();
    assert_eq!(lib.playlists().unwrap()[0].track_count, 2);
    assert_eq!(lib.tracks("", Some(pl.id)).unwrap().len(), 2);
    lib.remove_from_playlist(pl.id, ids[0]).unwrap();
    assert_eq!(lib.tracks("", Some(pl.id)).unwrap().len(), 1);

    lib.set_setting("audio.device", &"BlackHole".to_string()).unwrap();
    assert_eq!(lib.setting::<String>("audio.device").as_deref(), Some("BlackHole"));
    let _ = std::fs::remove_dir_all(dir);
}
