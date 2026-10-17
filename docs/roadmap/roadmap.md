# Roadmap

## Done (MVP foundation)
- M1 Two-deck audio foundation — native Rust engine, symphonia decode, cubic varispeed, shared mixer, cpal output.
- M3 Routing — single synchronized multichannel stream; master 1-2, headphones 3-4; PFL independent of fader/crossfader; stereo fallback (master-only / split-mono); hardware-phones mode for the FLX4.
- M5 Performance features — BPM/beatgrid analysis, tempo ranges, tempo+phase sync with continuous correction, CDJ cue, 8 hot cues (persisted), manual/beat loops, beat jump, quantize, bipolar filter, 3-band isolator, key lock, jog scratch/bend/search.
- M2 FLX4 mapping — implemented from the official MIDI list; simulated end-to-end; **physical verification pending**.

## Next (needs the FLX4 plugged in)
1. Hardware verification pass (see `docs/hardware-verification.md`): port names, 4-ch channel order, jog ticks/rev, 14-bit MSB/LSB ordering, keepalive SysEx effect, headphone MIX hardware behaviour, LED polarity, pad-mode LED exclusivity.
2. Tune jog scratch feel (velocity filter, touch ramp) with real hardware.
3. Measure real output latency (buffer 128/256) and xruns on the FLX4.

## Performance-mode polish
- Slip mode; reverse; censor.
- Memory cues (slots ≥ 8 in `cues` table), saved loops.
- Beat FX (echo, reverb, flanger) mapped to the FLX4 Beat FX section; Pad FX.
- Sampler (FLX4 SAMPLER pad mode).
- Dynamic beatgrids (tempo-varying tracks); downbeat (bar) detection.
- Musical key detection (chromagram + key profiles) with Camelot display.

## Phase 2 — Export mode
- Advanced library: smart playlists, tags, ratings, colours, history.
- Harmonic mixing tools (Camelot wheel, key compatibility in browser).
- Advanced beatgrid editor (drag/tap, per-section grids).
- rekordbox XML import (collection, cues, grids) and playlist export (M3U8 / XML).
- USB library export through documented/supported formats only.

## Cross-platform
- Windows: cpal WASAPI (ASIO via the `asio` feature + Steinberg SDK where licensing allows); midir WinMM.
- Linux: cpal ALSA/JACK/PipeWire; midir ALSA. FLX4 is class-compliant on both.
- CI matrix builds for macOS/Windows/Linux; packaging via `tauri build`.
