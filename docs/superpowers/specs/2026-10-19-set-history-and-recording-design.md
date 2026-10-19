# Set history and set recording

## Intent

Two features that work alone and together.

1. **Set history.** deckforge keeps a log of every set: which tracks reached the audience, in what order, and when each one took over. The user wants to look back at what they played at a given gig or session, and to paste a tracklist ("00:00 Artist – Title") under a YouTube video or send it to someone.
2. **Set recording.** A record button captures the mix in high quality inside deckforge. Today the user routes rekordbox's output through BlackHole into OBS, records a 48 kHz WAV, checks the mix, then either lines it up with camera footage in DaVinci Resolve or masters it and uploads to SoundCloud. The recorder replaces that chain, and the set's history enriches the recording with a tracklist and editor markers.

rekordbox has a history; this should match it and do better, chiefly by recording when each transition actually happened.

## Goals

- Every set is logged automatically, whether or not it is recorded. History never depends on recording.
- A track's time in a set is when it **took over**: the previous track is fully faded out while the new one is audible.
- One-click copy of a plain tracklist with times from 00:00.
- A set can be browsed later by date: tracks, order, times, total length.
- Recording writes a lossless file of the mix, with a tracklist and optional marker files beside it, timed from the start of the recording.
- Recording never causes an audio dropout and never risks the live master output.

## Non-goals

- Recording a microphone, the headphone cue, or individual decks (stems).
- Video, streaming, or uploading anywhere.
- Lossy recording (MP3/AAC); the user masters afterwards.
- Editing audio inside deckforge.

## Anti-goals

- A tracklist that lists tracks only previewed in headphones, or that adds or reorders entries because a track was looped.
- A recording whose level follows the master volume knob, so turning the room down makes the file quieter.
- A recording that clicks, drops samples or stops on a long set, or a disk problem that touches the live output.
- Markers tied to one video editor.

## What counts as a track in the set (decided)

- **Audible:** a deck is audible when it is playing and its channel fader and crossfader let any of it reach the master. Trim and EQ are ignored for this; a bass swap done with EQ counts when the old track's fader or crossfader goes down.
- **Took over (decided, the looser rule):** the new track's entry time is the moment every other deck has fully faded out (fader or crossfader down, or paused) while it is audible, whatever its own level. A DJ who never pushes the fader to the top, or leaves the crossfader in the middle, still gets every track. Bars and loops play no part, so looping a track for effect never adds or moves an entry.
- **Blend window:** the history also keeps when the incoming track first became audible on the master, shown as extra detail ("mixed in from 41:20, fully in at 42:05"). The copy-paste tracklist uses only the take-over time.
- **Not counted:** tracks never audible on the master (headphone preview, loaded and ejected).
- The same track played twice is two entries. A track that was audible but never took over (a short tease, a blend abandoned) is kept in the set marked as "teased", excluded from the copy-paste tracklist by default.
- **Crossfader off:** when the crossfader is switched off (a separate mixer feature), the take-over detection ignores it entirely.

### Entry rules (my calls)

- An entry belongs to one load of a track on a deck. Jumping, cueing or looping within that load never creates a new entry.
- A tease that later takes over without being reloaded is one entry: its blend start is the first time it became audible, and it stops being "teased".
- A flip-back (B takes over, then A, still loaded, takes over again) adds a new entry for A, because it was played again.
- Brief returns of the outgoing track during a blend (a crossfader wobble) are ignored: a deck must stay audible for at least one beat (or 0.5 s without a grid) to count as audible again.

## What counts as a set (my call)

- A set starts when the first track becomes audible on the master after the app opens or after the previous set ended.
- It ends when the app closes, or after 30 minutes with nothing audible on the master. Starting a recording during a set doesn't split it, and a set never ends while a recording is running.
- **Recordings and sets (my calls):** a recording belongs to exactly one set; a set can have several recordings (stop and record again). Pressing record before anything is audible starts a set at once. The audio file is named by date and time only (`YYYY-MM-DD HH.MM.wav`), so it never needs renaming; the set name goes into the files beside it.
- Sets can be renamed, deleted, and have entries removed afterwards. The default name is the date and start time.

## Recording (decided)

- **What is recorded:** the master mix after channel trim, EQ, filters, channel faders, crossfader and every effect, but **before the master volume knob**. It then goes through the same soft limiter as the master, so overlapping tracks cannot clip the file. Master volume never changes the recording.
- **Format:** WAV by default, with FLAC available. 16- or 24-bit (default 24), 44.1 or 48 kHz (default 48). If the engine runs at a different rate, the recording is resampled to the chosen one.
- **Where:** a folder chosen in settings, defaulting to `deckforge Recordings` in the user's Music folder on each OS.
- **Long sets:** WAV is limited to 4 GB (about 3 h 50 min at 48 kHz/24-bit). The recorder must keep going past that, by writing RF64 or by continuing in a second file; the builder picks, but the recording must not stop or lose audio.
- **Control:** a record button with elapsed time and file size in the top bar. Stopping, closing the app, or the device disappearing closes the file cleanly.
- **Real time:** audio is handed from the audio thread to a writer thread through a lock-free ring buffer. The audio thread never blocks, allocates or touches the disk. If the writer falls behind (slow disk) the recording drops samples and the app warns; the live output is never affected.
- **Disk:** warn before starting when the free space is under an hour of recording, and stop cleanly with a warning before the disk is full.

## Files beside a recording (decided)

Each recording gets the set's entries, timed from the first recorded sample (00:00), written when recording stops:

- **Tracklist** (`.txt`), always: one line per take-over, `00:00 Artist – Title`.
- **Marker files**, each switched on or off in settings so no editor is privileged:
  - CSV: time, artist, title, blend start, track length — readable by any tool.
  - CUE sheet: the standard tracklist for an audio file.
  - Timeline markers in a format DaVinci Resolve and Premiere Pro can both import. The builder picks the format that both accept.

Times are relative to the recording (H:MM:SS past an hour). The track already playing when recording starts is listed at 00:00 (its blend start clamped to 00:00); tracks fully replaced before recording started are left out. A blend still in progress when recording stops is listed if its track took over before the end.

**Edits afterwards (my call):** the files beside a recording are a snapshot taken when recording stops. Editing the set later (rename, remove an entry) doesn't touch them; the History screen has "Rewrite recording files" to refresh them. Deleting a set never deletes a recording from disk.

## History screen (my call)

- A **History** entry in the library sidebar lists sets newest first: name, date, length, track count, and whether there is a recording.
- Opening a set shows its tracks in order: take-over time, artist, title, key, BPM, blend window, and teased tracks greyed out. Clicking a track selects it in the collection; the set can be turned into a playlist.
- Buttons: copy tracklist, export (same formats as recording markers), open the recording's folder when there is one.
- The set currently being logged shows live at the top.

## Constraints

- Desktop app: Rust engine, Tauri 2, Svelte 5, SQLite library (new migration for sets and entries); existing patterns apply.
- The engine's existing secondary-output tap shows how to hand audio out of the callback without blocking; recording follows that approach.
- Music files are never modified. Recording and history write only to the chosen recording folder and the library database.
- Must work on macOS, Windows and Linux.

## Left to the builder

- Where on-air detection runs (engine snapshot versus a dedicated event from the audio thread) as long as take-over times are accurate to within one audio block.
- The exact threshold for "fully faded out" (e.g. channel gain to the master below −60 dB).
- RF64 versus file splitting; FLAC encoder choice; resampler choice.
- Database schema details and how a set in progress survives a crash.
