use deckforge_analysis::{Key, KeyEstimate, Phrase, PhraseKind};
use deckforge_common::Beatgrid;
use rusqlite::{params, Connection};

use crate::{AnalysisState, Library, MusicalAnalysis, ANALYSIS_VERSION};

fn insert_track(lib: &Library, path: &str, tag_key: Option<&str>) -> i64 {
    let conn = lib.conn.lock();
    conn.execute(
        "INSERT INTO tracks (path, title, file_type, added_at, tag_key, analysis_state) VALUES (?1, ?1, 'wav', 0, ?2, 'done')",
        params![path, tag_key],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn sample_phrases() -> Vec<Phrase> {
    vec![
        Phrase { start_secs: 0.0, end_secs: 30.0, kind: PhraseKind::Intro, bars: 16, energy: 0.25, fill_start_secs: None },
        Phrase { start_secs: 30.0, end_secs: 60.0, kind: PhraseKind::Drop, bars: 16, energy: 1.0, fill_start_secs: Some(58.125) },
        Phrase { start_secs: 60.0, end_secs: 75.0, kind: PhraseKind::Outro, bars: 8, energy: 0.5, fill_start_secs: None },
    ]
}

#[test]
fn migrates_a_version_one_database() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(crate::schema::MIGRATIONS[0]).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
    conn.execute(
        "INSERT INTO tracks (id, path, title, file_type, added_at, tag_key, analysis_state) VALUES (1, 'a.wav', 'a', 'wav', 0, 'Am', 'done')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO analysis (track_id, version, bpm, first_beat_secs, confidence, grid_locked, waveform, analyzed_at)
         VALUES (1, 1, 128.0, 0.2, 0.9, 0, x'0102', 0)",
        [],
    )
    .unwrap();

    let lib = Library::with_connection(conn).unwrap();

    let version: i64 = lib.conn.lock().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version as usize, crate::schema::MIGRATIONS.len());
    let stored = lib.analysis(1).unwrap().unwrap();
    assert_eq!(stored.version, 1);
    assert_eq!(stored.waveform, vec![1, 2]);
    assert_eq!(stored.grid.bpm, 128.0);
    assert!(stored.key.is_none() && !stored.is_current());
    assert!(lib.phrases(1).unwrap().is_empty());
    assert_eq!(lib.pending_analysis().unwrap(), vec![1], "pre-key analyses are requeued");
    let summary = lib.track(1).unwrap();
    assert_eq!((summary.key.as_deref(), summary.key_name.as_deref()), (Some("8A"), Some("Am")));
}

#[test]
fn analysed_key_is_preferred_over_the_tag_key() {
    let lib = Library::open_in_memory().unwrap();
    let id = insert_track(&lib, "k.wav", Some("Am"));
    assert_eq!(lib.track(id).unwrap().key.as_deref(), Some("8A"));

    let musical = MusicalAnalysis { key: Some(KeyEstimate { key: Key::new(6, true), confidence: 0.7 }), ..Default::default() };
    lib.save_full_analysis(id, None, 0.0, &[1], 10.0, &musical).unwrap();
    let summary = lib.track(id).unwrap();
    assert_eq!((summary.key.as_deref(), summary.key_name.as_deref()), (Some("11A"), Some("F#m")));
    let stored = lib.analysis(id).unwrap().unwrap();
    assert_eq!(stored.key.unwrap().key, Key::new(6, true));
    assert!((stored.key.unwrap().confidence - 0.7).abs() < 1e-6);

    lib.save_analysis(id, None, 0.0, &[1], 10.0).unwrap();
    assert_eq!(lib.track(id).unwrap().key.as_deref(), Some("8A"), "falls back to the tag when no key was analysed");
}

#[test]
fn tag_keys_are_normalised_to_camelot() {
    let lib = Library::open_in_memory().unwrap();
    let table = [
        ("Am", Some(("8A", "Am"))),
        ("A minor", Some(("8A", "Am"))),
        ("8A", Some(("8A", "Am"))),
        ("Abm", Some(("1A", "G#m"))),
        ("G#m", Some(("1A", "G#m"))),
        ("C", Some(("8B", "C"))),
        ("Db", Some(("3B", "Db"))),
        ("F#", Some(("2B", "F#"))),
        ("1m", Some(("8A", "Am"))),
        ("1d", Some(("8B", "C"))),
        ("Bbm", Some(("3A", "Bbm"))),
        ("garbage", None),
        ("", None),
    ];
    for (i, (tag, expected)) in table.iter().enumerate() {
        let id = insert_track(&lib, &format!("t{i}.wav"), Some(tag));
        let t = lib.track(id).unwrap();
        assert_eq!((t.key.as_deref(), t.key_name.as_deref()), (expected.map(|e| e.0), expected.map(|e| e.1)), "tag {tag:?}");
    }
    let untagged = insert_track(&lib, "none.wav", None);
    assert_eq!(lib.track(untagged).unwrap().key, None);
}

#[test]
fn phrases_round_trip_and_are_replaced_on_reanalysis() {
    let lib = Library::open_in_memory().unwrap();
    let id = insert_track(&lib, "p.wav", None);
    let musical = MusicalAnalysis { phrases: sample_phrases(), ..Default::default() };
    lib.save_full_analysis(id, Some(Beatgrid { bpm: 128.0, first_beat_secs: 0.1 }), 0.9, &[1], 75.0, &musical).unwrap();
    assert_eq!(lib.phrases(id).unwrap(), sample_phrases());

    let shorter = MusicalAnalysis { phrases: sample_phrases()[..1].to_vec(), ..Default::default() };
    lib.save_full_analysis(id, None, 0.0, &[1], 75.0, &shorter).unwrap();
    assert_eq!(lib.phrases(id).unwrap().len(), 1);
    assert!(lib.phrases(9999).unwrap().is_empty());

    lib.set_beatgrid(id, Beatgrid { bpm: 130.0, first_beat_secs: 0.0 }).unwrap();
    assert!(lib.phrases(id).unwrap().is_empty(), "a manual grid change invalidates stored phrases");
}

#[test]
fn locked_beatgrid_is_only_reported_when_locked() {
    let lib = Library::open_in_memory().unwrap();
    let id = insert_track(&lib, "l.wav", None);
    lib.save_analysis(id, Some(Beatgrid { bpm: 120.0, first_beat_secs: 0.0 }), 0.5, &[1], 5.0).unwrap();
    assert_eq!(lib.locked_beatgrid(id).unwrap(), None);
    lib.set_beatgrid(id, Beatgrid { bpm: 121.0, first_beat_secs: 0.3 }).unwrap();
    assert_eq!(lib.locked_beatgrid(id).unwrap(), Some(Beatgrid { bpm: 121.0, first_beat_secs: 0.3 }));
}

#[test]
fn stale_analysis_versions_are_requeued_until_saved_again() {
    let lib = Library::open_in_memory().unwrap();
    let id = insert_track(&lib, "v.wav", None);
    lib.save_analysis(id, None, 0.0, &[1], 5.0).unwrap();
    assert!(lib.pending_analysis().unwrap().is_empty());
    assert!(lib.analysis(id).unwrap().unwrap().is_current());

    lib.conn.lock().execute("UPDATE analysis SET version = ?1 WHERE track_id = ?2", params![ANALYSIS_VERSION - 1, id]).unwrap();
    assert_eq!(lib.pending_analysis().unwrap(), vec![id]);
    assert_eq!(lib.track(id).unwrap().analysis, AnalysisState::Done);

    lib.save_analysis(id, None, 0.0, &[1], 5.0).unwrap();
    assert!(lib.pending_analysis().unwrap().is_empty());
}

#[test]
fn the_stored_grid_is_anchored_on_the_first_downbeat() {
    let lib = Library::open_in_memory().unwrap();
    let id = insert_track(&lib, "d.wav", None);
    let grid = Beatgrid { bpm: 120.0, first_beat_secs: 0.1 };
    lib.save_full_analysis(id, Some(grid), 0.9, &[1], 60.0, &MusicalAnalysis { downbeat_beat: Some(3), ..Default::default() })
        .unwrap();
    let stored = lib.analysis(id).unwrap().unwrap();
    assert_eq!(stored.downbeat_beat, Some(3));
    assert!((stored.grid.first_beat_secs - 1.6).abs() < 1e-9);
    assert_eq!(lib.beatgrid(id).unwrap(), Some(stored.grid));

    lib.save_analysis(id, Some(grid), 0.9, &[1], 60.0).unwrap();
    assert_eq!(lib.analysis(id).unwrap().unwrap().grid, grid, "an unknown bar phase leaves the grid as analysed");
}

#[test]
fn a_locked_grid_keeps_its_bar_phase_through_reanalysis() {
    let lib = Library::open_in_memory().unwrap();
    let id = insert_track(&lib, "k.wav", None);
    let user = Beatgrid { bpm: 124.0, first_beat_secs: 0.5 };
    lib.set_beatgrid(id, user).unwrap();
    assert_eq!(lib.downbeat_beat(id).unwrap(), Some(0));
    let musical = MusicalAnalysis { downbeat_beat: Some(2), ..Default::default() };
    lib.save_full_analysis(id, Some(Beatgrid { bpm: 124.0, first_beat_secs: 0.1 }), 0.9, &[1], 60.0, &musical).unwrap();
    assert_eq!(lib.beatgrid(id).unwrap(), Some(user));

    lib.conn.lock().execute("UPDATE analysis SET downbeat_beat = NULL WHERE track_id = ?1", params![id]).unwrap();
    lib.save_full_analysis(id, None, 0.0, &[1], 60.0, &musical).unwrap();
    assert_eq!(lib.downbeat_beat(id).unwrap(), Some(2), "a locked grid without a bar phase gets one once");
}

#[test]
fn migrates_a_version_three_library_and_requeues_it() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(crate::schema::MIGRATIONS[0]).unwrap();
    conn.execute_batch(crate::schema::MIGRATIONS[1]).unwrap();
    conn.pragma_update(None, "user_version", 2).unwrap();
    conn.execute("INSERT INTO tracks (id, path, title, file_type, added_at, analysis_state) VALUES (1, 'a.wav', 'a', 'wav', 0, 'done')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO analysis (track_id, version, bpm, first_beat_secs, confidence, grid_locked, waveform, analyzed_at)
         VALUES (1, 3, 128.0, 0.2, 0.9, 0, x'0102', 0)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO phrases (track_id, idx, start_secs, end_secs, kind, bars, energy) VALUES (1, 0, 0.2, 30.2, 'intro', 16, 0.5)", [])
        .unwrap();

    let lib = Library::with_connection(conn).unwrap();
    let stored = lib.analysis(1).unwrap().unwrap();
    assert_eq!((stored.downbeat_beat, stored.grid.first_beat_secs), (None, 0.2));
    assert_eq!(lib.phrases(1).unwrap()[0].fill_start_secs, None);
    assert_eq!(lib.pending_analysis().unwrap(), vec![1]);
}

#[test]
fn loudness_round_trips_and_silence_is_not_stored() {
    use deckforge_analysis::Loudness;
    let lib = Library::open_in_memory().unwrap();
    let id = insert_track(&lib, "loud.wav", None);
    let loud = Loudness { integrated_lufs: -9.5, sample_peak: 0.98 };
    lib.save_full_analysis(id, None, 0.0, &[1], 5.0, &MusicalAnalysis { loudness: Some(loud), ..Default::default() }).unwrap();
    assert_eq!(lib.analysis(id).unwrap().unwrap().loudness, Some(loud));
    let silent = Loudness { integrated_lufs: f64::NEG_INFINITY, sample_peak: 0.0 };
    lib.save_full_analysis(id, None, 0.0, &[1], 5.0, &MusicalAnalysis { loudness: Some(silent), ..Default::default() }).unwrap();
    assert_eq!(lib.analysis(id).unwrap().unwrap().loudness, None);
}

#[test]
fn folders_nest_and_cascade_without_touching_tracks() {
    let lib = Library::open_in_memory().unwrap();
    let t = insert_track(&lib, "a.wav", None);
    let folder = lib.create_playlist("Crates", None, true).unwrap();
    let inner = lib.create_playlist("Deep", Some(folder.id), false).unwrap();
    lib.add_to_playlist(inner.id, &[t]).unwrap();
    lib.add_to_playlist(folder.id, &[t]).unwrap();
    let all = lib.playlists().unwrap();
    let f = all.iter().find(|p| p.id == folder.id).unwrap();
    assert!(f.is_folder && f.track_count == 0);
    assert_eq!(all.iter().find(|p| p.id == inner.id).unwrap().parent_id, Some(folder.id));
    lib.delete_playlist(folder.id).unwrap();
    assert!(lib.playlists().unwrap().is_empty());
    assert_eq!(lib.track(t).unwrap().id, t);
}

#[test]
fn renaming_marks_the_playlist_renamed() {
    let lib = Library::open_in_memory().unwrap();
    let p = lib.create_playlist("x", None, false).unwrap();
    lib.rename_playlist(p.id, "y").unwrap();
    let renamed: bool =
        lib.conn.lock().query_row("SELECT renamed FROM playlists WHERE id = ?1", params![p.id], |r| r.get(0)).unwrap();
    assert!(renamed);
}
