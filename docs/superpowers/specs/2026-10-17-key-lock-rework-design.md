# Key lock rework

## Goal

Key lock (master tempo) keeps a track's pitch while its tempo moves, and sounds as clean as normal playback: full treble, no warble, no clicks when the tempo fader crosses zero, when the jog wheel nudges, or when cues, loops and jumps fire. The jog wheel nudge also gets stronger, because today a full turn moves the track only about 36 ms.

## Findings that drive it

- The WSOLA stretcher aligns grains only to 8-sample steps. Overlapping grains then cancel above about 2 kHz: at +6 % a 4 kHz tone loses half its level and a 9 kHz tone loses about 36 dB. Sample-accurate alignment helps, but WSOLA still smears dense mixes.
- With key lock on and the fader at zero, the deck switches between varispeed and WSOLA whenever the tempo leaves or returns to exactly 1.0 (jog nudge, sync nudge, fader crossing zero). Each switch replays a stale overlap-add tail and rewinds about 25 ms.
- Signalsmith Stretch (MIT, header-only C++) kept 220 Hz–9 kHz tones exact at +6 % and kept a real track's treble and spectral flux equal to unity playback, at under 1 % CPU per deck.

## Decisions

| Question | Decision |
|---|---|
| Algorithm | Signalsmith Stretch, default preset, transpose 1 |
| Build | Vendor its headers under `crates/dsp/vendor/`, compile a small C shim with `cc`; no bindgen, so Windows and Linux CI need nothing new |
| When the stretcher runs | Whenever key lock is on and the jog top is not held, including at tempo 1.0. No switching at unity |
| Jumps (cue, hot cue, loop wrap, beat jump, seek, search) | Seek a second stretcher voice at the target and crossfade over 2 ms, so the target is heard immediately |
| Loops | Wrap when the *audible* position reaches loop out, split inside the block, crossfaded |
| Scratching | Jog-top touch in vinyl mode hands over to varispeed; handover in both directions crossfades over 2 ms |
| Latency | The stretcher reports which source position is audible; `audible_secs` (playhead, sync, loops, cues, FX transport) uses it |
| Output rate | The stretcher is rebuilt on device rate change (`reconfigure`, outside the audio callback) |
| Jog nudge | `BEND_PER_REV` 0.3 → 1.2: a full turn at a steady spin nudges about 0.14 s, with or without key lock |

Rubber Band (GPL) and SoundTouch (LGPL) were rejected on licence.

## Real-time rules

- No allocation in `render`: Signalsmith's `process`/`seek` only resize vectors within the capacity set at configure time; Rust-side buffers are sized at construction for the worst case (block 2048 × tempo 3.2, plus seek pre-roll).
- Both voices and scratch buffers belong to the deck; nothing is shared across threads.

## Testing

- Stretcher: tones at 220/1500/4000/9000 Hz within ±1 dB at tempo 0.84, 1.0 and 1.16, with no energy at the shifted frequency; reported audible position within 2 ms of a measured click, steady and right after a jump; no discontinuity on jumps or on per-block tempo changes; no Rust allocations while rendering.
- Engine: key lock at fader zero with jog nudges has no discontinuity; a key-locked 1-beat loop repeats a click at the loop length within 2 ms; a hot cue on a click plays the click within 5 ms; scratch handover in and out has no discontinuity; one jog revolution nudges 0.12–0.16 s with key lock on and off; existing tests stay green.
- By ear in the app: the user retests key lock with tempo moves, jog nudges and the master output.
