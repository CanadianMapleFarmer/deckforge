# Key lock rework Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the WSOLA key lock with Signalsmith Stretch, run it continuously while key lock is on, make jumps/loops/scratch handovers click-free and latency-correct, and strengthen the jog nudge.

**Architecture:** `crates/dsp` vendors the Signalsmith headers and compiles a C shim; `dsp::signalsmith::Voice` is the safe FFI wrapper and `dsp::stretch::Stretcher` owns two voices, crossfades between them on jumps and reports the audible source position. `engine::Deck` drives the stretcher whenever key lock is on and the jog top is not held.

**Tech Stack:** Rust 2024, `cc` build script, Signalsmith Stretch (MIT, C++14 header-only).

**Spec:** `docs/superpowers/specs/2026-10-17-key-lock-rework-design.md`

## Global Constraints

- No allocation on the audio thread; all buffers sized at construction or in `reconfigure`.
- Zero code comments by default; one line only for a non-obvious constraint.
- Never run `cargo fmt`.
- Music files are only read.
- MIT/Apache-compatible dependencies only; vendored licences kept beside the headers.
- Commit trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

- Tempo exactly 1.0 with key lock on: still stretched, no switching, no clicks.
- Very low tempo (Wide range near −100 %) and very high (bend + nudge on +100 %): no panic, no out-of-bounds input reads, buffers big enough.
- Jumps near the track start/end: pre-roll reads before frame 0 or past the end return silence, not garbage.
- Loop shorter than one render block, and a loop wrap landing in the middle of a block.
- Output-rate change (44.1 → 48 → 96 kHz) rebuilds the stretcher and keeps the audible position right.

---

### Task 1: Vendored Signalsmith Stretch and safe FFI voice

**Files:**
- Create: `crates/dsp/vendor/signalsmith-stretch/signalsmith-stretch.h`, `crates/dsp/vendor/signalsmith-stretch/LICENSE.txt`, `crates/dsp/vendor/signalsmith-linear/{fft.h,stft.h,LICENSE.txt}`, `crates/dsp/vendor/README.md`
- Create: `crates/dsp/src/signalsmith.cpp`, `crates/dsp/build.rs`, `crates/dsp/src/signalsmith.rs`
- Modify: `crates/dsp/Cargo.toml`, `crates/dsp/src/lib.rs`, root `Cargo.toml` (`cc = "1"`)

**Interfaces — Produces:**
```rust
pub struct Voice;              // Send, owns one SignalsmithStretch<float>, stereo
impl Voice {
    pub fn new(sample_rate: f64) -> Self;
    pub fn input_latency(&self) -> usize;
    pub fn output_latency(&self) -> usize;
    pub fn preroll_frames(&self) -> usize;          // block + interval
    pub fn reset(&mut self);
    pub fn seek(&mut self, l: &[f32], r: &[f32], rate: f64);
    pub fn process(&mut self, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]);
}
```

- [ ] Test: a 9 kHz + 220 Hz stereo tone pushed through `Voice` at 1.06× keeps both amplitudes within ±1 dB. Run `cargo test -p deckforge-dsp signalsmith` → fails to compile (no module).
- [ ] Vendor headers, shim, build script, wrapper. Test passes.
- [ ] Commit `feat(dsp): vendor Signalsmith Stretch behind a safe voice wrapper`.

### Task 2: Stretcher on two Signalsmith voices

**Files:** Replace `crates/dsp/src/stretch.rs` (tests inside).

**Interfaces — Produces:**
```rust
pub struct Stretcher;
impl Stretcher {
    pub fn new(out_rate: f64) -> Self;
    pub fn jump(&mut self, position: f64);          // source frames; heard immediately, crossfaded
    pub fn render(&mut self, track: &TrackAudio, step: f64, tempo: f64, out_l: &mut [f32], out_r: &mut [f32]);
    pub fn audible_position(&self) -> f64;          // source frames audible at the next output frame
    pub fn fade_out_into(&mut self, track: &TrackAudio, step: f64, tempo: f64, out_l: &mut [f32], out_r: &mut [f32]); // adds a falling 2 ms tail, then idles
    pub fn crossfade_frames() -> usize;
}
```
`step` = source frames per output frame at native pitch; `tempo` = playback rate. A jump while idle fades in instead of crossfading.

- [ ] Tests (fail first: old WSOLA loses 9 kHz / API differs): tone amplitudes at 0.84/1.0/1.16; audible position vs click within 2 ms steady and after a jump; second-difference continuity on a sine across a jump and across per-block tempo modulation; zero Rust allocations in `render` after warm-up; reads before 0 / past end are silent.
- [ ] Implement. Tests pass.
- [ ] Commit `feat(dsp): rebuild the key-lock stretcher on Signalsmith Stretch`.

### Task 3: Deck runs the stretcher continuously

**Files:** `crates/engine/src/deck.rs`, `crates/engine/tests/engine.rs`

**Interfaces — Consumes:** Task 2 `Stretcher`.

- `use_stretch = key_lock && !jog_touched` (no unity check).
- Varispeed → stretch: `stretcher.jump(position)` (fades in) while a varispeed declick tail fades out.
- Stretch → varispeed: `position = stretcher.audible_position()`; render varispeed, then `stretcher.fade_out_into` adds the tail.
- Loop wrap while stretching: frames until the audible position reaches loop out = `(b - audible) / (step·tempo)`; render up to it, `jump(a + overshoot)`, render the rest.
- `audible_secs` uses `stretcher.audible_position()`; `set_output_rate` rebuilds the stretcher.

- [ ] Tests (fail first): key lock at fader 0 with jog nudges has no discontinuity on a sine; key-locked 1-beat loop on a click track repeats at the loop length within 2 ms; hot cue on a click plays it within 5 ms; scratch touch/release under key lock has no discontinuity.
- [ ] Implement. `cargo test -p deckforge-engine` green.
- [ ] Commit `fix(engine): run key lock continuously with latency-correct jumps and loops`.

### Task 4: Stronger jog nudge

**Files:** `crates/engine/src/deck.rs`, `crates/engine/tests/engine.rs`

- [ ] Test (fails first): a steady 1 rev/s spin for 1 s nudges the track 0.12–0.16 s ahead, key lock off and on.
- [ ] `BEND_PER_REV` 0.3 → 1.2. Update the existing bend test bounds if they pin the old value.
- [ ] Commit `fix(engine): make the jog nudge four times stronger`.

### Task 5: Docs and full verification

- [ ] README (Key lock line), CHANGELOG Unreleased (Changed/Fixed), architecture overview line for the stretcher.
- [ ] `cargo test --workspace --release`, `cargo clippy --workspace --all-targets --release -- -D warnings`, `pnpm check && pnpm test`, CLI `bench` with two real tracks under budget.
- [ ] Commit `docs: describe the Signalsmith key lock`.
