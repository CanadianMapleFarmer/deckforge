# deckforge architecture

```
                 ┌──────────────── apps/desktop (Tauri 2) ────────────────┐
  Svelte 5 UI ◀─ engine://state (60 Hz snapshot) ── state pump thread       │
      │  invoke(engine_command / load_track / …)                         │
      ▼                                                                    │
  commands.rs ──▶ EngineHandle ──(rtrb SPSC, lock-free)──▶ RtEngine        │
      ▲               ▲   ▲                                  │ (audio      │
      │               │   └─ ControllerService thread         │  callback)  │
      │               │        midir ⇄ DDJ-FLX4 (MIDI in/LEDs)│             │
      │          triple-buffered EngineSnapshot ◀──────────────┘             │
  AnalysisQueue (2 workers): decode → beatgrid + waveform → SQLite          │
  Library (rusqlite) — tracks, analysis, cues, playlists, settings          │
                 └──────────────────────────────────────────────────────────┘
```

## Crates
| Crate | Responsibility |
|---|---|
| `common` | Shared domain types: `TrackAudio`, `Beatgrid`, `HotCue`, tempo ranges, curves. |
| `dsp` | Original DSP: TPT state-variable filter, Linkwitz-Riley 3-band isolator, bipolar DJ filter, crossfader curves, soft limiter, peak meters, Catmull-Rom interpolation, and the key-lock stretcher: two Signalsmith Stretch voices (vendored, MIT) that crossfade on jumps and report the audible position. |
| `analysis` | Symphonia decoding to stereo f32; spectral-flux + kick-envelope BPM/beatgrid; 3-band waveform envelopes. |
| `engine` | Real-time two-deck engine (`RtEngine`), command queue, snapshots, mixer with PFL bus, `AudioManager` (cpal stream ownership, routing, reconnect, silent fallback). |
| `controller` | `ControllerMapping` trait, DDJ-FLX4 mapping (decode + LED feedback), `ControllerService` (discovery, hot-plug, diagnostics log, injection). |
| `library` | SQLite collection: import (lofty tags + artwork), analysis cache, cues, playlists, settings. |
| `cli` | Headless diagnostics: device list, BlackHole loopback routing test, simulated-FLX4 end-to-end test. |

## Real-time rules (audio callback)
- No locks except `try_lock` on the engine mutex (contention only during device reconfiguration → one silent block).
- No allocation/deallocation: replaced tracks and load requests are pushed to a garbage ring and dropped by the state pump.
- No I/O, no analysis. Tracks are fully decoded off-thread before `Command::Load`.
- Commands: bounded SPSC ring (`rtrb`, 4096); snapshots: `triple_buffer` (wait-free).
- Parameter changes are smoothed (one-pole, 15 ms) and position jumps are de-clicked with a 96-frame crossfade.

## Signal flow per deck
`track (native rate) → varispeed cubic read (rate = src/out × tempo × bend × sync-nudge)` or `Signalsmith stretch (key lock, whenever the jog top is not held)` → play/pause fade → **trim → isolator EQ → bipolar filter** → *(PFL tap → cue bus)* → channel fader → crossfader → master bus → master gain → soft limiter → master pair.
Headphones: `cue bus × mix + master × (1 − mix)` × level, or pure cue when `hardwarePhones` (the FLX4 blends cue/master and sets level in hardware).

## Routing
- ≥4 output channels: master → 1-2, headphones → 3-4 (configurable pairs), single synchronized stream.
- Stereo-only: master only (cue unavailable, UI warns) or split-mono (master L / cue R).

## Sync
Master deck = explicit choice or the first playing unsynced deck. A synced deck's tempo ratio tracks `master_bpm / own_grid_bpm` (half/double-time folded), and a bounded rate nudge (≤3 %) continuously corrects beat-phase error measured from both grids at the audible position (stretch latency compensated). Enabling sync on a playing deck also performs an immediate phase-aligned seek.

## State ownership
The engine is the single source of truth. UI and controller both send commands; both render from the same snapshot. The controller's soft takeover compares the physical tempo fader with the engine's fader ratio before taking control.
