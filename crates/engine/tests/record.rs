use std::sync::Arc;

use deckforge_common::{TrackAudio, HOT_CUE_SLOTS};
use deckforge_engine::{create_engine, record_channel, Command, EngineHandle, LoadRequest, MixerParam, RecordFrame, RecordTapSlot, Routing, RtEngine};

const SR: u32 = 48_000;
const BLOCK: usize = 256;
const CH: usize = 4;

fn sine(freq: f64, amp: f32) -> Arc<TrackAudio> {
    let n = SR as usize * 20;
    Arc::new(TrackAudio::new(SR, (0..n).flat_map(|i| { let s = (std::f64::consts::TAU * freq * i as f64 / SR as f64).sin() as f32 * amp; [s, s] }).collect()))
}

fn rig(amp: f32) -> (EngineHandle, RtEngine) {
    let (h, mut rt) = create_engine(SR);
    rt.reconfigure(SR, Routing::for_channel_count(CH));
    for (deck, freq) in [(0, 220.0), (1, 330.0)] {
        h.send(Command::Load { deck, request: Box::new(LoadRequest { track: sine(freq, amp), token: deck as u64 + 1, grid: None, cue_secs: Some(0.0), hot_cues: [None; HOT_CUE_SLOTS] }) });
        h.send(Command::Mixer { channel: deck, param: MixerParam::Fader(1.0) });
        h.send(Command::Play { deck });
    }
    (h, rt)
}

fn run(rt: &mut RtEngine, blocks: usize) {
    let mut buf = vec![0.0; BLOCK * CH];
    for _ in 0..blocks {
        rt.process(&mut buf, CH);
    }
}

fn drain(rx: &mut rtrb::Consumer<RecordFrame>) -> Vec<RecordFrame> {
    let mut out = Vec::new();
    while let Ok(f) = rx.pop() {
        out.push(f);
    }
    out
}

fn audio(frames: &[RecordFrame]) -> Vec<[f32; 2]> {
    frames.iter().filter_map(|f| if let RecordFrame::Audio(a) = f { Some(*a) } else { None }).collect()
}

fn record(h: &EngineHandle, rt: &mut RtEngine, capacity: usize, blocks: usize) -> (Vec<RecordFrame>, Arc<deckforge_engine::RecordShared>) {
    let (tap, mut rx, shared) = record_channel(capacity);
    h.send(Command::SetRecordTap(RecordTapSlot::new(Some(tap))));
    run(rt, blocks);
    (drain(&mut rx), shared)
}

#[test]
fn the_recording_ignores_the_master_knob() {
    let mut takes = Vec::new();
    for level in [0.2, 1.0] {
        let (h, mut rt) = rig(0.3);
        h.send(Command::MasterLevel(level));
        run(&mut rt, 40);
        let (frames, _) = record(&h, &mut rt, 1 << 16, 40);
        takes.push(audio(&frames));
    }
    assert!(takes[0].len() >= 40 * BLOCK);
    assert_eq!(takes[0], takes[1]);
    assert!(takes[0].iter().any(|f| f[0].abs() > 0.2));
}

#[test]
fn the_recording_includes_fader_and_crossfader() {
    let (h, mut rt) = rig(0.3);
    run(&mut rt, 40);
    h.send(Command::Mixer { channel: 0, param: MixerParam::Fader(0.0) });
    h.send(Command::Crossfader(0.0));
    run(&mut rt, 40);
    let (frames, _) = record(&h, &mut rt, 1 << 16, 40);
    let peak = audio(&frames).iter().map(|f| f[0].abs()).fold(0.0, f32::max);
    assert!(peak < 1e-4, "deck A faded and deck B crossfaded out, yet the recording peaks at {peak}");
}

#[test]
fn two_loud_decks_are_limited_not_clipped() {
    let (h, mut rt) = rig(0.95);
    h.send(Command::MasterLevel(0.1));
    run(&mut rt, 40);
    let (frames, _) = record(&h, &mut rt, 1 << 16, 80);
    let a = audio(&frames);
    assert!(a.iter().all(|f| f[0].abs() <= 1.0 && f[1].abs() <= 1.0));
    assert!(a.iter().any(|f| f[0].abs() > 0.9), "the limiter keeps it loud");
}

#[test]
fn installing_a_tap_reports_its_start_clock() {
    let (h, mut rt) = rig(0.3);
    run(&mut rt, 10);
    let before = h.snapshot().clock_secs;
    let (frames, shared) = record(&h, &mut rt, 1 << 16, 10);
    assert!(matches!(frames[0], RecordFrame::Rate(SR)));
    let start = shared.start_clock().expect("started");
    assert!((start - before).abs() < 1e-9, "{start} vs {before}");
    assert!((h.snapshot().clock_secs - before - 10.0 * BLOCK as f64 / SR as f64).abs() < 1e-9);
}

#[test]
fn a_full_ring_reports_a_gap_instead_of_blocking() {
    let (h, mut rt) = rig(0.3);
    let (tap, mut rx, shared) = record_channel(BLOCK * 2);
    h.send(Command::SetRecordTap(RecordTapSlot::new(Some(tap))));
    run(&mut rt, 6);
    drain(&mut rx);
    run(&mut rt, 1);
    let frames = drain(&mut rx);
    let gap: u32 = frames.iter().filter_map(|f| if let RecordFrame::Gap(n) = f { Some(*n) } else { None }).sum();
    assert!(gap > 0 && shared.dropped() > 0, "{frames:?}");
}

#[test]
fn a_rate_change_is_marked_and_the_clock_keeps_counting() {
    let (h, mut rt) = rig(0.3);
    let (tap, mut rx, _) = record_channel(1 << 16);
    h.send(Command::SetRecordTap(RecordTapSlot::new(Some(tap))));
    run(&mut rt, 10);
    let clock = h.snapshot().clock_secs;
    rt.reconfigure(44_100, Routing::for_channel_count(CH));
    run(&mut rt, 10);
    let frames = drain(&mut rx);
    assert!(frames.iter().any(|f| matches!(f, RecordFrame::Rate(44_100))));
    let after = h.snapshot().clock_secs;
    assert!((after - clock - 10.0 * BLOCK as f64 / 44_100.0).abs() < 1e-9, "{after} {clock}");
}

#[test]
fn removing_the_tap_stops_the_stream() {
    let (h, mut rt) = rig(0.3);
    let (tap, mut rx, _) = record_channel(1 << 16);
    h.send(Command::SetRecordTap(RecordTapSlot::new(Some(tap))));
    run(&mut rt, 4);
    h.send(Command::SetRecordTap(RecordTapSlot::new(None)));
    run(&mut rt, 4);
    drain(&mut rx);
    run(&mut rt, 4);
    assert!(drain(&mut rx).is_empty());
    assert!(h.collect_garbage() >= 1, "the old tap is released off the audio thread");
}
