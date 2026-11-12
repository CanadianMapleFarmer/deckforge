use deckforge_common::DECK_COUNT;
use deckforge_dsp::{channel_fader_gain, crossfader_gains};
use deckforge_engine::EngineSnapshot;
use deckforge_library::history::{DeckObservation, Observation, SetTracker, TrackerEvent};
use deckforge_library::Library;

pub fn observation(snap: &EngineSnapshot) -> Observation {
    let (left, right) = if snap.master.crossfader_active { crossfader_gains(snap.master.crossfader, snap.master.curve) } else { (1.0, 1.0) };
    let decks = std::array::from_fn(|i| {
        let d = &snap.decks[i];
        let side = if i == 0 { left } else { right };
        DeckObservation {
            track_id: if d.loaded { d.track_token as i64 } else { 0 },
            playing: d.playing || d.jog_touched,
            gain: channel_fader_gain(snap.channels[i].fader) * side,
            bpm: (d.bpm > 0.0).then_some(d.bpm),
        }
    });
    Observation { clock_secs: snap.clock_secs, decks }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenSet {
    pub id: i64,
    pub clock_start: f64,
}

#[derive(Default)]
pub struct HistoryService {
    tracker: SetTracker,
    set: Option<OpenSet>,
    entries: [Option<i64>; DECK_COUNT],
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl HistoryService {
    pub fn open_set(&self) -> Option<OpenSet> {
        self.set
    }

    /// Feeds one engine snapshot through the tracker; returns true when the stored history changed.
    pub fn observe(&mut self, snap: &EngineSnapshot, library: &Library) -> bool {
        let events = self.tracker.observe(&observation(snap));
        self.apply(&events, library)
    }

    /// Starts a set now if none is running and returns the open set.
    pub fn force_start(&mut self, clock_secs: f64, library: &Library) -> Option<OpenSet> {
        if let Some(e) = self.tracker.force_start(clock_secs) {
            self.apply(&[e], library);
        }
        self.set
    }

    pub fn set_recording(&mut self, recording: bool) {
        self.tracker.set_recording(recording);
    }

    pub fn end(&mut self, clock_secs: f64, library: &Library) -> bool {
        match self.tracker.end(clock_secs) {
            Some(e) => self.apply(&[e], library),
            None => false,
        }
    }

    fn apply(&mut self, events: &[TrackerEvent], library: &Library) -> bool {
        let mut changed = false;
        for &event in events {
            if let Err(e) = self.apply_one(event, library) {
                log::warn!("set history: {e}");
            }
            changed = true;
        }
        changed
    }

    fn apply_one(&mut self, event: TrackerEvent, library: &Library) -> deckforge_library::Result<()> {
        match event {
            TrackerEvent::SetStarted { at } => {
                let id = library.begin_set(now_ms(), at)?;
                self.set = Some(OpenSet { id, clock_start: at });
                self.entries = [None; DECK_COUNT];
            }
            TrackerEvent::Audible { deck, track_id, at } => {
                let Some(set) = self.set else { return Ok(()) };
                self.entries[deck] = Some(library.add_set_entry(set.id, track_id, at - set.clock_start)?);
            }
            TrackerEvent::TookOver { deck, track_id, at } => {
                let Some(set) = self.set else { return Ok(()) };
                let entry = match self.entries[deck] {
                    Some(id) => id,
                    None => library.add_set_entry(set.id, track_id, at - set.clock_start)?,
                };
                self.entries[deck] = Some(entry);
                library.mark_took_over(entry, at - set.clock_start)?;
            }
            TrackerEvent::SetEnded { .. } => {
                if let Some(set) = self.set.take() {
                    library.end_set(set.id, now_ms())?;
                }
                self.entries = [None; DECK_COUNT];
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckforge_common::CrossfaderCurve;

    fn snap(clock: f64, decks: [(i64, bool, f32); 2], crossfader: f32) -> EngineSnapshot {
        let mut s = EngineSnapshot { clock_secs: clock, ..EngineSnapshot::default() };
        for (i, (track, playing, fader)) in decks.into_iter().enumerate() {
            s.decks[i].loaded = track != 0;
            s.decks[i].track_token = track as u64;
            s.decks[i].playing = playing;
            s.decks[i].bpm = 120.0;
            s.channels[i].fader = fader;
        }
        s.master.crossfader = crossfader;
        s.master.curve = CrossfaderCurve::default();
        s.master.crossfader_active = true;
        s
    }

    #[test]
    fn observation_uses_the_fader_curve_and_crossfader_side() {
        let o = observation(&snap(3.0, [(7, true, 0.5), (9, false, 1.0)], 0.5));
        let (l, r) = crossfader_gains(0.5, CrossfaderCurve::default());
        assert_eq!(o.clock_secs, 3.0);
        assert_eq!(o.decks[0], DeckObservation { track_id: 7, playing: true, gain: 0.25 * l, bpm: Some(120.0) });
        assert_eq!(o.decks[1].gain, r);
        let full_left = observation(&snap(0.0, [(7, true, 1.0), (9, true, 1.0)], 0.0));
        assert!(full_left.decks[1].gain < 0.001, "deck B is on the right side");
        let mut scratching = snap(0.0, [(7, false, 1.0), (0, false, 1.0)], 0.5);
        scratching.decks[0].jog_touched = true;
        assert!(observation(&scratching).decks[0].playing);
        assert_eq!(observation(&scratching).decks[1].track_id, 0);
    }

    #[test]
    fn an_inactive_crossfader_is_ignored() {
        let mut off = snap(0.0, [(7, true, 1.0), (9, true, 1.0)], 0.0);
        off.master.curve = CrossfaderCurve::Off;
        off.master.crossfader_active = false;
        assert_eq!(observation(&off).decks[1].gain, 1.0);
        let mut awaiting_pickup = snap(0.0, [(7, true, 1.0), (9, true, 1.0)], 0.0);
        awaiting_pickup.master.crossfader_active = false;
        assert_eq!(observation(&awaiting_pickup).decks[1].gain, 1.0, "B is audible until the crossfader is picked up");
    }

    #[test]
    fn a_snapshot_sequence_produces_a_stored_set() {
        let lib = Library::open_in_memory().unwrap();
        let mut h = HistoryService::default();
        let step = 0.01;
        let run = |h: &mut HistoryService, from: f64, to: f64, decks: [(i64, bool, f32); 2], xf: f32| {
            let mut k = (from / step).round() as i64;
            while (k as f64) * step < to {
                h.observe(&snap(k as f64 * step, decks, xf), &lib);
                k += 1;
            }
        };
        run(&mut h, 100.0, 160.0, [(1, true, 1.0), (2, false, 0.0)], 0.5);
        run(&mut h, 160.0, 170.0, [(1, true, 1.0), (2, true, 1.0)], 0.5);
        run(&mut h, 170.0, 200.0, [(1, true, 0.0), (2, true, 1.0)], 0.5);
        let set = h.open_set().unwrap();
        assert_eq!(set.clock_start, 100.0);
        let detail = lib.set_detail(set.id).unwrap();
        assert!(detail.summary.live);
        let took: Vec<_> = detail.entries.iter().map(|e| (e.track_id, e.took_over_secs.map(|t| (t * 100.0).round() / 100.0))).collect();
        assert_eq!(took, vec![(Some(1), Some(0.0)), (Some(2), Some(70.0))]);
        assert!((detail.entries[1].audible_secs - 60.0).abs() < 0.02);
        assert!(h.end(200.0, &lib));
        assert!(!lib.set_detail(set.id).unwrap().summary.live);
        assert!(h.open_set().is_none());
    }
}
