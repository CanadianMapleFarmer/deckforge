//! Verifies the JSON emitted by the backend matches the field names declared in the UI's types.ts.

use std::collections::BTreeSet;

use deckforge_common::{HotCue, TempoRange};
use deckforge_controller::{ControllerStatus, MidiDirection, MidiLogEntry};
use deckforge_engine::{
    EngineSnapshot, OutputDeviceInfo, OutputState, OutputStatus, Routing, SecondaryOutputRequest, SecondaryOutputStatus,
    SecondarySource, SecondaryState,
};
use serde_json::Value;

const TYPES_TS: &str = include_str!("../../src/lib/api/types.ts");

fn ts_fields(interface: &str) -> BTreeSet<String> {
    let start = TYPES_TS.find(&format!("export interface {interface} {{")).unwrap_or_else(|| panic!("{interface} missing"));
    let body = &TYPES_TS[start..];
    let body = &body[body.find('{').unwrap() + 1..body.find("\n}").unwrap()];
    body.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with("//") || l.is_empty() {
                return None;
            }
            l.split(':').next().map(|k| k.trim_end_matches('?').trim().to_string())
        })
        .collect()
}

fn json_fields(v: &Value) -> BTreeSet<String> {
    v.as_object().expect("object").keys().cloned().collect()
}

fn check(interface: &str, v: &Value) {
    assert_eq!(json_fields(v), ts_fields(interface), "{interface} field names differ between Rust and types.ts");
}

#[test]
fn engine_snapshot_matches_types_ts() {
    let mut snap = EngineSnapshot::default();
    snap.decks[0].hot_cues[0] = Some(HotCue { position_secs: 1.0, color: 0xFF0000 });
    snap.decks[0].tempo_range = TempoRange::Sixteen;
    let v = serde_json::to_value(snap).unwrap();
    check("EngineSnapshot", &v);
    check("DeckSnapshot", &v["decks"][0]);
    check("ChannelSnapshot", &v["channels"][0]);
    check("MasterSnapshot", &v["master"]);
    check("HotCue", &v["decks"][0]["hotCues"][0]);
    check("BeatFxSnapshot", &v["beatFx"]);
    assert_eq!(v["beatFx"]["kind"], "echo");
    assert_eq!(v["beatFx"]["target"], "master");
    assert_eq!(v["colorFx"], "filter");
    assert_eq!(v["decks"][0]["tempoRange"], "Sixteen");
    assert_eq!(v["master"]["curve"], "equal-power");
}

#[test]
fn beat_fx_enums_and_beats_match_types_ts() {
    use deckforge_common::{BeatFxKind, ColorFxKind, FxTarget, BEAT_FX_BEATS};

    let ts_union = |name: &str| -> BTreeSet<String> {
        let start = TYPES_TS.find(&format!("export type {name} =")).unwrap_or_else(|| panic!("{name} missing"));
        let line = TYPES_TS[start..].lines().next().unwrap();
        line.split('\'').skip(1).step_by(2).map(String::from).collect()
    };
    let json_strings = |values: Vec<Value>| -> BTreeSet<String> {
        values.into_iter().map(|v| v.as_str().unwrap().to_string()).collect()
    };
    let kinds = BeatFxKind::ALL.iter().map(|k| serde_json::to_value(k).unwrap()).collect();
    assert_eq!(json_strings(kinds), ts_union("BeatFxKind"));
    let targets = [FxTarget::A, FxTarget::B, FxTarget::Master].iter().map(|k| serde_json::to_value(k).unwrap()).collect();
    assert_eq!(json_strings(targets), ts_union("FxTarget"));
    let colors = [ColorFxKind::Filter, ColorFxKind::Space, ColorFxKind::DubEcho, ColorFxKind::Crush, ColorFxKind::Noise]
        .iter()
        .map(|k| serde_json::to_value(k).unwrap())
        .collect();
    assert_eq!(json_strings(colors), ts_union("ColorFxKind"));

    let start = TYPES_TS.find("export const BEAT_FX_BEATS = [").unwrap();
    let list = &TYPES_TS[start..][TYPES_TS[start..].find('[').unwrap() + 1..];
    let ts_beats: Vec<f64> = list[..list.find(']').unwrap()].split(',').map(|n| n.trim().parse().unwrap()).collect();
    assert_eq!(ts_beats, BEAT_FX_BEATS.to_vec());
}

#[test]
fn beat_fx_percent_contract_matches_types_ts() {
    use deckforge_common::{BeatFxKind, BEAT_FX_PERCENT_STEPS};

    let list_after = |marker: &str| -> Vec<String> {
        let start = TYPES_TS.find(marker).unwrap_or_else(|| panic!("{marker} missing"));
        let list = &TYPES_TS[start..][TYPES_TS[start..].find('[').unwrap() + 1..];
        list[..list.find(']').unwrap()].split(',').map(|n| n.trim().trim_matches('\'').to_string()).collect()
    };
    let kinds: Vec<String> =
        BeatFxKind::ALL.iter().filter(|k| k.uses_percent()).map(|k| serde_json::to_value(k).unwrap().as_str().unwrap().to_string()).collect();
    assert_eq!(list_after("export const BEAT_FX_PERCENT_KINDS = ["), kinds);
    let steps: Vec<f64> = list_after("export const BEAT_FX_PERCENT_STEPS = [").iter().map(|n| n.parse().unwrap()).collect();
    assert_eq!(steps, BEAT_FX_PERCENT_STEPS.to_vec());
}

fn ts_body(marker: &str) -> &'static str {
    let start = TYPES_TS.find(marker).unwrap_or_else(|| panic!("{marker} missing"));
    let rest = &TYPES_TS[start..];
    &rest[..rest.find(";\n").unwrap()]
}

fn split_top_level(text: &str) -> Vec<String> {
    let (mut parts, mut depth, mut quoted, mut cur) = (Vec::new(), 0i32, false, String::new());
    for c in text.chars() {
        match c {
            '\'' => quoted = !quoted,
            '[' if !quoted => depth += 1,
            ']' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts.into_iter().map(|p| p.trim().to_string()).collect()
}

#[derive(Debug, PartialEq)]
struct TsParam {
    id: String,
    label: String,
    min: f64,
    max: f64,
    default: f64,
    step: Option<f64>,
    unit: String,
    options: Vec<String>,
}

fn parse_ts_param(line: &str) -> TsParam {
    let inner = line.trim().trim_start_matches('{').trim_end_matches(',').trim_end_matches('}').trim();
    let mut fields = std::collections::BTreeMap::new();
    for part in split_top_level(inner) {
        let (k, v) = part.split_once(':').unwrap();
        fields.insert(k.trim().to_string(), v.trim().to_string());
    }
    let text = |k: &str| fields.get(k).map(|v| v.trim_matches('\'').to_string());
    let number = |k: &str| fields.get(k).map(|v| v.parse::<f64>().unwrap_or_else(|_| panic!("{k}: {v}")));
    TsParam {
        id: text("id").unwrap(),
        label: text("label").unwrap(),
        min: number("min").unwrap(),
        max: number("max").unwrap(),
        default: number("default").unwrap(),
        step: number("step"),
        unit: text("unit").unwrap_or_default(),
        options: fields
            .get("options")
            .map(|v| split_top_level(v.trim_start_matches('[').trim_end_matches(']')).iter().map(|o| o.trim_matches('\'').to_string()).collect())
            .unwrap_or_default(),
    }
}

fn kind_name(kind: deckforge_common::BeatFxKind) -> String {
    serde_json::to_value(kind).unwrap().as_str().unwrap().to_string()
}

#[test]
fn beat_fx_parameter_table_matches_types_ts() {
    use deckforge_common::{beat_fx_params, BeatFxKind, BEAT_FX_MAX_PARAMS};

    let mut table: std::collections::BTreeMap<String, Vec<TsParam>> = std::collections::BTreeMap::new();
    let mut current = None;
    for line in ts_body("export const BEAT_FX_PARAMS").lines().skip(1) {
        let line = line.trim();
        if let Some(name) = line.strip_suffix(": [],").or_else(|| line.strip_suffix(": [")) {
            table.insert(name.to_string(), Vec::new());
            current = Some(name.to_string());
        } else if line.starts_with("{ id:") {
            table.get_mut(current.as_ref().unwrap()).unwrap().push(parse_ts_param(line));
        }
    }
    assert_eq!(table.len(), BeatFxKind::ALL.len(), "every kind has an entry: {:?}", table.keys());
    for kind in BeatFxKind::ALL {
        let ts = &table[&kind_name(kind)];
        let rust: Vec<TsParam> = beat_fx_params(kind)
            .iter()
            .map(|p| TsParam {
                id: p.id.into(),
                label: p.label.into(),
                min: p.min as f64,
                max: p.max as f64,
                default: p.default as f64,
                step: p.step.map(f64::from),
                unit: p.unit.into(),
                options: p.options.iter().map(|o| o.to_string()).collect(),
            })
            .collect();
        assert_eq!(ts, &rust, "{kind:?} parameters differ between Rust and types.ts");
        assert!(ts.len() <= BEAT_FX_MAX_PARAMS);
    }
    let max: usize = ts_body("export const BEAT_FX_MAX_PARAMS").split('=').nth(1).unwrap().trim().parse().unwrap();
    assert_eq!(max, BEAT_FX_MAX_PARAMS);
}

#[test]
fn color_fx_parameter_table_matches_types_ts() {
    use deckforge_common::{color_fx_params, ColorFxKind, COLOR_FX_MAX_PARAMS};

    let mut table: std::collections::BTreeMap<String, Vec<TsParam>> = std::collections::BTreeMap::new();
    let mut current = None;
    for line in ts_body("export const COLOR_FX_PARAMS").lines().skip(1) {
        let line = line.trim();
        if let Some(name) = line.strip_suffix(": [],").or_else(|| line.strip_suffix(": [")) {
            table.insert(name.to_string(), Vec::new());
            current = Some(name.to_string());
        } else if line.starts_with("{ id:") {
            table.get_mut(current.as_ref().unwrap()).unwrap().push(parse_ts_param(line));
        }
    }
    assert_eq!(table.len(), ColorFxKind::ALL.len(), "every kind has an entry: {:?}", table.keys());
    for kind in ColorFxKind::ALL {
        let name = serde_json::to_value(kind).unwrap().as_str().unwrap().to_string();
        let rust: Vec<TsParam> = color_fx_params(kind)
            .iter()
            .map(|p| TsParam {
                id: p.id.into(),
                label: p.label.into(),
                min: p.min as f64,
                max: p.max as f64,
                default: p.default as f64,
                step: p.step.map(f64::from),
                unit: p.unit.into(),
                options: p.options.iter().map(|o| o.to_string()).collect(),
            })
            .collect();
        assert_eq!(table[&name], rust, "{kind:?} parameters differ between Rust and types.ts");
        assert!(rust.len() <= COLOR_FX_MAX_PARAMS);
    }
    let max: usize = ts_body("export const COLOR_FX_MAX_PARAMS").split('=').nth(1).unwrap().trim().parse().unwrap();
    assert_eq!(max, COLOR_FX_MAX_PARAMS);
}

#[test]
fn beat_fx_kind_helpers_and_triggers_match_types_ts() {
    use deckforge_common::{BeatFxKind, FxTrigger};

    let names = |text: &str| -> Vec<String> { text.split('\'').skip(1).step_by(2).map(String::from).collect() };
    let kinds_where = |f: fn(BeatFxKind) -> bool| -> Vec<String> { BeatFxKind::ALL.iter().filter(|k| f(**k)).map(|k| kind_name(*k)).collect() };
    assert_eq!(names(ts_body("export const BEAT_FX_THROW_KINDS")), kinds_where(BeatFxKind::has_throw));
    assert_eq!(names(ts_body("export const BEAT_FX_FREEZE_KINDS")), kinds_where(BeatFxKind::has_freeze));
    assert_eq!(names(ts_body("export const BEAT_FX_RELEASE_KINDS")), kinds_where(BeatFxKind::has_release));

    let options_body = ts_body("export const BEAT_FX_KIND_BEATS");
    for kind in BeatFxKind::ALL {
        let line = options_body.lines().find(|l| l.trim_start().starts_with(&format!("{}:", kind_name(kind))));
        let listed: Option<Vec<f64>> = line.map(|l| {
            let list = &l[l.find('[').unwrap() + 1..l.find(']').unwrap()];
            list.split(',').map(|n| n.trim().parse().unwrap()).collect()
        });
        match listed {
            Some(values) => assert_eq!(values, kind.beat_options().to_vec(), "{kind:?}"),
            None => assert_eq!(kind.beat_options(), &deckforge_common::BEAT_FX_BEATS[..], "{kind:?} uses the shared divisions"),
        }
    }

    let primary_body = ts_body("export const BEAT_FX_PRIMARY_PARAM");
    for kind in BeatFxKind::ALL {
        let listed = primary_body
            .split_once('{')
            .map(|(_, rest)| rest)
            .and_then(|rest| rest.split(',').find(|part| part.trim().starts_with(&format!("{}:", kind_name(kind)))))
            .map(|part| part.split(':').nth(1).unwrap().trim().trim_end_matches('}').trim().parse::<usize>().unwrap());
        assert_eq!(listed, kind.primary_param(), "{kind:?}");
    }

    let ts_actions: std::collections::BTreeSet<String> = {
        let start = TYPES_TS.find("export type FxTriggerAction =").unwrap();
        TYPES_TS[start..].lines().next().unwrap().split('\'').skip(1).step_by(2).map(String::from).collect()
    };
    let rust_actions: std::collections::BTreeSet<String> = [FxTrigger::ThrowOn, FxTrigger::ThrowOff, FxTrigger::FreezeOn, FxTrigger::FreezeOff, FxTrigger::Release]
        .iter()
        .map(|t| serde_json::to_value(t).unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(ts_actions, rust_actions);
}

#[test]
fn beat_fx_snapshot_carries_the_selected_kinds_parameters_and_hold_state() {
    let mut snap = EngineSnapshot::default();
    snap.beat_fx.held = true;
    snap.beat_fx.ascend_progress = Some(0.25);
    snap.beat_fx.ascend_landing_bar = Some(17);
    let v = serde_json::to_value(snap).unwrap();
    check("BeatFxSnapshot", &v["beatFx"]);
    assert_eq!(v["beatFx"]["params"].as_array().unwrap().len(), deckforge_common::BEAT_FX_MAX_PARAMS);
    assert_eq!(v["beatFx"]["params"][0], 42.0);
    assert_eq!(v["colorFxParams"].as_array().unwrap().len(), deckforge_common::COLOR_FX_MAX_PARAMS);
    assert_eq!(v["beatFx"]["held"], true);
    assert_eq!(v["beatFx"]["ascendProgress"], 0.25);
    assert!(v["beatFx"]["ascendBars"].is_null());
    assert_eq!(v["beatFx"]["ascendLandingBar"], 17);
}

#[test]
fn status_types_match_types_ts() {
    let routing = Routing::for_channel_count(4);
    check("Routing", &serde_json::to_value(routing).unwrap());
    assert_eq!(serde_json::to_value(routing).unwrap()["mode"], "master-and-phones");
    let status = OutputStatus {
        state: OutputState::Running,
        active: None,
        last_error: None,
        callbacks: 0,
        last_callback_frames: 0,
        stream_errors: 0,
        xruns: 0,
        secondary: Some(SecondaryOutputStatus {
            device_id: "d".into(),
            device_name: "n".into(),
            source: SecondarySource::Cue,
            level: 0.8,
            state: SecondaryState::Lost,
            sample_rate: 44_100,
            latency_ms: 20.0,
            drift_ppm: 1.0,
            underruns: 0,
            channels: [0, 1],
        }),
    };
    let v = serde_json::to_value(status).unwrap();
    check("OutputStatus", &v);
    check("SecondaryOutputStatus", &v["secondary"]);
    assert_eq!((v["secondary"]["source"].as_str(), v["secondary"]["state"].as_str()), (Some("cue"), Some("lost")));
    let request = SecondaryOutputRequest { device_id: "d".into(), source: SecondarySource::Master, level: 0.8, channels: [0, 1] };
    let v = serde_json::to_value(&request).unwrap();
    check("SecondaryOutputRequest", &v);
    assert_eq!(serde_json::from_value::<SecondaryOutputRequest>(v).unwrap(), request);
    let defaulted: SecondaryOutputRequest = serde_json::from_str(r#"{"deviceId":"d"}"#).unwrap();
    assert_eq!((defaulted.channels, defaulted.level), ([0, 1], 0.8));
    let ts_unions = |name: &str| -> BTreeSet<String> {
        let start = TYPES_TS.find(&format!("export type {name} =")).unwrap_or_else(|| panic!("{name} missing"));
        TYPES_TS[start..].lines().next().unwrap().split('\'').skip(1).step_by(2).map(String::from).collect()
    };
    assert_eq!(ts_unions("SecondarySource"), BTreeSet::from(["master".to_string(), "cue".to_string()]));
    assert_eq!(ts_unions("SecondaryState"), BTreeSet::from(["running".to_string(), "lost".to_string()]));
    let dev = OutputDeviceInfo { id: "x".into(), name: "y".into(), max_channels: 4, sample_rates: vec![], is_default: false };
    check("OutputDeviceInfo", &serde_json::to_value(dev).unwrap());
    check("ControllerStatus", &serde_json::to_value(ControllerStatus::default()).unwrap());
    let entry = MidiLogEntry { time_ms: 0, direction: MidiDirection::In, bytes: vec![], summary: String::new() };
    let v = serde_json::to_value(entry).unwrap();
    check("MidiLogEntry", &v);
    assert_eq!(v["direction"], "in");
}

#[test]
fn library_types_match_types_ts() {
    let lib = deckforge_library::Library::open_in_memory().unwrap();
    let dir = std::env::temp_dir().join(format!("deckforge-contract-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("t.wav");
    let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&wav, spec).unwrap();
    for _ in 0..800 {
        w.write_sample(0i16).unwrap();
    }
    w.finalize().unwrap();
    lib.import_paths(std::slice::from_ref(&dir));
    let track = &lib.tracks("", None).unwrap()[0];
    check("TrackSummary", &serde_json::to_value(track).unwrap());
    let pl = lib.create_playlist("x", None, false).unwrap();
    check("Playlist", &serde_json::to_value(pl).unwrap());
    check("ImportResult", &serde_json::to_value(deckforge_library::ImportResult::default()).unwrap());
    let report = deckforge_library::PlaylistImportReport {
        playlists: vec![deckforge_library::PlaylistImportSummary { id: 1, name: "x".into(), found: 1, total: 2, missing: vec!["y.mp3".into()] }],
        ..Default::default()
    };
    let v = serde_json::to_value(report).unwrap();
    check("PlaylistImportReport", &v);
    check("PlaylistImportSummary", &v["playlists"][0]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn spotify_types_match_types_ts() {
    use deckforge_spotify::{SpotifyPlaylist, SpotifyStatus, SpotifyTrack, SpotifyUser};

    let user = SpotifyUser { id: "u".into(), display_name: "U".into(), product: None, image_url: None };
    check("SpotifyUser", &serde_json::to_value(&user).unwrap());
    let status = SpotifyStatus { client_id: None, connected: false, user: Some(user), redirect_uri: String::new(), last_error: None };
    let v = serde_json::to_value(status).unwrap();
    check("SpotifyStatus", &v);
    check("SpotifyUser", &v["user"]);
    let playlist = SpotifyPlaylist { id: "p".into(), name: "n".into(), owner: "o".into(), track_count: 1, image_url: None };
    check("SpotifyPlaylist", &serde_json::to_value(playlist).unwrap());
    let track = SpotifyTrack {
        id: "t".into(),
        name: "n".into(),
        artists: vec![],
        album: "a".into(),
        duration_ms: 1,
        isrc: None,
        image_url: None,
        local_track_id: None,
        match_score: None,
    };
    check("SpotifyTrack", &serde_json::to_value(track).unwrap());
    let playlist = deckforge_library::Library::open_in_memory().unwrap().create_playlist("x", None, false).unwrap();
    let result = deckforge_desktop_lib::spotify::SpotifyImportResult { playlist, matched: 1, unmatched: 0 };
    let v = serde_json::to_value(result).unwrap();
    check("SpotifyImportResult", &v);
    check("Playlist", &v["playlist"]);
}

#[test]
fn key_and_phrase_types_match_types_ts() {
    use deckforge_analysis::{Key, KeyEstimate, Phrase, PhraseKind};
    use deckforge_library::{Library, MusicalAnalysis};

    let lib = Library::open_in_memory().unwrap();
    let dir = std::env::temp_dir().join(format!("deckforge-contract-keys-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("k.wav");
    let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&wav, spec).unwrap();
    for _ in 0..800 {
        w.write_sample(0i16).unwrap();
    }
    w.finalize().unwrap();
    lib.import_paths(std::slice::from_ref(&dir));
    let id = lib.tracks("", None).unwrap()[0].id;
    let phrases = vec![
        Phrase { start_secs: 0.0, end_secs: 15.0, kind: PhraseKind::Intro, bars: 8, energy: 0.2, fill_start_secs: None },
        Phrase { start_secs: 15.0, end_secs: 30.0, kind: PhraseKind::Drop, bars: 8, energy: 1.0, fill_start_secs: Some(28.0) },
    ];
    let musical = MusicalAnalysis { key: Some(KeyEstimate { key: Key::new(9, true), confidence: 0.8 }), phrases: phrases.clone(), ..Default::default() };
    lib.save_full_analysis(id, None, 0.0, &[1], 1.0, &musical).unwrap();

    let v = serde_json::to_value(lib.track(id).unwrap()).unwrap();
    check("TrackSummary", &v);
    assert_eq!((v["key"].as_str(), v["keyName"].as_str()), (Some("8A"), Some("Am")));

    let stored = lib.phrases(id).unwrap();
    assert_eq!(stored, phrases);
    let v = serde_json::to_value(stored[1]).unwrap();
    check("Phrase", &v);
    assert_eq!(v["kind"], "drop");
    assert_eq!(v["startSecs"], 15.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn fx_control_snapshots_match_types_ts() {
    use deckforge_common::{BeatFxKind, PadVoice, ReleaseFxKind};

    let mut snap = EngineSnapshot::default();
    snap.pad_fx[0].voice = Some(PadVoice { kind: BeatFxKind::Echo, beats: 0.5, percent: 50.0, level: 0.7, param0: Some(55.0) });
    snap.pad_fx[1].release = Some(ReleaseFxKind::Backspin);
    let v = serde_json::to_value(snap).unwrap();
    check("PadFxSnapshot", &v["padFx"][0]);
    check("PadVoice", &v["padFx"][0]["voice"]);
    check("ReleaseFxSnapshot", &v["releaseFx"]);
    check("SmartCfxSnapshot", &v["smartCfx"]);
    assert_eq!(v["padFx"][1]["release"], "backspin");
    assert_eq!(v["releaseFx"]["kind"], "echoOut");
    assert_eq!(v["releaseFx"]["beats"], 2.0);
    assert_eq!(v["smartCfx"]["preset"], "echoWash");
}

#[test]
fn pad_fx_banks_match_types_ts() {
    use deckforge_common::{PadFxAction, PAD_FX1_BANK, PAD_FX1_SHIFT_BANK, PAD_FX2_BEATS, PAD_FX2_FAVOURITES, PAD_FX2_PERCENTS};

    let rust_line = |slot: &deckforge_common::PadFxSlot| match slot.action {
        PadFxAction::Momentary { voice } => {
            let kind = serde_json::to_value(voice.kind).unwrap();
            let param = voice.param0.map_or("null".to_string(), |p| format!("{p}"));
            format!("hold('{}', {kind}, {}, {}, {}, {param})", slot.label, voice.beats, voice.percent, voice.level).replace('"', "'")
        }
        PadFxAction::OneShot { release, beats } => {
            format!("shot('{}', {}, {beats})", slot.label, serde_json::to_value(release).unwrap()).replace('"', "'")
        }
    };
    for (marker, bank) in [("export const PAD_FX1_BANK: PadFxSlot[] = [", &PAD_FX1_BANK), ("export const PAD_FX1_SHIFT_BANK: PadFxSlot[] = [", &PAD_FX1_SHIFT_BANK)] {
        let body = ts_body(marker);
        let ts: Vec<String> = body.lines().skip(1).map(|l| l.trim().trim_end_matches(',').to_string()).filter(|l| l.starts_with("hold") || l.starts_with("shot")).collect();
        let rust: Vec<String> = bank.iter().map(rust_line).collect();
        assert_eq!(ts, rust, "{marker}");
    }
    let numbers = |marker: &str| -> Vec<f64> {
        let body = ts_body(marker);
        body[body.find('[').unwrap() + 1..body.rfind(']').unwrap()].split(',').map(|n| n.trim().parse().unwrap()).collect()
    };
    assert_eq!(numbers("export const PAD_FX2_BEATS = ["), PAD_FX2_BEATS.to_vec());
    assert_eq!(numbers("export const PAD_FX2_PERCENTS = ["), PAD_FX2_PERCENTS.to_vec());
    let favourites = ts_body("export const PAD_FX2_FAVOURITES: BeatFxKind[] = [");
    let ts: Vec<String> = favourites[favourites.find("= [").unwrap() + 3..favourites.rfind(']').unwrap()].split(',').map(|k| k.trim().trim_matches('\'').to_string()).collect();
    let rust: Vec<String> = PAD_FX2_FAVOURITES.iter().map(|k| serde_json::to_value(k).unwrap().as_str().unwrap().to_string()).collect();
    assert_eq!(ts, rust);
}

#[test]
fn fx_control_state_matches_types_ts() {
    use deckforge_controller::{AppAction, FxControlState, FxHud, FxHudControl, FxKnobTarget, FxPickup, HeldPad};
    use deckforge_controller::flx4::PadMode;

    let state = FxControlState {
        shift: true,
        knob_target: FxKnobTarget::Param,
        focus: Some(0),
        pickup: Some(FxPickup { target: FxKnobTarget::Param, deck: None, index: Some(0), value: 0.4, knob: 0.9 }),
        pad_modes: [PadMode::PadFx1, PadMode::PadFx2],
        held: [Some(HeldPad { pad: 2, shift: false, mode: PadMode::PadFx1 }), None],
        smart_arming: [false, true],
        hud: Some(FxHud { seq: 3, control: FxHudControl::SmartPreset, deck: None }),
    };
    let v = serde_json::to_value(AppAction::FxControl(state)).unwrap();
    assert_eq!(v["type"], "fxControl");
    let mut fields = json_fields(&v);
    fields.remove("type");
    assert_eq!(fields, ts_fields("FxControlState"));
    check("FxPickup", &v["pickup"]);
    check("FxHud", &v["hud"]);
    check("HeldPad", &v["held"][0]);
    assert_eq!(v["padModes"][1], "padFx2");
    assert_eq!(v["knobTarget"], "param");
    assert_eq!(v["hud"]["control"], "smartPreset");
}

#[test]
fn history_and_recording_types_match_types_ts() {
    use deckforge_library::history::{SetDetail, SetEntry, SetRecording, SetSummary};
    use deckforge_recorder::{RecorderStatus, RecordingSettings};
    let summary = SetSummary { id: 1, name: "Friday".into(), started_at: 0, ended_at: Some(1), duration_secs: 1.0, track_count: 1, has_recording: true, live: false };
    let entry = SetEntry {
        id: 1,
        track_id: Some(2),
        title: "T".into(),
        artist: "A".into(),
        path: "/a.mp3".into(),
        key: Some("8A".into()),
        bpm: Some(124.0),
        audible_secs: 0.0,
        took_over_secs: Some(1.0),
        teased: false,
    };
    let recording = SetRecording { id: 1, path: "/r.wav".into(), started_secs: 0.0, stopped_secs: Some(1.0), format: "wav".into() };
    let detail = serde_json::to_value(SetDetail { summary, entries: vec![entry], recordings: vec![recording] }).unwrap();
    check("SetDetail", &detail);
    check("SetSummary", &detail["summary"]);
    check("SetEntry", &detail["entries"][0]);
    check("SetRecording", &detail["recordings"][0]);
    let settings = serde_json::to_value(RecordingSettings::default()).unwrap();
    check("RecordingSettings", &settings);
    check("MarkerFormats", &settings["markers"]);
    assert_eq!(settings["format"], "wav");
    check("RecorderStatus", &serde_json::to_value(RecorderStatus::default()).unwrap());
    assert_eq!(serde_json::to_value(deckforge_library::history::ExportFormat::FcpXml).unwrap(), "fcpXml");
}
