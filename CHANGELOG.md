# Changelog

All notable changes to deckforge are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.0] - 2026-10-18

### Changed
- Key lock now uses Signalsmith Stretch. The treble stays intact and the sound no longer warbles when the tempo moves.
- The jog wheel nudge is about four times stronger: a full turn moves the track about 0.13 s.

### Fixed
- Key lock no longer clicks or repeats audio when the jog wheel nudges a deck at zero tempo, or when you start or stop scratching.
- Key-locked loops keep their exact length, and cues and hot cues are heard the moment they fire.
- Spamming CUE, firing hot cues, scratching or nudging one deck can no longer click the master output, and loading a track onto a playing deck fades it out instead of cutting it off.
- The waveform now follows the deck's real speed while nudging and scratching, shows what you hear rather than running ahead by the output latency, and stays under the cursor when dragged at any tempo.
- Pulling the tempo to −100 % with key lock on goes silent instead of droning.

## [0.2.0] - 2026-10-16

### Added
- Playlist folders in the sidebar, with collapse state remembered between launches.
- Import folders as playlists: subfolders become playlist folders, and a `.m3u8` beside the tracks sets the playlist's name and order.
- Import M3U/M3U8 files and rekordbox collection XML exports, including pooled `Tracks/` + `Playlists/` exports such as Music Ripper's.
- Re-importing the same folder or file updates its playlists in place; tracks that can't be found are listed after the import.

### Changed
- The FLX4 browse highlight is bolder, so the selected row stays visible on a dim screen.

## [0.1.0] - 2026-10-15

First public release.

### Added
- Native two-deck engine with sync, key lock, quantize, hot cues, loops and beat jump.
- Track analysis: BPM, beatgrid, key, phrases and three-band waveforms rendered with `wgpu`.
- Mixer with DJM-style EQ and Isolator curves, Colour FX, Smart CFX, 17 Beat FX, Pad FX and Release FX.
- SQLite library with folder import, playlists and Spotify playlist matching.
- Full Pioneer DJ DDJ-FLX4 mapping with LED feedback.
- macOS, Windows and Linux builds.

[Unreleased]: https://github.com/CanadianMapleFarmer/deckforge/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/CanadianMapleFarmer/deckforge/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/CanadianMapleFarmer/deckforge/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/CanadianMapleFarmer/deckforge/releases/tag/v0.1.0
