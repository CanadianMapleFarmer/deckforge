use std::sync::Arc;

use deckforge_common::{Beatgrid, HotCue, TempoRange, TrackAudio, HOT_CUE_SLOTS};
use deckforge_engine::{create_engine, Command, EngineHandle, JogMode, LoadRequest, MixerParam, Routing, RtEngine};

const SR: u32 = 48_000;
const SRC: u32 = 44_100;
const BLOCK: usize = 512;
const CH: usize = 4;

fn track(secs: f64, sample: impl Fn(f64) -> f32) -> Arc<TrackAudio> {
    let n = (secs * SRC as f64) as usize;
    Arc::new(TrackAudio::new(SRC, (0..n).flat_map(|i| { let s = sample(i as f64 / SRC as f64); [s, s] }).collect()))
}

fn sine(t: f64) -> f32 {
    (std::f64::consts::TAU * 440.0 * t).sin() as f32 * 0.4
}

/// A 1 kHz burst at the start of every `every` seconds.
fn bursts(every: f64) -> impl Fn(f64) -> f32 {
    move |t| {
        let phase = t % every;
        if phase < 0.03 { (std::f64::consts::TAU * 1000.0 * phase).sin() as f32 * 0.5 } else { 0.0 }
    }
}

fn rig(track: Arc<TrackAudio>, grid: Option<Beatgrid>, hot_cues: [Option<HotCue>; HOT_CUE_SLOTS], tempo: f64) -> (EngineHandle, RtEngine) {
    let (h, mut rt) = create_engine(SR);
    rt.reconfigure(SR, Routing::for_channel_count(CH));
    h.send(Command::Load { deck: 0, request: Box::new(LoadRequest { track, token: 1, grid, cue_secs: Some(0.0), hot_cues }) });
    h.send(Command::SetTempoRange { deck: 0, range: TempoRange::Sixteen });
    h.send(Command::SetKeyLock { deck: 0, enabled: true });
    h.send(Command::SetTempo { deck: 0, ratio: tempo });
    h.send(Command::Mixer { channel: 0, param: MixerParam::Fader(1.0) });
    h.send(Command::Play { deck: 0 });
    (h, rt)
}

fn run(rt: &mut RtEngine, secs: f64) -> Vec<f32> {
    let blocks = ((secs * SR as f64) as usize).div_ceil(BLOCK);
    let mut out = Vec::with_capacity(blocks * BLOCK);
    let mut buf = vec![0.0; BLOCK * CH];
    for _ in 0..blocks {
        rt.process(&mut buf, CH);
        out.extend(buf.chunks(CH).map(|f| f[0]));
    }
    out
}

fn max_second_difference(x: &[f32]) -> f32 {
    x.windows(3).map(|w| (w[0] - 2.0 * w[1] + w[2]).abs()).fold(0.0, f32::max)
}

fn onsets(x: &[f32]) -> Vec<usize> {
    let win = 48;
    let env: Vec<f32> = x.windows(win).map(|w| w.iter().map(|v| v.abs()).fold(0.0, f32::max)).collect();
    let peak = env.iter().cloned().fold(0.0, f32::max);
    let mut found = Vec::new();
    let mut armed = true;
    for (i, e) in env.iter().enumerate() {
        if armed && *e > peak * 0.5 {
            found.push(i + win);
            armed = false;
        } else if *e < peak * 0.05 {
            armed = true;
        }
    }
    found
}

#[test]
fn jog_nudges_at_unity_tempo_do_not_click() {
    let (h, mut rt) = rig(track(20.0, sine), None, [None; HOT_CUE_SLOTS], 1.0);
    let mut out = run(&mut rt, 0.5);
    for i in 0..60 {
        let ticks = if (i / 10) % 2 == 0 { 25 } else { -25 };
        h.send(Command::Jog { deck: 0, ticks, mode: JogMode::Bend });
        out.extend(run(&mut rt, 0.02));
    }
    out.extend(run(&mut rt, 1.5));
    let steady = 0.4 * (std::f64::consts::TAU * 440.0 / SR as f64).powi(2) as f32;
    let worst = max_second_difference(&out[BLOCK * 8..]);
    assert!(worst < steady * 4.0, "second difference {worst} vs steady sine {steady}");
}

#[test]
fn key_locked_loop_repeats_at_the_loop_length() {
    let grid = Some(Beatgrid { bpm: 120.0, first_beat_secs: 0.0 });
    let tempo = 1.06;
    let (h, mut rt) = rig(track(20.0, bursts(2.0)), grid, [None; HOT_CUE_SLOTS], tempo);
    run(&mut rt, 0.1);
    h.send(Command::BeatLoop { deck: 0, beats: 1.0 });
    let out = run(&mut rt, 3.0);
    let heard = onsets(&out);
    assert!(heard.len() >= 5, "{heard:?}");
    let expected = 0.5 / tempo * SR as f64;
    for pair in heard.windows(2) {
        let err_ms = (pair[1] - pair[0]) as f64 - expected;
        assert!(err_ms.abs() / SR as f64 * 1000.0 < 3.0, "loop period {} frames, expected {expected:.0}", pair[1] - pair[0]);
    }
}

#[test]
fn hot_cue_on_a_beat_plays_it_at_once() {
    let mut cues = [None; HOT_CUE_SLOTS];
    cues[0] = Some(HotCue { position_secs: 3.0, color: 0 });
    let (h, mut rt) = rig(track(20.0, bursts(1.0)), None, cues, 1.04);
    run(&mut rt, 1.3);
    h.send(Command::HotCue { deck: 0, slot: 0, pressed: true });
    h.send(Command::HotCue { deck: 0, slot: 0, pressed: false });
    let out = run(&mut rt, 0.3);
    let first = onsets(&out)[0];
    assert!(first < (0.005 * SR as f64) as usize, "hot cue heard after {:.1} ms", first as f64 / SR as f64 * 1000.0);
    assert!((h.snapshot().decks[0].position_secs - 3.0 - 0.3 * 1.04).abs() < 0.02);
}

#[test]
fn scratch_handover_under_key_lock_does_not_click() {
    let (h, mut rt) = rig(track(20.0, sine), None, [None; HOT_CUE_SLOTS], 1.05);
    let mut out = run(&mut rt, 0.5);
    for _ in 0..3 {
        h.send(Command::JogTouch { deck: 0, touched: true });
        for _ in 0..15 {
            h.send(Command::Jog { deck: 0, ticks: 720 / 50, mode: JogMode::Scratch });
            out.extend(run(&mut rt, 0.02));
        }
        h.send(Command::JogTouch { deck: 0, touched: false });
        out.extend(run(&mut rt, 0.4));
    }
    let worst = max_second_difference(&out[BLOCK * 8..]);
    assert!(worst < 0.02, "second difference {worst}");
}

#[test]
fn key_lock_keeps_pitch_after_an_output_rate_change() {
    let (h, mut rt) = rig(track(20.0, sine), None, [None; HOT_CUE_SLOTS], 1.1);
    run(&mut rt, 0.3);
    rt.reconfigure(96_000, Routing::for_channel_count(CH));
    let blocks = 96_000 * 2 / BLOCK;
    let mut buf = vec![0.0; BLOCK * CH];
    let mut out = Vec::new();
    for _ in 0..blocks {
        rt.process(&mut buf, CH);
        out.extend(buf.chunks(CH).map(|f| f[0]));
    }
    let tail = &out[out.len() / 2..];
    let crossings = tail.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f64 / (tail.len() as f64 / 96_000.0);
    assert!((crossings - 440.0).abs() < 3.0, "{crossings} Hz at 96 kHz");
    let p = h.snapshot().decks[0].position_secs;
    assert!((p - (0.3 + 2.0) * 1.1).abs() < 0.05, "position {p}");
}

#[test]
fn one_jog_turn_nudges_a_useful_amount() {
    for key_lock in [false, true] {
        let (h, mut rt) = rig(track(20.0, sine), None, [None; HOT_CUE_SLOTS], 1.0);
        h.send(Command::SetKeyLock { deck: 0, enabled: key_lock });
        run(&mut rt, 0.5);
        let before = h.snapshot().decks[0].position_secs;
        let mut ticks = 0;
        for _ in 0..94 {
            h.send(Command::Jog { deck: 0, ticks: 7, mode: JogMode::Bend });
            ticks += 7;
            run(&mut rt, 0.01);
        }
        run(&mut rt, 1.0);
        let elapsed = (94 + 94) as f64 * BLOCK as f64 / SR as f64;
        let nudge = h.snapshot().decks[0].position_secs - before - elapsed;
        let per_turn = nudge / (ticks as f64 / deckforge_engine::JOG_TICKS_PER_REV);
        assert!((0.12..0.16).contains(&per_turn), "key lock {key_lock}: {per_turn:.3} s per turn");
    }
}
