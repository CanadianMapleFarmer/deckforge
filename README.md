<div align="center">

<img src="docs/assets/logo.svg" alt="deckforge logo" width="112" height="112">

# deckforge

**A fast, native two-deck DJ app built for the Pioneer DJ DDJ-FLX4.**
Real-time Rust audio engine · GPU waveforms · Tauri 2 + Svelte 5 interface

[![CI](https://github.com/CanadianMapleFarmer/deckforge/actions/workflows/ci.yml/badge.svg)](https://github.com/CanadianMapleFarmer/deckforge/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/CanadianMapleFarmer/deckforge?include_prereleases&sort=semver&color=4d6bff)](https://github.com/CanadianMapleFarmer/deckforge/releases)
[![Downloads](https://img.shields.io/github/downloads/CanadianMapleFarmer/deckforge/total?color=ff4aa8)](https://github.com/CanadianMapleFarmer/deckforge/releases)
[![License](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-1ee3ff)](#license)
<br>
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-0d0f17)](#download)
[![Rust](https://img.shields.io/badge/Rust-2024-ff9a3c?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-2-24c8db?logo=tauri&logoColor=white)](https://tauri.app)
[![Svelte](https://img.shields.io/badge/Svelte-5-ff3e00?logo=svelte&logoColor=white)](https://svelte.dev)

[Download](#download) · [Features](#features) · [Build from source](#build-from-source) · [Contributing](CONTRIBUTING.md)

<br>

<img src="docs/screenshots/layout-v2-1600.png" alt="deckforge with two decks loaded, phrase-coloured waveforms and the FX display" width="100%">

</div>

## Why deckforge

deckforge is an original DJ performance app for two-deck sets. Audio runs in a native real-time engine (no WebAudio, no garbage collector in the audio path), waveforms are drawn on the GPU at display refresh rate, and the DDJ-FLX4 is mapped end to end — pads, FX, jogs and LEDs.

Your music files are only ever read, never modified.

## Features

**Decks**
- Two decks with sync, key lock (master tempo, using [Signalsmith Stretch](https://github.com/Signalsmith-Audio/signalsmith-stretch)), quantize and pitch bend
- Hot cues, loops (in/out, auto 1/4–32 beats, reloop) and beat jump; a loop pressed just before the beat still catches it
- CDJ-style CUE behaviour and a jog wheel with scratch, bend and fast search

**Analysis**
- BPM and beatgrid, musical key (Camelot) and phrase detection (intro, break, drop, outro)
- Three-band colour waveforms with bar and phrase markers, rendered natively with `wgpu`
- Phrase countdowns, a "mixes with" key filter and a phase meter between decks

**Mixer and effects**
- Three-band EQ with two curves: a gentle DJM-style **EQ** curve (−26 dB) or a full-kill **Isolator**
- Colour FX: filter, space, dub echo, crush, noise — plus **Smart CFX** macros
- 17 beat-synced Beat FX (echo, ping pong, spiral, reverb with Classic / Tension voicing, flanger, phaser, roll, trans …)
- Pad FX on each deck and one-shot Release FX (echo out, backspin, vinyl brake)

**Library**
- SQLite collection with search and background analysis
- Playlists in nested folders, imported from music folders, M3U/M3U8 files or rekordbox XML exports, and refreshed in place on re-import
- Spotify playlist matching to your local files (Spotify audio is never captured)
- **Set history**: every set is logged automatically with the time each track took over, browsable by date, with a one-click copy of the `00:00 Artist – Title` tracklist and export to CSV, CUE or video-editor markers

**Recording**
- One-click recording of the mix to WAV (RF64 past 4 GB) or FLAC, 16 or 24-bit at 44.1 or 48 kHz, written on its own thread so the live output is never touched
- Taken before the master volume and limited like the master, so turning the room down never changes the file
- A tracklist, plus optional CSV, CUE and DaVinci Resolve / Premiere Pro markers, written next to each recording

**Hardware**
- Full Pioneer DJ **DDJ-FLX4** mapping with host-driven LED feedback, soft takeover and keepalive
- Headphone cue routing on the FLX4's 4-channel audio interface
- Complete [keyboard control](apps/desktop/KEYBOARD.md) when no controller is connected

## Download

Grab the latest build from [**Releases**](https://github.com/CanadianMapleFarmer/deckforge/releases).

| Platform | File |
|---|---|
| macOS (Apple Silicon) | `deckforge_<version>_aarch64.dmg` |
| macOS (Intel) | `deckforge_<version>_x64.dmg` |
| Windows 10/11 | `deckforge_<version>_x64-setup.exe` or `.msi` |
| Linux | `.AppImage`, `.deb` or `.rpm` |

> [!NOTE]
> Builds are not yet signed with a paid certificate.
> **macOS:** right-click the app → **Open** the first time, or run `xattr -cr /Applications/deckforge.app`.
> **Windows:** SmartScreen may warn — choose **More info → Run anyway**.

## Build from source

**Requirements:** stable Rust, Node 20+, pnpm 10.
On Linux, also install the Tauri and audio system libraries:

```bash
sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libasound2-dev
```

```bash
git clone https://github.com/CanadianMapleFarmer/deckforge.git
cd deckforge/apps/desktop
pnpm install
pnpm tauri dev      # development build with hot reload
pnpm tauri build    # release bundle in target/release/bundle
```

Data lives in the platform data directory (`~/Library/Application Support/deckforge` on macOS). Override it with `DECKFORGE_DATA_DIR=/some/dir`.

### Tests

```bash
cargo test --workspace --release
cargo clippy --workspace --all-targets
cd apps/desktop && pnpm check && pnpm test
```

The `deckforge-cli` crate has device and simulation checks (`devices`, `flx4-sim`, `bench`); `loopback` uses [BlackHole 16ch](https://existential.audio/blackhole/) on macOS.

## Project layout

```
apps/desktop/          Tauri 2 app — src-tauri (Rust backend) and src (Svelte 5 UI)
crates/common          shared domain types
crates/dsp             filters, EQ, effects, limiter, key-lock time-stretch
crates/analysis        decoding, BPM / beatgrid, key, phrases, waveform
crates/engine          real-time two-deck engine, mixer, audio output
crates/controller      MIDI service, DDJ-FLX4 mapping and LED feedback
crates/gfx             wgpu waveform renderer
crates/library         SQLite collection and set history
crates/recorder        WAV/RF64 and FLAC set recording
crates/spotify         Spotify playlist import (PKCE, user-supplied client ID)
crates/cli             diagnostics and simulation tools
docs/                  architecture, design notes, FLX4 MIDI reference, roadmap
```

## Status

deckforge is an early **0.x** release. It is used live with a DDJ-FLX4 on macOS; Windows and Linux builds are compiled and tested in CI but have seen less real-world use. See the [hardware checklist](docs/hardware-verification.md) and [roadmap](docs/roadmap/roadmap.md). Bug reports and feedback are very welcome.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

Unless you state otherwise, any contribution you submit is dual-licensed as above, without additional terms.

## Disclaimer

deckforge is an independent project and is not affiliated with, endorsed by or sponsored by AlphaTheta Corporation or Pioneer DJ. Pioneer DJ, DDJ-FLX4, DJM and rekordbox are trademarks of their respective owners and are used only to describe compatibility.
The FLX4 mapping follows Pioneer DJ's published MIDI message list and hands-on testing; no rekordbox code, assets or data are included.
