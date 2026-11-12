use std::sync::Arc;

use deckforge_common::{Band, Beatgrid, CrossfaderCurve, EqCurve, TrackAudio, HOT_CUE_SLOTS};
use deckforge_engine::{
    create_engine, secondary_channel, Command, EngineHandle, LoadRequest, MixerParam, Routing, RtEngine, SecondaryShared,
    SecondarySource,
};

const SR: u32 = 48_000;
const BLOCK: usize = 512;
const CH: usize = 4;

fn tone(freq: f64, secs: f64, sr: u32) -> Arc<TrackAudio> {
    let n = (secs * sr as f64) as usize;
    let samples = (0..n)
        .flat_map(|i| {
            let s = ((std::f64::consts::TAU * freq * i as f64 / sr as f64).sin() * 0.5) as f32;
            [s, s]
        })
        .collect();
    Arc::new(TrackAudio::new(sr, samples))
}

fn rig() -> (EngineHandle, RtEngine) {
    let (h, mut rt) = create_engine(SR);
    rt.reconfigure(SR, Routing::for_channel_count(CH));
    (h, rt)
}

fn load(h: &EngineHandle, deck: usize, track: Arc<TrackAudio>, grid: Option<Beatgrid>) {
    let request = Box::new(LoadRequest { track, token: deck as u64 + 1, grid, cue_secs: Some(0.0), hot_cues: [None; HOT_CUE_SLOTS] });
    assert!(h.send(Command::Load { deck, request }));
}

/// Runs the engine and returns interleaved output.
fn run(rt: &mut RtEngine, secs: f64) -> Vec<f32> {
    let blocks = ((secs * SR as f64) as usize).div_ceil(BLOCK);
    let mut all = Vec::with_capacity(blocks * BLOCK * CH);
    let mut buf = vec![0.0; BLOCK * CH];
    for _ in 0..blocks {
        rt.process(&mut buf, CH);
        all.extend_from_slice(&buf);
    }
    all
}

/// Amplitude of a frequency in one output channel (Goertzel over the last half of the capture).
fn level(out: &[f32], channel: usize, freq: f64) -> f64 {
    let frames: Vec<f64> = out.chunks(CH).map(|f| f[channel] as f64).collect();
    let tail = &frames[frames.len() / 2..];
    let w = std::f64::consts::TAU * freq / SR as f64;
    let (mut re, mut im) = (0.0, 0.0);
    for (i, x) in tail.iter().enumerate() {
        re += x * (w * i as f64).cos();
        im += x * (w * i as f64).sin();
    }
    2.0 * (re * re + im * im).sqrt() / tail.len() as f64
}

fn mixer(h: &EngineHandle, channel: usize, param: MixerParam) {
    h.send(Command::Mixer { channel, param });
}

const A_HZ: f64 = 220.0;
const B_HZ: f64 = 1_250.0;

fn two_tone_rig() -> (EngineHandle, RtEngine) {
    let (h, rt) = rig();
    load(&h, 0, tone(A_HZ, 20.0, SR), None);
    load(&h, 1, tone(B_HZ, 20.0, SR), None);
    for ch in 0..2 {
        mixer(&h, ch, MixerParam::Fader(1.0));
    }
    h.send(Command::MasterLevel(0.8));
    (h, rt)
}

#[test]
fn both_decks_play_simultaneously_and_pause_independently() {
    let (h, mut rt) = two_tone_rig();
    h.send(Command::Play { deck: 0 });
    h.send(Command::Play { deck: 1 });
    let out = run(&mut rt, 1.0);
    assert!(level(&out, 0, A_HZ) > 0.15, "deck A audible");
    assert!(level(&out, 0, B_HZ) > 0.15, "deck B audible");
    let s = h.snapshot();
    assert!(s.decks[0].playing && s.decks[1].playing);
    assert!((s.decks[0].position_secs - 1.0).abs() < 0.05);

    h.send(Command::Pause { deck: 0 });
    let out = run(&mut rt, 0.5);
    assert!(level(&out, 0, A_HZ) < 0.005, "deck A paused");
    assert!(level(&out, 0, B_HZ) > 0.15, "deck B still playing");
    let s = h.snapshot();
    assert!(!s.decks[0].playing && s.decks[1].playing);
}

#[test]
fn crossfader_and_channel_faders_control_master() {
    let (h, mut rt) = two_tone_rig();
    h.send(Command::Play { deck: 0 });
    h.send(Command::Play { deck: 1 });
    h.send(Command::Crossfader(0.0));
    let out = run(&mut rt, 0.5);
    assert!(level(&out, 0, A_HZ) > 0.2);
    assert!(level(&out, 0, B_HZ) < 0.002, "B silenced at full-A crossfader");

    h.send(Command::Crossfader(1.0));
    let out = run(&mut rt, 0.5);
    assert!(level(&out, 0, A_HZ) < 0.002);
    assert!(level(&out, 0, B_HZ) > 0.2);

    h.send(Command::Crossfader(0.5));
    mixer(&h, 1, MixerParam::Fader(0.0));
    let out = run(&mut rt, 0.5);
    assert!(level(&out, 0, A_HZ) > 0.1);
    assert!(level(&out, 0, B_HZ) < 0.002, "B channel fader down");
}

#[test]
fn headphone_cue_is_independent_of_master() {
    let (h, mut rt) = two_tone_rig();
    h.send(Command::Play { deck: 0 });
    h.send(Command::Play { deck: 1 });
    mixer(&h, 1, MixerParam::Fader(0.0));
    mixer(&h, 1, MixerParam::Pfl(true));
    h.send(Command::HeadphoneMix(0.0));
    h.send(Command::Crossfader(0.0));
    let out = run(&mut rt, 0.6);
    assert!(level(&out, 0, B_HZ) < 0.002, "B not in master");
    assert!(level(&out, 0, A_HZ) > 0.2, "A in master");
    assert!(level(&out, 2, B_HZ) > 0.15, "B in headphones (pre-fader, ignores crossfader)");
    assert!(level(&out, 2, A_HZ) < 0.002, "A not cued");

    mixer(&h, 1, MixerParam::Fader(1.0));
    h.send(Command::Crossfader(0.5));
    let out = run(&mut rt, 0.6);
    assert!(level(&out, 0, B_HZ) > 0.15, "B enters master when fader raised");

    h.send(Command::HeadphoneMix(1.0));
    mixer(&h, 1, MixerParam::Pfl(false));
    let out = run(&mut rt, 0.6);
    assert!(level(&out, 2, A_HZ) > 0.1, "MIX at master side monitors the master");
}

#[test]
fn hardware_phones_mode_sends_pure_cue() {
    let (h, mut rt) = two_tone_rig();
    h.send(Command::HardwarePhones(true));
    h.send(Command::HeadphoneMix(1.0));
    h.send(Command::Play { deck: 0 });
    h.send(Command::Play { deck: 1 });
    mixer(&h, 0, MixerParam::Pfl(true));
    let out = run(&mut rt, 0.6);
    assert!(level(&out, 2, A_HZ) > 0.2);
    assert!(level(&out, 2, B_HZ) < 0.002, "master not blended in software");
}

#[test]
fn eq_kill_and_filter_affect_channel() {
    let (h, mut rt) = two_tone_rig();
    load(&h, 0, tone(60.0, 20.0, SR), None);
    h.send(Command::Play { deck: 0 });
    let out = run(&mut rt, 0.6);
    let open = level(&out, 0, 60.0);
    mixer(&h, 0, MixerParam::Eq(Band::Low, 0.0));
    let out = run(&mut rt, 0.6);
    assert!(level(&out, 0, 60.0) < open * 0.15, "EQ curve: LOW fully off is about -20 dB at 60 Hz (-26 dB at 20 Hz)");
    h.send(Command::EqCurve(EqCurve::Isolator));
    let out = run(&mut rt, 0.6);
    assert!(level(&out, 0, 60.0) < open * 0.05, "isolator curve: low kill removes 60 Hz");
    assert_eq!(h.snapshot().master.eq_curve, EqCurve::Isolator);
    mixer(&h, 0, MixerParam::Eq(Band::Low, 0.5));
    mixer(&h, 0, MixerParam::Filter(1.0));
    let out = run(&mut rt, 0.6);
    assert!(level(&out, 0, 60.0) < open * 0.01, "high-pass filter removes 60 Hz");
}

#[test]
fn cdj_cue_sets_previews_and_returns() {
    let (h, mut rt) = two_tone_rig();
    h.send(Command::SeekSecs { deck: 0, secs: 2.0 });
    run(&mut rt, 0.05);
    h.send(Command::CueDown { deck: 0 });
    run(&mut rt, 0.5);
    let s = h.snapshot().decks[0];
    assert!((s.cue_secs - 2.0).abs() < 0.01, "cue set at paused position");
    assert!(s.playing && s.previewing, "previewing while held");
    assert!(s.position_secs > 2.3);
    h.send(Command::CueUp { deck: 0 });
    run(&mut rt, 0.1);
    let s = h.snapshot().decks[0];
    assert!(!s.playing, "release pauses");
    assert!((s.position_secs - 2.0).abs() < 0.01, "release returns to cue");

    h.send(Command::Play { deck: 0 });
    run(&mut rt, 1.0);
    h.send(Command::CueDown { deck: 0 });
    run(&mut rt, 0.1);
    let s = h.snapshot().decks[0];
    assert!(!s.playing && (s.position_secs - 2.0).abs() < 0.01, "cue while playing returns and stops");
    h.send(Command::CueUp { deck: 0 });

    h.send(Command::CueDown { deck: 0 });
    run(&mut rt, 0.2);
    h.send(Command::Play { deck: 0 });
    h.send(Command::CueUp { deck: 0 });
    run(&mut rt, 0.2);
    assert!(h.snapshot().decks[0].playing, "PLAY during cue preview latches playback");
}

#[test]
fn hot_cues_set_jump_and_delete() {
    let grid = Some(Beatgrid { bpm: 120.0, first_beat_secs: 0.0 });
    let (h, mut rt) = rig();
    load(&h, 0, tone(A_HZ, 20.0, SR), grid);
    h.send(Command::Quantize(false));
    h.send(Command::Play { deck: 0 });
    run(&mut rt, 1.0);
    h.send(Command::HotCue { deck: 0, slot: 2, pressed: true });
    h.send(Command::HotCue { deck: 0, slot: 2, pressed: false });
    run(&mut rt, 2.0);
    let set_at = h.snapshot().decks[0].hot_cues[2].unwrap().position_secs;
    assert!((set_at - 1.0).abs() < 0.03);
    h.send(Command::HotCue { deck: 0, slot: 2, pressed: true });
    h.send(Command::HotCue { deck: 0, slot: 2, pressed: false });
    run(&mut rt, 0.02);
    let s = h.snapshot().decks[0];
    assert!(s.playing && (s.position_secs - set_at).abs() < 0.05, "jumped while playing");
    h.send(Command::HotCueDelete { deck: 0, slot: 2 });
    run(&mut rt, 0.02);
    assert!(h.snapshot().decks[0].hot_cues[2].is_none());
}

#[test]
fn quantized_hot_cue_snaps_to_beat() {
    let grid = Some(Beatgrid { bpm: 120.0, first_beat_secs: 0.1 });
    let (h, mut rt) = rig();
    load(&h, 0, tone(A_HZ, 20.0, SR), grid);
    h.send(Command::Play { deck: 0 });
    run(&mut rt, 1.37);
    h.send(Command::HotCue { deck: 0, slot: 0, pressed: true });
    run(&mut rt, 0.02);
    let at = h.snapshot().decks[0].hot_cues[0].unwrap().position_secs;
    assert!((at - 1.35).abs() < 1e-6 || (at - 1.6).abs() < 1e-6, "snapped to grid, got {at}");
}

#[test]
fn beat_loop_keeps_playhead_inside_loop() {
    let grid = Some(Beatgrid { bpm: 120.0, first_beat_secs: 0.0 });
    let (h, mut rt) = rig();
    load(&h, 0, tone(A_HZ, 30.0, SR), grid);
    h.send(Command::Play { deck: 0 });
    run(&mut rt, 1.1);
    h.send(Command::BeatLoop { deck: 0, beats: 4.0 });
    run(&mut rt, 0.05);
    let s = h.snapshot().decks[0];
    let (a, b) = (s.loop_in_secs.unwrap(), s.loop_out_secs.unwrap());
    assert!((a - 1.0).abs() < 1e-6 && (b - 3.0).abs() < 1e-6, "4 beats from the current beat");
    for _ in 0..20 {
        run(&mut rt, 0.37);
        let p = h.snapshot().decks[0].position_secs;
        assert!(p >= a - 0.01 && p < b + 0.02, "position {p} escaped loop");
    }
    h.send(Command::ReloopExit { deck: 0 });
    run(&mut rt, 3.0);
    assert!(h.snapshot().decks[0].position_secs > b, "exit lets playback continue");
}

#[test]
fn loop_pressed_just_before_the_downbeat_catches_the_bar() {
    let grid = Some(Beatgrid { bpm: 120.0, first_beat_secs: 0.0 });
    let (h, mut rt) = rig();
    load(&h, 0, tone(A_HZ, 30.0, SR), grid);
    h.send(Command::Play { deck: 0 });
    run(&mut rt, 1.88);
    h.send(Command::BeatLoop { deck: 0, beats: 4.0 });
    run(&mut rt, 0.02);
    let s = h.snapshot().decks[0];
    let (a, b) = (s.loop_in_secs.unwrap(), s.loop_out_secs.unwrap());
    assert!((a - 2.0).abs() < 1e-6 && (b - 4.0).abs() < 1e-6, "loops the coming bar, got {a}..{b}");
    assert!(s.position_secs < 2.0, "plays on into the bar instead of jumping back");
    for _ in 0..20 {
        run(&mut rt, 0.37);
        let p = h.snapshot().decks[0].position_secs;
        assert!(p >= a - 0.01 && p < b + 0.02, "position {p} escaped loop");
    }
}

#[test]
fn loop_pressed_on_the_last_beat_of_a_bar_starts_there() {
    let grid = Some(Beatgrid { bpm: 120.0, first_beat_secs: 0.0 });
    let (h, mut rt) = rig();
    load(&h, 0, tone(A_HZ, 30.0, SR), grid);
    h.send(Command::Play { deck: 0 });
    run(&mut rt, 1.55);
    h.send(Command::BeatLoop { deck: 0, beats: 4.0 });
    run(&mut rt, 0.02);
    let s = h.snapshot().decks[0];
    assert!((s.loop_in_secs.unwrap() - 1.5).abs() < 1e-6, "a deliberate beat-4 loop, got {:?}", s.loop_in_secs);
}

#[test]
fn manual_loop_in_out() {
    let (h, mut rt) = rig();
    load(&h, 0, tone(A_HZ, 30.0, SR), None);
    h.send(Command::Quantize(false));
    h.send(Command::Play { deck: 0 });
    run(&mut rt, 1.0);
    h.send(Command::LoopIn { deck: 0 });
    run(&mut rt, 0.5);
    h.send(Command::LoopOut { deck: 0 });
    run(&mut rt, 2.0);
    let s = h.snapshot().decks[0];
    assert!(s.loop_active);
    assert!(s.position_secs >= s.loop_in_secs.unwrap() - 0.01 && s.position_secs <= s.loop_out_secs.unwrap() + 0.02);
}

#[test]
fn sync_matches_tempo_and_phase() {
    let (h, mut rt) = rig();
    load(&h, 0, tone(A_HZ, 60.0, SR), Some(Beatgrid { bpm: 124.0, first_beat_secs: 0.05 }));
    load(&h, 1, tone(B_HZ, 60.0, SR), Some(Beatgrid { bpm: 120.0, first_beat_secs: 0.21 }));
    h.send(Command::SetTempo { deck: 0, ratio: 1.02 });
    h.send(Command::Play { deck: 0 });
    run(&mut rt, 1.0);
    h.send(Command::Play { deck: 1 });
    run(&mut rt, 0.73);
    h.send(Command::Sync { deck: 1, enabled: true });
    run(&mut rt, 3.0);
    let s = h.snapshot();
    assert_eq!(s.master_deck, Some(0));
    assert!((s.decks[1].bpm - s.decks[0].bpm).abs() < 0.05, "{} vs {}", s.decks[1].bpm, s.decks[0].bpm);
    let mut err = s.decks[0].beat_phase - s.decks[1].beat_phase;
    err -= err.round();
    assert!(err.abs() < 0.02, "phase error {err} beats");

    h.send(Command::SetTempo { deck: 0, ratio: 0.97 });
    run(&mut rt, 3.0);
    let s = h.snapshot();
    assert!((s.decks[1].bpm - s.decks[0].bpm).abs() < 0.05, "follows master tempo changes");
}

#[test]
fn key_lock_keeps_time_moving_at_tempo() {
    let (h, mut rt) = rig();
    load(&h, 0, tone(A_HZ, 30.0, SR), None);
    h.send(Command::SetTempoRange { deck: 0, range: deckforge_common::TempoRange::Sixteen });
    h.send(Command::SetKeyLock { deck: 0, enabled: true });
    h.send(Command::SetTempo { deck: 0, ratio: 1.1 });
    mixer(&h, 0, MixerParam::Fader(1.0));
    h.send(Command::Play { deck: 0 });
    let out = run(&mut rt, 4.0);
    let p = h.snapshot().decks[0].position_secs;
    assert!((p - 4.4).abs() < 0.1, "position {p}");
    assert!(level(&out, 0, A_HZ) > 0.15, "pitch preserved at 220 Hz");
    assert!(level(&out, 0, A_HZ * 1.1) < 0.05);
}

#[test]
fn track_end_stops_playback_and_load_while_playing_does_not_interrupt_other_deck() {
    let (h, mut rt) = two_tone_rig();
    load(&h, 0, tone(A_HZ, 0.5, SR), None);
    h.send(Command::Play { deck: 0 });
    h.send(Command::Play { deck: 1 });
    run(&mut rt, 1.0);
    let s = h.snapshot();
    assert!(!s.decks[0].playing && s.decks[0].at_end);
    load(&h, 0, tone(A_HZ, 10.0, SR), None);
    let out = run(&mut rt, 0.5);
    assert!(h.snapshot().decks[1].playing);
    assert!(level(&out, 0, B_HZ) > 0.15);
    assert!(h.collect_garbage() >= 1, "replaced track returned for off-thread release");
}

#[test]
fn startup_ramp_and_limiter_bound_output() {
    let (h, mut rt) = rig();
    load(&h, 0, tone(A_HZ, 5.0, SR), None);
    mixer(&h, 0, MixerParam::Fader(1.0));
    mixer(&h, 0, MixerParam::Trim(1.0));
    mixer(&h, 0, MixerParam::Eq(Band::Low, 1.0));
    h.send(Command::MasterLevel(1.0));
    h.send(Command::CrossfaderCurve(CrossfaderCurve::Sharp));
    h.send(Command::Play { deck: 0 });
    let out = run(&mut rt, 1.0);
    assert!(out.iter().all(|s| s.abs() <= 1.0));
    assert!(out[..CH * 64].iter().all(|s| s.abs() < 0.05), "output ramps in from silence");
    assert!(h.snapshot().master.clipping, "clip indicator reports pre-limiter overs");
}

#[test]
fn jog_scratch_moves_one_platter_revolution_per_1_8_seconds_and_releases_to_play() {
    use deckforge_engine::JogMode;
    let (h, mut rt) = rig();
    load(&h, 0, tone(A_HZ, 30.0, SR), None);
    h.send(Command::SeekSecs { deck: 0, secs: 5.0 });
    run(&mut rt, 0.05);
    h.send(Command::JogTouch { deck: 0, touched: true });
    for _ in 0..50 {
        h.send(Command::Jog { deck: 0, ticks: 720 / 50, mode: JogMode::Scratch });
        run(&mut rt, 0.01);
    }
    run(&mut rt, 0.3);
    let p = h.snapshot().decks[0].position_secs;
    assert!((p - 6.8).abs() < 0.15, "one revolution ≈ 1.8 s of audio, got {p}");
    let held = h.snapshot().decks[0].position_secs;
    run(&mut rt, 0.2);
    assert!((h.snapshot().decks[0].position_secs - held).abs() < 0.01, "holding the platter stops playback");
    h.send(Command::JogTouch { deck: 0, touched: false });
    run(&mut rt, 0.2);
    assert!(!h.snapshot().decks[0].playing, "paused deck stays paused after release");

    h.send(Command::Play { deck: 0 });
    run(&mut rt, 0.2);
    h.send(Command::JogTouch { deck: 0, touched: true });
    run(&mut rt, 0.3);
    let held = h.snapshot().decks[0].position_secs;
    run(&mut rt, 0.2);
    assert!((h.snapshot().decks[0].position_secs - held).abs() < 0.02, "touching a playing platter brakes it");
    h.send(Command::JogTouch { deck: 0, touched: false });
    run(&mut rt, 0.5);
    let s = h.snapshot().decks[0];
    assert!(s.playing && s.position_secs > held + 0.4, "release resumes playback");
}

#[test]
fn edge_bend_temporarily_speeds_up_and_decays() {
    use deckforge_engine::JogMode;
    let (h, mut rt) = rig();
    load(&h, 0, tone(A_HZ, 30.0, SR), None);
    h.send(Command::Play { deck: 0 });
    run(&mut rt, 1.0);
    let before = h.snapshot().decks[0].position_secs;
    for _ in 0..10 {
        h.send(Command::Jog { deck: 0, ticks: 20, mode: JogMode::Bend });
        run(&mut rt, 0.02);
    }
    let gained = h.snapshot().decks[0].position_secs - before - 0.2;
    assert!(gained > 0.005, "bend advanced the track faster than real time: {gained}");
    run(&mut rt, 2.0);
    let a = h.snapshot().decks[0].position_secs;
    run(&mut rt, 1.0);
    let b = h.snapshot().decks[0].position_secs;
    assert!((b - a - 1.0).abs() < 0.005, "bend decays back to normal speed");
}

struct SecondaryRun {
    primary: Vec<f32>,
    secondary: Vec<f32>,
}

fn run_with_secondary(rt: &mut RtEngine, shared: &Arc<SecondaryShared>, secs: f64) -> SecondaryRun {
    let (tap, mut renderer) = secondary_channel(shared.clone(), SR, SR, 16_384);
    rt.set_secondary_tap(Some(tap));
    let blocks = ((secs * SR as f64) as usize).div_ceil(BLOCK);
    let mut primary = Vec::with_capacity(blocks * BLOCK * CH);
    let mut secondary = Vec::with_capacity(blocks * BLOCK * 2);
    let (mut buf, mut sec_buf) = (vec![0.0; BLOCK * CH], vec![0.0; BLOCK * 2]);
    for _ in 0..blocks {
        rt.process(&mut buf, CH);
        renderer.render(&mut sec_buf);
        primary.extend_from_slice(&buf);
        secondary.extend_from_slice(&sec_buf);
    }
    SecondaryRun { primary, secondary }
}

/// Peak of one channel over the last quarter; lockstep runs let the drift loop shift pitch slightly, so no Goertzel here.
fn tail_peak(out: &[f32], channels: usize, channel: usize) -> f64 {
    let frames: Vec<f32> = out.chunks(channels).map(|f| f[channel]).collect();
    frames[frames.len() * 3 / 4..].iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64
}

fn cue_rig() -> (EngineHandle, RtEngine) {
    let (h, rt) = two_tone_rig();
    h.send(Command::Play { deck: 0 });
    h.send(Command::Play { deck: 1 });
    mixer(&h, 1, MixerParam::Fader(0.0));
    mixer(&h, 1, MixerParam::Pfl(true));
    h.send(Command::HeadphoneMix(0.0));
    h.send(Command::Crossfader(0.0));
    (h, rt)
}

#[test]
fn secondary_tap_mirrors_the_master_bus_at_unity_level() {
    let (_h, mut rt) = cue_rig();
    let shared = SecondaryShared::new(SecondarySource::Master, 0.8);
    let run = run_with_secondary(&mut rt, &shared, 3.0);
    let primary = tail_peak(&run.primary, CH, 0);
    assert!(primary > 0.4, "deck A audible on the primary master");
    for channel in 0..2 {
        let mirrored = tail_peak(&run.secondary, 2, channel);
        assert!((mirrored / primary - 1.0).abs() < 0.02, "master mirrored at unity on ch{channel}: {mirrored} vs {primary}");
    }
    assert!(tail_peak(&run.primary, CH, 2) < primary, "cue bus is a different signal from the master");
}

#[test]
fn secondary_tap_carries_the_cue_bus_when_selected() {
    let (_h, mut rt) = cue_rig();
    let shared = SecondaryShared::new(SecondarySource::Cue, 0.8);
    let run = run_with_secondary(&mut rt, &shared, 3.0);
    let phones = tail_peak(&run.primary, CH, 2);
    let master = tail_peak(&run.primary, CH, 0);
    let mirrored = tail_peak(&run.secondary, 2, 0);
    assert!(phones > 0.3 && (phones - master).abs() > 0.05, "the two buses differ: phones {phones}, master {master}");
    assert!((mirrored / phones - 1.0).abs() < 0.02, "cue mirrored at unity: {mirrored} vs {phones}");
}

#[test]
fn secondary_source_can_be_switched_while_running() {
    let (_h, mut rt) = cue_rig();
    let shared = SecondaryShared::new(SecondarySource::Master, 0.8);
    let (tap, mut renderer) = secondary_channel(shared.clone(), SR, SR, 16_384);
    rt.set_secondary_tap(Some(tap));
    let (mut buf, mut sec_buf) = (vec![0.0; BLOCK * CH], vec![0.0; BLOCK * 2]);
    let mut capture = |rt: &mut RtEngine, blocks: usize| {
        let mut out = Vec::new();
        for _ in 0..blocks {
            rt.process(&mut buf, CH);
            renderer.render(&mut sec_buf);
            out.extend_from_slice(&sec_buf);
        }
        out
    };
    let before = capture(&mut rt, 200);
    shared.set_source(SecondarySource::Cue);
    capture(&mut rt, 20);
    let after = capture(&mut rt, 200);
    let (master_peak, cue_peak) = (tail_peak(&before, 2, 0), tail_peak(&after, 2, 0));
    assert!(master_peak > 0.4, "master bus before the switch: {master_peak}");
    assert!((0.3..0.4).contains(&cue_peak), "cue bus after the switch: {cue_peak}");
}

#[test]
fn secondary_level_is_applied_to_the_mirrored_bus_only() {
    let (_h, mut rt) = cue_rig();
    let shared = SecondaryShared::new(SecondarySource::Master, 0.4);
    let run = run_with_secondary(&mut rt, &shared, 3.0);
    let primary = tail_peak(&run.primary, CH, 0);
    let mirrored = tail_peak(&run.secondary, 2, 0);
    assert!(primary > 0.4, "primary level untouched by the secondary level");
    assert!((mirrored / primary - 0.25).abs() < 0.01, "level 0.4 is a quarter of unity gain: {}", mirrored / primary);
}

#[test]
fn attaching_a_secondary_leaves_primary_master_and_phones_routing_bit_identical() {
    let render = |with_tap: bool| {
        let (_h, mut rt) = cue_rig();
        if with_tap {
            let shared = SecondaryShared::new(SecondarySource::Cue, 0.8);
            return run_with_secondary(&mut rt, &shared, 1.5).primary;
        }
        run(&mut rt, 1.5)
    };
    let (plain, tapped) = (render(false), render(true));
    assert_eq!(plain.len(), tapped.len());
    assert!(plain == tapped, "primary output changed when a secondary tap was attached");
    assert!(level(&plain, 2, B_HZ) > 0.15 && level(&plain, 0, A_HZ) > 0.2, "routing still master 1-2, phones 3-4");
}

#[test]
fn engine_keeps_feeding_the_tap_across_primary_reconfigure() {
    let (_h, mut rt) = cue_rig();
    let shared = SecondaryShared::new(SecondarySource::Master, 0.8);
    let (tap, mut renderer) = secondary_channel(shared.clone(), SR, SR, 16_384);
    rt.set_secondary_tap(Some(tap));
    let (mut buf, mut sec_buf) = (vec![0.0; BLOCK * CH], vec![0.0; BLOCK * 2]);
    for _ in 0..150 {
        rt.process(&mut buf, CH);
        renderer.render(&mut sec_buf);
    }
    assert!(renderer.is_playing());
    rt.reconfigure(SR, Routing::for_channel_count(2));
    let mut stereo = vec![0.0; BLOCK * 2];
    let mut tail = Vec::new();
    for _ in 0..300 {
        rt.process(&mut stereo, 2);
        renderer.render(&mut sec_buf);
        tail = sec_buf.clone();
    }
    assert!(renderer.is_playing(), "secondary stays attached across a primary reconfigure");
    assert_eq!(shared.stats().underruns, 0);
    assert!(tail.iter().any(|v| v.abs() > 0.05), "mirror still carries audio after the reconfigure");
}

#[test]
fn crossfader_off_lets_both_decks_through_wherever_it_sits() {
    let (h, mut rt) = two_tone_rig();
    h.send(Command::Play { deck: 0 });
    h.send(Command::Play { deck: 1 });
    h.send(Command::CrossfaderCurve(CrossfaderCurve::Off));
    for position in [0.0, 1.0] {
        h.send(Command::Crossfader(position));
        let out = run(&mut rt, 0.5);
        assert!(level(&out, 0, A_HZ) > 0.2, "A full with crossfader off at {position}");
        assert!(level(&out, 0, B_HZ) > 0.2, "B full with crossfader off at {position}");
    }
    let s = h.snapshot();
    assert_eq!(s.master.curve, CrossfaderCurve::Off);
    assert!(!s.master.crossfader_active);
}

#[test]
fn turning_the_crossfader_back_on_waits_for_it_to_pass_the_centre() {
    let (h, mut rt) = two_tone_rig();
    h.send(Command::Play { deck: 0 });
    h.send(Command::Play { deck: 1 });
    h.send(Command::CrossfaderCurve(CrossfaderCurve::Off));
    h.send(Command::Crossfader(0.0));
    run(&mut rt, 0.2);

    h.send(Command::CrossfaderCurve(CrossfaderCurve::EqualPower));
    let out = run(&mut rt, 0.5);
    assert!(level(&out, 0, B_HZ) > 0.2, "B keeps playing until the crossfader is picked up");
    assert!(!h.snapshot().master.crossfader_active);

    h.send(Command::Crossfader(0.3));
    let out = run(&mut rt, 0.5);
    assert!(level(&out, 0, B_HZ) > 0.2, "still on A's side, not picked up");

    h.send(Command::Crossfader(0.52));
    run(&mut rt, 0.1);
    assert!(h.snapshot().master.crossfader_active, "crossing the centre picks it up");

    h.send(Command::Crossfader(0.0));
    let out = run(&mut rt, 0.5);
    assert!(level(&out, 0, B_HZ) < 0.002, "crossfader works again");
}

#[test]
fn turning_the_crossfader_on_at_the_centre_engages_at_once() {
    let (h, mut rt) = two_tone_rig();
    h.send(Command::CrossfaderCurve(CrossfaderCurve::Off));
    h.send(Command::Crossfader(0.5));
    h.send(Command::CrossfaderCurve(CrossfaderCurve::Linear));
    run(&mut rt, 0.05);
    assert!(h.snapshot().master.crossfader_active);
}

fn thd(out: &[f32], channel: usize, freq: f64) -> f64 {
    let harmonics: f64 = (2..10).map(|h| level(out, channel, freq * h as f64).powi(2)).sum();
    harmonics.sqrt() / level(out, channel, freq)
}

#[test]
fn cranked_trim_and_master_are_limited_without_distortion() {
    let (h, mut rt) = rig();
    let loud = Arc::new(TrackAudio::new(SR, (0..SR as usize * 4).flat_map(|i| {
        let s = (std::f64::consts::TAU * 1_000.0 * i as f64 / SR as f64).sin() as f32;
        [s, s]
    }).collect()));
    load(&h, 0, loud, None);
    mixer(&h, 0, MixerParam::Fader(1.0));
    mixer(&h, 0, MixerParam::Trim(1.0));
    mixer(&h, 0, MixerParam::Pfl(true));
    h.send(Command::Crossfader(0.0));
    h.send(Command::MasterLevel(1.0));
    h.send(Command::HeadphoneMix(0.0));
    h.send(Command::HeadphoneLevel(1.0));
    h.send(Command::Play { deck: 0 });
    run(&mut rt, 2.5);
    let out = run(&mut rt, 1.024);
    for (channel, bus) in [(0, "master"), (2, "headphones")] {
        let distortion = thd(&out, channel, 1_000.0);
        assert!(distortion < 0.01, "{bus} THD {:.1} %", distortion * 100.0);
        assert!(level(&out, channel, 1_000.0) > 0.85, "{bus} stays loud");
    }
    assert!(out.iter().all(|x| x.abs() <= 1.0));
    assert!(h.snapshot().master.limiting);
}

#[test]
fn crossfader_off_survives_an_output_rate_change() {
    let (h, mut rt) = two_tone_rig();
    h.send(Command::Play { deck: 0 });
    h.send(Command::Play { deck: 1 });
    h.send(Command::CrossfaderCurve(CrossfaderCurve::Off));
    h.send(Command::Crossfader(0.0));
    run(&mut rt, 0.1);
    mixer(&h, 0, MixerParam::Fader(0.0));
    rt.reconfigure(44_100, Routing::for_channel_count(CH));
    let out = run(&mut rt, 0.5);
    let tail: Vec<f32> = out.chunks(CH).skip(out.len() / CH / 2).map(|f| f[0]).collect();
    let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
    assert!(rms > 0.2, "B still passes after the mixer is rebuilt, rms {rms}");
    assert!(!h.snapshot().master.crossfader_active);
}

