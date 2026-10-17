use std::sync::Arc;
use std::time::{Duration, Instant};

use deckforge_common::{Beatgrid, HotCue, TempoRange, TrackAudio, HOT_CUE_SLOTS};
use deckforge_engine::{create_engine, Command, EngineHandle, JogMode, LoadRequest, MixerParam, Routing, RtEngine};

const SR: u32 = 48_000;
const SRC: u32 = 44_100;
const BLOCK: usize = 256;
const CH: usize = 4;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn unit(&mut self) -> f64 {
        self.next() as f64 / (1u64 << 31) as f64
    }
}

fn sine(freq: f64, secs: f64) -> Arc<TrackAudio> {
    let n = (secs * SRC as f64) as usize;
    Arc::new(TrackAudio::new(SRC, (0..n).flat_map(|i| { let s = (std::f64::consts::TAU * freq * i as f64 / SRC as f64).sin() as f32 * 0.3; [s, s] }).collect()))
}

fn request(track: Arc<TrackAudio>, token: u64) -> Box<LoadRequest> {
    let mut hot_cues = [None; HOT_CUE_SLOTS];
    for (slot, cue) in hot_cues.iter_mut().enumerate().take(4) {
        *cue = Some(HotCue { position_secs: 4.0 + slot as f64 * 7.5, color: 0 });
    }
    Box::new(LoadRequest { track, token, grid: Some(Beatgrid { bpm: 120.0, first_beat_secs: 0.0 }), cue_secs: Some(1.0), hot_cues })
}

fn rig(fader_a: f32) -> (EngineHandle, RtEngine, Arc<TrackAudio>) {
    let (h, mut rt) = create_engine(SR);
    rt.reconfigure(SR, Routing::for_channel_count(CH));
    let a = sine(440.0, 120.0);
    h.send(Command::Load { deck: 0, request: request(a.clone(), 1) });
    h.send(Command::Load { deck: 1, request: request(sine(330.0, 600.0), 2) });
    h.send(Command::Mixer { channel: 0, param: MixerParam::Fader(fader_a) });
    h.send(Command::Mixer { channel: 1, param: MixerParam::Fader(1.0) });
    h.send(Command::SetTempoRange { deck: 0, range: TempoRange::Sixteen });
    h.send(Command::Play { deck: 0 });
    h.send(Command::Play { deck: 1 });
    (h, rt, a)
}

/// One random performance action on `deck`; returns its name for the report.
fn chaos(h: &EngineHandle, rng: &mut Lcg, deck: usize, track: &Arc<TrackAudio>, touched: &mut bool, only: Option<u64>) -> &'static str {
    let ticks = rng.below(60) as i32 - 30;
    match only.unwrap_or_else(|| rng.below(24)) {
        0..=4 => { h.send(Command::Jog { deck, ticks, mode: JogMode::Bend }); "nudge" }
        5 => { h.send(Command::Jog { deck, ticks: ticks * 2, mode: JogMode::FastSearch }); "fast search" }
        6 => { *touched = !*touched; h.send(Command::JogTouch { deck, touched: *touched }); "jog touch" }
        7..=8 => { h.send(Command::Jog { deck, ticks, mode: JogMode::Scratch }); "scratch" }
        9 => { h.send(Command::SetKeyLock { deck, enabled: rng.below(2) == 0 }); "key lock" }
        10..=11 => { h.send(Command::SetPitchFader { deck, position: rng.unit() * 2.0 - 1.0 }); "tempo" }
        12 => { let slot = rng.below(4) as usize; h.send(Command::HotCue { deck, slot, pressed: true }); h.send(Command::HotCue { deck, slot, pressed: false }); "hot cue" }
        13 => { h.send(Command::BeatJump { deck, beats: [-4.0, -1.0, 1.0, 4.0][rng.below(4) as usize] }); "beat jump" }
        14 => { h.send(Command::BeatLoop { deck, beats: [0.25, 1.0, 4.0][rng.below(3) as usize] }); "beat loop" }
        15 => { h.send(Command::ReloopExit { deck }); "reloop/exit" }
        16 => { h.send(Command::TogglePlay { deck }); "play/pause" }
        17 => { h.send(Command::CueDown { deck }); h.send(Command::CueUp { deck }); "cue" }
        18 => { let on = rng.below(2) == 0; h.send(Command::Sync { deck, enabled: on }); if on { "sync on" } else { "sync off" } }
        19 => { h.send(Command::SeekSecs { deck, secs: rng.unit() * 110.0 }); "seek" }
        20 => { h.send(Command::LoopScale { deck, factor: if rng.below(2) == 0 { 0.5 } else { 2.0 } }); "loop scale" }
        21 => { h.send(Command::SetVinylMode { deck, enabled: rng.below(3) != 0 }); "vinyl" }
        22 => { h.send(Command::Play { deck }); "play" }
        _ => { h.send(Command::Load { deck, request: request(track.clone(), 10 + rng.below(1000)) }); "load" }
    }
}

struct Report {
    worst_jump: f32,
    worst_at: usize,
    last_ops: Vec<(usize, &'static str)>,
    non_finite: usize,
    over_full_scale: usize,
    worst_block: Duration,
}

fn soak(fader_a: f32, chaos_decks: &[usize], secs: f64, seed: u64) -> Report {
    soak_only(fader_a, chaos_decks, secs, seed, None, false)
}

fn soak_only(fader_a: f32, chaos_decks: &[usize], secs: f64, seed: u64, only: Option<u64>, key_lock: bool) -> Report {
    let (h, mut rt, track) = rig(fader_a);
    h.send(Command::SetKeyLock { deck: 0, enabled: key_lock });
    let mut rng = Lcg(seed);
    let mut buf = vec![0.0f32; BLOCK * CH];
    let mut prev = [0.0f32; 2];
    let mut report = Report { worst_jump: 0.0, worst_at: 0, last_ops: Vec::new(), non_finite: 0, over_full_scale: 0, worst_block: Duration::ZERO };
    let mut ops: Vec<(usize, &'static str)> = Vec::new();
    let mut touched = [false; 2];
    let blocks = (secs * SR as f64) as usize / BLOCK;
    for b in 0..blocks {
        if b > 40 {
            for &d in chaos_decks {
                if rng.below(3) == 0 {
                    ops.push((b, chaos(&h, &mut rng, d, &track, &mut touched[d], only)));
                }
            }
        }
        let t = Instant::now();
        rt.process(&mut buf, CH);
        report.worst_block = report.worst_block.max(t.elapsed());
        h.collect_garbage();
        for f in buf.chunks(CH) {
            let x = f[0];
            if !x.is_finite() {
                report.non_finite += 1;
                continue;
            }
            if x.abs() > 1.0 {
                report.over_full_scale += 1;
            }
            let jump = (x - 2.0 * prev[1] + prev[0]).abs();
            if jump > report.worst_jump && b > 40 {
                report.worst_jump = jump;
                report.worst_at = b;
                report.last_ops = ops.iter().rev().take_while(|(ob, _)| b - ob < 4).cloned().collect();
            }
            prev = [prev[1], x];
        }
    }
    report
}

fn steady_bound() -> f32 {
    0.3 * (std::f64::consts::TAU * 330.0 / SR as f64).powi(2) as f32
}

#[test]
fn a_silent_deck_cannot_disturb_the_master_whatever_it_does() {
    for seed in [1, 2, 3] {
        let r = soak(0.0, &[0], 40.0, seed);
        assert_eq!(r.non_finite, 0);
        assert!(r.worst_jump < steady_bound() * 2.0, "seed {seed}: master second difference {} at block {} after {:?}", r.worst_jump, r.worst_at, r.last_ops);
        eprintln!("seed {seed}: worst block {:?}", r.worst_block);
    }
}

#[test]
fn an_audible_deck_under_abuse_never_breaks_the_master() {
    for seed in [11, 12, 13] {
        let r = soak(1.0, &[0], 40.0, seed);
        assert_eq!(r.non_finite, 0, "seed {seed}");
        assert_eq!(r.over_full_scale, 0, "seed {seed}");
        eprintln!("seed {seed}: worst second difference {:.4} at block {} after {:?}, worst block {:?}", r.worst_jump, r.worst_at, r.last_ops, r.worst_block);
        assert!(r.worst_jump < 0.1, "seed {seed}: click of {} at block {} after {:?}", r.worst_jump, r.worst_at, r.last_ops);
    }
}

#[test]
fn both_decks_under_abuse_stay_within_the_block_budget() {
    let budget = Duration::from_secs_f64(BLOCK as f64 / SR as f64);
    for seed in [21, 22] {
        let r = soak(1.0, &[0, 1], 40.0, seed);
        assert_eq!(r.non_finite, 0, "seed {seed}");
        assert_eq!(r.over_full_scale, 0, "seed {seed}");
        eprintln!("seed {seed}: worst block {:?} of {:?}, worst second difference {:.4} after {:?}", r.worst_block, budget, r.worst_jump, r.last_ops);
        assert!(r.worst_block < budget / 2, "seed {seed}: block took {:?}", r.worst_block);
    }
}

#[test]
#[ignore]
fn per_action_click_survey() {
    for key_lock in [false, true] {
        for op in [0, 5, 6, 7, 9, 10, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 23] {
            let r = soak_only(1.0, &[0], 10.0, 7 + op, Some(op), key_lock);
            eprintln!("key lock {key_lock} op {op:2} {:>12}: worst {:.4} finite {} over {}", r.last_ops.first().map_or("-", |o| o.1), r.worst_jump, r.non_finite == 0, r.over_full_scale);
        }
    }
}

#[test]
#[ignore]
fn replay() {
    let seed: u64 = std::env::var("SEED").unwrap().parse().unwrap();
    let at: usize = std::env::var("BLOCK").unwrap().parse().unwrap();
    let both = seed % 2 == 1;
    let (h, mut rt, track) = rig(1.0);
    h.send(Command::SetKeyLock { deck: 0, enabled: false });
    let mut rng = Lcg(seed);
    let mut buf = vec![0.0f32; BLOCK * CH];
    let mut touched = [false; 2];
    let mut prev = [0.0f32; 2];
    for b in 0..at + 2 {
        let mut op = String::new();
        if b > 40 {
            for d in if both { vec![0, 1] } else { vec![0] } {
                if rng.below(3) == 0 {
                    op += &format!("{d}:{} ", chaos(&h, &mut rng, d, &track, &mut touched[d], None));
                }
            }
        }
        rt.process(&mut buf, CH);
        let x: Vec<f32> = buf.chunks(CH).map(|f| f[0]).collect();
        let mut worst = (0.0f32, 0usize);
        for (i, v) in x.iter().enumerate() {
            let j = (v - 2.0 * prev[1] + prev[0]).abs();
            if j > worst.0 {
                worst = (j, i);
            }
            prev = [prev[1], *v];
        }
        if b + 6 > at || op.contains("load") {
            let s = &h.snapshot().decks;
            eprintln!("block {b} {op:30} A playing {} pos {:.4} tempo {:.4} keylock {} | B playing {} keylock {} | worst {:.4} at {}", s[0].playing, s[0].position_secs, s[0].tempo_ratio, s[0].key_lock, s[1].playing, s[1].key_lock, worst.0, worst.1);
            if b == at {
                let k = worst.1;
                eprintln!("  around: {:?}", &x[k.saturating_sub(4)..(k + 4).min(x.len())]);
            }
        }
    }
}

#[test]
fn spamming_cue_on_one_deck_keeps_the_master_clean() {
    for key_lock in [false, true] {
        for gap in [1usize, 2, 3, 5] {
            let (h, mut rt, _) = rig(1.0);
            h.send(Command::SetKeyLock { deck: 0, enabled: key_lock });
            let mut buf = vec![0.0f32; BLOCK * CH];
            let mut prev = [0.0f32; 2];
            let mut worst = 0.0f32;
            for b in 0..1500 {
                if b > 40 && b % gap == 0 {
                    match (b / gap) % 4 {
                        0 => h.send(Command::CueDown { deck: 0 }),
                        1 => h.send(Command::CueUp { deck: 0 }),
                        2 => h.send(Command::Play { deck: 0 }),
                        _ => h.send(Command::BeatJump { deck: 0, beats: 1.0 }),
                    };
                }
                rt.process(&mut buf, CH);
                for f in buf.chunks(CH) {
                    assert!(f[0].is_finite());
                    if b > 40 {
                        worst = worst.max((f[0] - 2.0 * prev[1] + prev[0]).abs());
                    }
                    prev = [prev[1], f[0]];
                }
            }
            assert!(worst < 0.1, "key lock {key_lock}, every {gap} blocks: click of {worst}");
        }
    }
}

#[test]
fn back_to_back_jumps_inside_one_crossfade_do_not_click() {
    for key_lock in [false, true] {
        let (h, mut rt, _) = rig(1.0);
        h.send(Command::SetKeyLock { deck: 0, enabled: key_lock });
        let mut buf = vec![0.0f32; 32 * CH];
        let mut prev = [0.0f32; 2];
        let mut worst = 0.0f32;
        for b in 0..6000 {
            if b > 400 && b % 5 < 2 {
                h.send(Command::SeekSecs { deck: 0, secs: 5.0 + (b % 97) as f64 * 0.0371 });
                if b % 2 == 0 {
                    h.send(Command::SeekSecs { deck: 0, secs: 9.0 + (b % 89) as f64 * 0.0213 });
                }
            }
            rt.process(&mut buf, CH);
            for f in buf.chunks(CH) {
                if b > 400 {
                    worst = worst.max((f[0] - 2.0 * prev[1] + prev[0]).abs());
                }
                prev = [prev[1], f[0]];
            }
        }
        assert!(worst < 0.1, "key lock {key_lock}: click of {worst}");
    }
}

#[test]
fn loading_onto_a_playing_deck_fades_the_old_track_out() {
    for key_lock in [false, true] {
        let (h, mut rt, track) = rig(1.0);
        h.send(Command::SetKeyLock { deck: 0, enabled: key_lock });
        h.send(Command::Mixer { channel: 1, param: MixerParam::Fader(0.0) });
        let mut buf = vec![0.0f32; BLOCK * CH];
        let mut prev = [0.0f32; 2];
        let mut worst = 0.0f32;
        for b in 0..400 {
            if b % 50 == 49 {
                h.send(Command::Load { deck: 0, request: request(track.clone(), 100 + b as u64) });
                if b % 100 == 49 {
                    h.send(Command::Play { deck: 0 });
                }
            }
            rt.process(&mut buf, CH);
            h.collect_garbage();
            for f in buf.chunks(CH) {
                if b > 40 {
                    worst = worst.max((f[0] - 2.0 * prev[1] + prev[0]).abs());
                }
                prev = [prev[1], f[0]];
            }
        }
        assert!(worst < 0.02, "key lock {key_lock}: click of {worst}");
    }
}

#[test]
#[ignore]
fn long_soak() {
    let budget = Duration::from_secs_f64(BLOCK as f64 / SR as f64);
    for seed in 100..140u64 {
        let decks: &[usize] = if seed % 2 == 0 { &[0] } else { &[0, 1] };
        let r = soak(1.0, decks, 60.0, seed);
        let ok = r.non_finite == 0 && r.over_full_scale == 0 && r.worst_jump < 0.1 && r.worst_block < budget / 2;
        eprintln!("{} seed {seed} decks {decks:?}: click {:.4} after {:?}, worst block {:?}", if ok { "ok  " } else { "FAIL" }, r.worst_jump, r.last_ops, r.worst_block);
    }
}
