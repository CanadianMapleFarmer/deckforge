use std::collections::VecDeque;
use std::time::Instant;

use deckforge_common::{clamp_beat_fx_percent, snap_beat_fx_beats_for, FxTarget, SharedTrack, DECK_COUNT};
use deckforge_dsp::{AscendLanding, BackwashArm, Smoothed, Transport};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::anchors::BarMap;
use crate::command::{Command, LoadRequest};
use crate::deck::{Deck, MAX_BLOCK};
use crate::mixer::Mixer;
use crate::record::{RecordTap, RecordTapSlot};
use crate::secondary::{SecondarySource, SecondaryTap};
use crate::snapshot::EngineSnapshot;

const COMMAND_CAPACITY: usize = 4096;
const GARBAGE_CAPACITY: usize = 64;
const MAX_COMMANDS_PER_BLOCK: usize = 512;
/// Commands for a deck that arrive while it fades out for a load; beyond this the load completes at once.
const HELD_COMMANDS: usize = 256;
const STARTUP_FADE_MS: f32 = 400.0;
const SYNC_CORRECTION_SECS: f64 = 0.4;
const SYNC_MAX_NUDGE: f64 = 0.03;
const DEFAULT_FX_BPM: f64 = 120.0;
const BACKWASH_RENDER_SPEED: usize = 16;
const BACKWASH_URGENT_SPEED: usize = 64;
const BACKWASH_MIN_LEAD_BEATS: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoutingMode {
    /// Master on one stereo pair, headphones on another pair of the same device.
    #[default]
    MasterAndPhones,
    /// Master only; headphone cueing unavailable.
    MasterOnly,
    /// Stereo-only device: master summed to mono on the left, cue mono on the right.
    SplitMono,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Routing {
    pub mode: RoutingMode,
    pub master: [usize; 2],
    pub phones: [usize; 2],
}

impl Routing {
    pub fn for_channel_count(channels: usize) -> Self {
        if channels >= 4 {
            Self { mode: RoutingMode::MasterAndPhones, master: [0, 1], phones: [2, 3] }
        } else {
            Self { mode: RoutingMode::MasterOnly, master: [0, 1.min(channels.saturating_sub(1))], phones: [0, 0] }
        }
    }

    pub fn cue_available(&self) -> bool {
        self.mode != RoutingMode::MasterOnly
    }
}

/// Real-time half of the engine. Owned by the audio callback; never blocks or allocates per block.
/// Heap objects released by the audio thread and dropped on a control thread.
pub enum Garbage {
    Track(SharedTrack),
    Request(Box<LoadRequest>),
    Phrases(Box<[f64]>),
    RecordTap(Box<RecordTap>),
    RecordSlot(RecordTapSlot),
}

pub struct RtEngine {
    sample_rate: f32,
    decks: [Deck; DECK_COUNT],
    mixer: Mixer,
    commands: rtrb::Consumer<Command>,
    garbage: rtrb::Producer<Garbage>,
    snapshot: triple_buffer::Input<EngineSnapshot>,
    routing: Routing,
    quantize: bool,
    master_deck: Option<usize>,
    output_fade: Smoothed,
    frames_processed: u64,
    load: f32,
    tap: Option<SecondaryTap>,
    record: Option<Box<RecordTap>>,
    clock_secs: f64,
    pending_loads: [Option<Box<LoadRequest>>; DECK_COUNT],
    held: [VecDeque<Command>; DECK_COUNT],
    device_latency: f64,
    output_latency: f64,
}

/// Control half: cheap to share between UI, MIDI and background threads.
pub struct EngineHandle {
    commands: Mutex<rtrb::Producer<Command>>,
    snapshot: Mutex<triple_buffer::Output<EngineSnapshot>>,
    garbage: Mutex<rtrb::Consumer<Garbage>>,
}

pub fn create_engine(sample_rate: u32) -> (EngineHandle, RtEngine) {
    let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
    let (gb_tx, gb_rx) = rtrb::RingBuffer::new(GARBAGE_CAPACITY);
    let (snap_in, snap_out) = triple_buffer::triple_buffer(&EngineSnapshot::default());
    let sr = sample_rate as f32;
    let rt = RtEngine {
        sample_rate: sr,
        decks: [Deck::new(sr as f64), Deck::new(sr as f64)],
        mixer: Mixer::new(sr),
        commands: cmd_rx,
        garbage: gb_tx,
        snapshot: snap_in,
        routing: Routing::for_channel_count(2),
        quantize: true,
        master_deck: None,
        output_fade: Smoothed::new(0.0, sr, STARTUP_FADE_MS),
        frames_processed: 0,
        load: 0.0,
        tap: None,
        record: None,
        clock_secs: 0.0,
        pending_loads: [None, None],
        held: std::array::from_fn(|_| VecDeque::with_capacity(HELD_COMMANDS)),
        device_latency: 0.0,
        output_latency: 0.0,
    };
    let handle = EngineHandle {
        commands: Mutex::new(cmd_tx),
        snapshot: Mutex::new(snap_out),
        garbage: Mutex::new(gb_rx),
    };
    (handle, rt)
}

impl EngineHandle {
    /// Queues a command for the audio thread. Returns false if the queue is full.
    pub fn send(&self, cmd: Command) -> bool {
        self.commands.lock().push(cmd).is_ok()
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        *self.snapshot.lock().read()
    }

    /// Drops tracks released by the audio thread; call periodically from a non-realtime thread.
    pub fn collect_garbage(&self) -> usize {
        let mut gb = self.garbage.lock();
        let mut n = 0;
        while gb.pop().is_ok() {
            n += 1;
        }
        n
    }
}

impl RtEngine {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate as u32
    }

    pub fn routing(&self) -> Routing {
        self.routing
    }

    /// Reconfigures for a new device; must be called while no audio callback is running.
    pub fn reconfigure(&mut self, sample_rate: u32, routing: Routing) {
        let sr = sample_rate as f32;
        if (sr - self.sample_rate).abs() > 0.5 {
            self.sample_rate = sr;
            self.mixer = self.mixer.rebuilt_for_rate(sr);
            for d in &mut self.decks {
                d.set_output_rate(sr as f64);
            }
        }
        self.output_fade = Smoothed::new(0.0, sr, STARTUP_FADE_MS);
        self.routing = routing;
    }

    /// Delay between a callback and its first sample reaching the speakers, as the device reports it.
    pub fn set_device_latency(&mut self, secs: f64) {
        if secs.is_finite() {
            self.device_latency = secs.clamp(0.0, 1.0);
        }
    }

    /// Installs or removes the secondary-output tap; returns the previous tap so the caller drops it off the audio thread.
    pub fn set_secondary_tap(&mut self, tap: Option<SecondaryTap>) -> Option<SecondaryTap> {
        std::mem::replace(&mut self.tap, tap)
    }

    fn swap_record_tap(&mut self, slot: RecordTapSlot) {
        if let Some(tap) = slot.take()
            && let Some(old) = std::mem::replace(&mut self.record, tap)
        {
            self.release(Garbage::RecordTap(old));
        }
        self.release(Garbage::RecordSlot(slot));
    }

    fn release(&mut self, item: Garbage) {
        if let Err(rtrb::PushError::Full(item)) = self.garbage.push(item) {
            std::mem::forget(item);
        }
    }

    /// Applies a command, holding it back while its deck fades out for a pending load.
    fn dispatch(&mut self, cmd: Command) {
        if let Some(deck) = cmd.deck().filter(|&d| d < DECK_COUNT && self.pending_loads[d].is_some()) {
            if self.held[deck].len() < HELD_COMMANDS {
                self.held[deck].push_back(cmd);
                return;
            }
            self.finish_load(deck);
        }
        let before = self.was_playing_mask();
        let sync_target = match &cmd {
            Command::Play { deck } | Command::TogglePlay { deck } => Some(*deck),
            _ => None,
        };
        self.handle(cmd);
        if let Some(deck) = sync_target
            && deck < DECK_COUNT
            && !before[deck]
            && self.decks[deck].is_playing()
            && self.decks[deck].sync_enabled()
            && let Some(master) = self.master_deck.filter(|&m| m != deck)
        {
            self.align_phase(deck, master);
        }
    }

    fn load_now(&mut self, deck: usize, request: Box<LoadRequest>) {
        let d = &mut self.decks[deck];
        let phrases = d.replace_phrase_starts(Box::default());
        if let Some(old) = d.load(&request) {
            self.release(Garbage::Track(old));
        }
        self.release(Garbage::Phrases(phrases));
        self.release(Garbage::Request(request));
    }

    /// Swaps in the track waiting for its deck to fall silent, then replays the commands held meanwhile.
    fn finish_load(&mut self, deck: usize) {
        let Some(request) = self.pending_loads[deck].take() else { return };
        self.load_now(deck, request);
        while self.pending_loads[deck].is_none()
            && let Some(cmd) = self.held[deck].pop_front()
        {
            self.dispatch(cmd);
        }
    }

    fn handle(&mut self, cmd: Command) {
        let q = self.quantize;
        match cmd {
            Command::Load { deck, request } => {
                if deck >= DECK_COUNT {
                    self.release(Garbage::Request(request));
                } else if self.decks[deck].is_silent() {
                    self.load_now(deck, request);
                } else {
                    self.decks[deck].fade_out();
                    self.pending_loads[deck] = Some(request);
                }
            }
            Command::Eject { deck } => {
                if let Some(old) = self.decks.get_mut(deck).and_then(|d| d.eject()) {
                    self.release(Garbage::Track(old));
                    let phrases = self.decks[deck].replace_phrase_starts(Box::default());
                    self.release(Garbage::Phrases(phrases));
                }
            }
            Command::SetPhraseStarts { deck, starts } => {
                if let Some(d) = self.decks.get_mut(deck) {
                    let old = d.replace_phrase_starts(starts);
                    self.release(Garbage::Phrases(old));
                }
            }
            Command::Play { deck } => self.with_deck(deck, |d| d.play()),
            Command::Pause { deck } => self.with_deck(deck, |d| d.pause()),
            Command::TogglePlay { deck } => self.with_deck(deck, |d| d.toggle_play()),
            Command::CueDown { deck } => self.with_deck(deck, |d| d.cue_down(q)),
            Command::CueUp { deck } => self.with_deck(deck, |d| d.cue_up()),
            Command::SeekSecs { deck, secs } => self.with_deck(deck, |d| d.seek_secs(secs)),
            Command::SetTempo { deck, ratio } => self.with_deck(deck, |d| d.set_tempo(ratio)),
            Command::SetPitchFader { deck, position } => self.with_deck(deck, |d| d.set_pitch_fader(position)),
            Command::SetTempoRange { deck, range } => self.with_deck(deck, |d| d.set_tempo_range(range)),
            Command::SetKeyLock { deck, enabled } => self.with_deck(deck, |d| d.set_key_lock(enabled)),
            Command::SetVinylMode { deck, enabled } => self.with_deck(deck, |d| d.set_vinyl(enabled)),
            Command::JogTouch { deck, touched } => self.with_deck(deck, |d| d.jog_touch(touched)),
            Command::Jog { deck, ticks, mode } => {
                self.with_deck(deck, |d| d.jog(ticks as f64 / crate::JOG_TICKS_PER_REV, mode))
            }
            Command::HotCue { deck, slot, pressed } => self.with_deck(deck, |d| d.hot_cue(slot, pressed, q)),
            Command::HotCueDelete { deck, slot } => self.with_deck(deck, |d| d.delete_hot_cue(slot)),
            Command::HotCueColor { deck, slot, color } => self.with_deck(deck, |d| d.set_hot_cue_color(slot, color)),
            Command::LoopIn { deck } => self.with_deck(deck, |d| d.loop_in(q)),
            Command::LoopOut { deck } => self.with_deck(deck, |d| d.loop_out(q)),
            Command::ReloopExit { deck } => self.with_deck(deck, |d| d.reloop_exit()),
            Command::BeatLoop { deck, beats } => self.with_deck(deck, |d| d.beat_loop(beats, q)),
            Command::LoopScale { deck, factor } => self.with_deck(deck, |d| d.loop_scale(factor)),
            Command::BeatJump { deck, beats } => self.with_deck(deck, |d| d.beat_jump(beats)),
            Command::SetBeatgrid { deck, grid } => self.with_deck(deck, |d| d.set_grid(grid)),
            Command::Sync { deck, enabled } => self.set_sync(deck, enabled),
            Command::SetMasterDeck { deck } => {
                if deck < DECK_COUNT {
                    self.master_deck = Some(deck);
                    self.decks[deck].set_sync(false);
                }
            }
            Command::Mixer { channel, param } => self.mixer.channel(channel, param),
            Command::Crossfader(v) => self.mixer.set_crossfader(v),
            Command::CrossfaderCurve(c) => self.mixer.set_curve(c),
            Command::EqCurve(c) => self.mixer.set_eq_curve(c),
            Command::MasterLevel(v) => self.mixer.set_level(v),
            Command::SetRecordTap(slot) => self.swap_record_tap(slot),
            Command::HeadphoneMix(v) => self.mixer.set_headphone_mix(v),
            Command::HeadphoneLevel(v) => self.mixer.set_headphone_level(v),
            Command::Quantize(on) => self.quantize = on,
            Command::HardwarePhones(on) => self.mixer.hardware_phones = on,
            Command::BeatFxKind(kind) => self.mixer.set_beat_fx_kind(kind),
            Command::BeatFxTarget(target) => self.mixer.set_beat_fx_target(target),
            Command::BeatFxBeats(beats) => {
                if let Some(beats) = snap_beat_fx_beats_for(self.mixer.beat_fx_kind(), beats) {
                    self.mixer.set_beat_fx_beats(beats);
                }
            }
            Command::BeatFxParam { index, value } => self.mixer.set_beat_fx_param(index, value),
            Command::BeatFxTrigger(trigger) => self.mixer.trigger_beat_fx(trigger),
            Command::BeatFxPercent(percent) => {
                if let Some(percent) = clamp_beat_fx_percent(percent) {
                    self.mixer.set_beat_fx_percent(percent);
                }
            }
            Command::BeatFxLevel(v) => self.mixer.set_beat_fx_level(v),
            Command::BeatFxBands(bands) => self.mixer.set_beat_fx_bands(bands),
            Command::BeatFxEnabled(on) => {
                let position = self.fx_sync_deck().and_then(|d| self.decks[d].beat_position());
                self.mixer.set_beat_fx_enabled(on, position);
            }
            Command::ColorFx(kind) => self.mixer.set_color_fx(kind),
            Command::ColorFxParam { index, value } => self.mixer.set_color_fx_param(index, value),
            Command::PadFx { deck, voice } => {
                let position = self.decks.get(deck).and_then(Deck::beat_position);
                self.mixer.set_pad_voice(deck, voice, position);
            }
            Command::PadFxRelease { deck, kind, beats } => self.mixer.pad_release(deck, kind, beats),
            Command::ReleaseFx => self.mixer.fire_release(),
            Command::ReleaseFxConfig { kind, beats } => self.mixer.set_release_config(kind, beats),
            Command::SmartCfx(on) => self.mixer.set_smart_cfx(on),
            Command::SmartCfxPreset(preset) => self.mixer.set_smart_cfx_preset(preset),
        }
    }

    fn fx_sync_deck(&self) -> Option<usize> {
        match self.mixer.beat_fx_target() {
            FxTarget::A => Some(0),
            FxTarget::B => Some(1),
            FxTarget::Master => self.master_deck,
        }
    }

    fn update_fx_tempo(&mut self) {
        let master_bpm = self.master_deck.and_then(|m| self.decks[m].effective_bpm());
        let deck_bpm = |d: usize| self.decks[d].effective_bpm().or(master_bpm).unwrap_or(DEFAULT_FX_BPM);
        let channels = [deck_bpm(0), deck_bpm(1)];
        let fx_bpm = match self.mixer.beat_fx_target() {
            FxTarget::A => channels[0],
            FxTarget::B => channels[1],
            FxTarget::Master => master_bpm.unwrap_or(DEFAULT_FX_BPM),
        };
        self.mixer.set_tempo_context(fx_bpm, channels);
        self.mixer.set_channel_beats([self.decks[0].beat_position(), self.decks[1].beat_position()]);
    }

    /// Feeds the Beat FX the target deck's bar lines, places Ascend and Backwash on them and renders the Backwash swell.
    fn update_fx_transport(&mut self, frames: usize) {
        let deck = self.fx_sync_deck().filter(|&d| self.decks[d].grid().is_some());
        let Some(deck) = deck else {
            self.mixer.set_fx_transport(None);
            self.cancel_deck_bound_effects();
            return;
        };
        let d = &self.decks[deck];
        let Some(grid) = d.grid() else { return };
        let (now, rate, out_rate) = (d.audible_secs(), d.play_rate().max(1e-3), d.out_rate());
        let to_frames = |secs: f64| secs / rate * out_rate;
        let map = BarMap::new(grid, d.phrase_starts());
        let next_bar = map.next_bar_line(now);
        self.mixer.set_fx_transport(Some(Transport { frames_per_bar: to_frames(map.bar_secs()), frames_to_downbeat: to_frames(next_bar - now) }));

        let playing = d.is_playing();
        let fx = self.mixer.beat_fx();
        if let Some(request) = fx.ascend_request() {
            let landing = map.ascend_landing(now, request.length_bars, request.land_on_phrase);
            fx.arm_ascend(AscendLanding { frames: to_frames(landing.secs - now), secs: landing.secs, bars: landing.bars, landing_bar: landing.bar_number });
        }
        if let (true, Some(secs)) = (playing, fx.ascend_landing_secs()) {
            fx.sync_ascend_remaining(to_frames(secs - now));
        }
        if fx.backwash_request().is_some() {
            match d.track().filter(|_| playing) {
                Some(track) => {
                    let mut landing = next_bar;
                    if landing - now < BACKWASH_MIN_LEAD_BEATS * grid.beat_secs() {
                        landing = map.next_bar_line(landing + 1e-6);
                    }
                    fx.backwash_arm(BackwashArm {
                        frames_to_landing: to_frames(landing - now),
                        landing_secs: landing,
                        source_start: landing * track.sample_rate as f64,
                        source_step: rate * track.sample_rate as f64 / out_rate,
                    });
                }
                None => fx.backwash_cancel(),
            }
        }
        if let (true, Some(secs)) = (playing, fx.backwash_landing_secs()) {
            fx.sync_backwash_remaining(to_frames(secs - now));
        }
        if fx.backwash_rendering()
            && let Some(track) = d.track()
        {
            let urgent = fx.backwash_landing_secs().is_some_and(|secs| to_frames(secs - now) < out_rate);
            fx.backwash_pump(track, frames * if urgent { BACKWASH_URGENT_SPEED } else { BACKWASH_RENDER_SPEED });
        }
    }

    fn cancel_deck_bound_effects(&mut self) {
        let fx = self.mixer.beat_fx();
        let _ = fx.backwash_request();
        fx.backwash_cancel();
    }

    fn with_deck(&mut self, deck: usize, f: impl FnOnce(&mut Deck)) {
        if let Some(d) = self.decks.get_mut(deck) {
            f(d);
        }
    }

    fn other(deck: usize) -> usize {
        (deck + 1) % DECK_COUNT
    }

    fn set_sync(&mut self, deck: usize, enabled: bool) {
        if deck >= DECK_COUNT {
            return;
        }
        if !enabled {
            self.decks[deck].set_sync(false);
            return;
        }
        let master = self.master_deck.filter(|&m| m != deck).unwrap_or(Self::other(deck));
        if self.decks[master].grid().is_none() || self.decks[deck].grid().is_none() {
            return;
        }
        if self.master_deck.is_none() || self.master_deck == Some(deck) {
            self.master_deck = Some(master);
        }
        self.decks[deck].set_sync(true);
        self.align_phase(deck, master);
    }

    fn target_ratio(&self, deck: usize, master: usize) -> Option<f64> {
        let master_bpm = self.decks[master].effective_bpm()?;
        let own = self.decks[deck].grid()?.bpm;
        let mut target = master_bpm;
        while target / own > 1.5 {
            target /= 2.0;
        }
        while own / target > 1.5 {
            target *= 2.0;
        }
        Some(target / own)
    }

    /// Beat-phase error of `deck` relative to `master`, in beats of the synced deck, in [-0.5, 0.5).
    fn phase_error(&self, deck: usize, master: usize) -> Option<f64> {
        let mp = self.decks[master].beat_phase()?;
        let dp = self.decks[deck].beat_phase()?;
        let master_bpm = self.decks[master].effective_bpm()?;
        let deck_bpm = self.decks[deck].grid()?.bpm * self.target_ratio(deck, master)?;
        let ratio = (deck_bpm / master_bpm).round().max(1.0);
        let mut err = (mp * ratio).rem_euclid(1.0) - dp;
        err -= err.round();
        Some(err)
    }

    fn align_phase(&mut self, deck: usize, master: usize) {
        let Some(ratio) = self.target_ratio(deck, master) else { return };
        self.decks[deck].set_sync_state(ratio, 0.0);
        if !self.decks[master].is_playing() {
            return;
        }
        if let (Some(err), Some(grid)) = (self.phase_error(deck, master), self.decks[deck].grid()) {
            let target = self.decks[deck].audible_secs() + err * grid.beat_secs();
            self.decks[deck].seek_secs(target);
        }
    }

    fn update_master_and_sync(&mut self) {
        let playing = self.was_playing_mask();
        let master_valid = self.master_deck.is_some_and(|m| playing[m] && !self.decks[m].sync_enabled());
        if !master_valid {
            let candidate = (0..DECK_COUNT).find(|&i| playing[i] && !self.decks[i].sync_enabled());
            if candidate.is_some() {
                self.master_deck = candidate;
            }
        }
        let Some(master) = self.master_deck else { return };
        for (deck, &deck_playing) in playing.iter().enumerate() {
            if deck == master || !self.decks[deck].sync_enabled() {
                continue;
            }
            let Some(ratio) = self.target_ratio(deck, master) else { continue };
            let nudge = match (self.decks[master].is_playing() && deck_playing, self.phase_error(deck, master)) {
                (true, Some(err)) => (err / (SYNC_CORRECTION_SECS * self.decks[deck].effective_bpm().unwrap_or(120.0) / 60.0))
                    .clamp(-SYNC_MAX_NUDGE, SYNC_MAX_NUDGE),
                _ => 0.0,
            };
            self.decks[deck].set_sync_state(ratio, nudge);
        }
    }

    fn was_playing_mask(&self) -> [bool; DECK_COUNT] {
        [self.decks[0].is_playing(), self.decks[1].is_playing()]
    }

    /// Fills an interleaved output buffer of `channels` channels.
    pub fn process(&mut self, output: &mut [f32], channels: usize) {
        let started = Instant::now();
        let channels = channels.max(1);
        let total = output.len() / channels;

        for _ in 0..MAX_COMMANDS_PER_BLOCK {
            let Ok(cmd) = self.commands.pop() else { break };
            self.dispatch(cmd);
        }
        self.update_master_and_sync();
        self.update_fx_tempo();
        self.update_fx_transport(total);

        output.fill(0.0);
        if let Some(rec) = &mut self.record {
            rec.begin_block(self.clock_secs, self.sample_rate as u32);
        }
        let mut offset = 0;
        while offset < total {
            let n = (total - offset).min(MAX_BLOCK);
            self.render_block(n);
            self.write_output(&mut output[offset * channels..(offset + n) * channels], channels, n);
            offset += n;
        }
        for deck in 0..DECK_COUNT {
            if self.pending_loads[deck].is_some() && self.decks[deck].is_silent() {
                self.finish_load(deck);
            }
        }
        self.frames_processed += total as u64;
        self.clock_secs += total as f64 / self.sample_rate as f64;
        self.output_latency = self.device_latency + total as f64 / self.sample_rate as f64;
        if let Some(tap) = &mut self.tap {
            tap.end_block(total, self.sample_rate as u32);
        }

        let budget = total as f32 / self.sample_rate;
        let used = started.elapsed().as_secs_f32();
        let ratio = if budget > 0.0 { used / budget } else { 0.0 };
        self.load = (self.load * 0.95).max(ratio);
        self.publish();
    }

    fn render_block(&mut self, n: usize) {
        for d in &mut self.decks {
            d.render(n);
        }
        let [d0, d1] = &mut self.decks;
        self.mixer.process(
            [(&mut d0.buf_l[..], &mut d0.buf_r[..]), (&mut d1.buf_l[..], &mut d1.buf_r[..])],
            n,
        );
    }

    fn write_output(&mut self, out: &mut [f32], channels: usize, n: usize) {
        let m = &self.mixer;
        let r = self.routing;
        let tap_source = self.tap.as_ref().map(SecondaryTap::source);
        if let Some(rec) = &mut self.record {
            for i in 0..n {
                rec.push([m.rec_l[i], m.rec_r[i]]);
            }
        }
        for i in 0..n {
            let g = self.output_fade.next();
            if let (Some(tap), Some(source)) = (&mut self.tap, tap_source) {
                tap.push(match source {
                    SecondarySource::Master => [m.master_l[i] * g, m.master_r[i] * g],
                    SecondarySource::Cue => [m.phones_l[i] * g, m.phones_r[i] * g],
                });
            }
            let frame = &mut out[i * channels..(i + 1) * channels];
            match r.mode {
                RoutingMode::MasterAndPhones => {
                    put(frame, r.master[0], m.master_l[i] * g);
                    put(frame, r.master[1], m.master_r[i] * g);
                    put(frame, r.phones[0], m.phones_l[i] * g);
                    put(frame, r.phones[1], m.phones_r[i] * g);
                }
                RoutingMode::MasterOnly => {
                    put(frame, r.master[0], m.master_l[i] * g);
                    put(frame, r.master[1], m.master_r[i] * g);
                }
                RoutingMode::SplitMono => {
                    put(frame, r.master[0], 0.5 * (m.master_l[i] + m.master_r[i]) * g);
                    put(frame, r.master[1], 0.5 * (m.phones_l[i] + m.phones_r[i]) * g);
                }
            }
        }
        self.output_fade.set(1.0);
    }

    fn publish(&mut self) {
        let master = self.master_deck;
        let snap = self.snapshot.input_buffer_mut();
        for (i, d) in self.decks.iter().enumerate() {
            snap.decks[i] = d.snapshot(master == Some(i));
            snap.channels[i] = self.mixer.channel_snapshot(i);
        }
        snap.master = self.mixer.master_snapshot();
        snap.quantize = self.quantize;
        snap.master_deck = master.map(|m| m as u8);
        snap.sample_rate = self.sample_rate as u32;
        snap.frames_processed = self.frames_processed;
        snap.clock_secs = self.clock_secs;
        snap.output_latency_secs = self.output_latency;
        snap.callback_load = self.load;
        snap.cue_available = self.routing.cue_available();
        snap.beat_fx = self.mixer.beat_fx_snapshot();
        snap.color_fx = self.mixer.color_fx_kind();
        snap.color_fx_params = self.mixer.color_fx_params();
        snap.pad_fx = [self.mixer.pad_fx_snapshot(0), self.mixer.pad_fx_snapshot(1)];
        snap.release_fx = self.mixer.release_fx_snapshot();
        snap.smart_cfx = self.mixer.smart_cfx_snapshot();
        self.snapshot.publish();
    }
}

#[inline]
fn put(frame: &mut [f32], channel: usize, value: f32) {
    if let Some(s) = frame.get_mut(channel) {
        *s = value;
    }
}
