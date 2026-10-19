/// A deck counts as fully faded out at or below −60 dB to the master.
pub const SILENT_GAIN: f32 = 0.001;
pub const SET_IDLE_SECS: f64 = 1800.0;
const HOLD_WITHOUT_GRID_SECS: f64 = 0.5;
const DECKS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeckObservation {
    pub track_id: i64,
    pub playing: bool,
    pub gain: f32,
    pub bpm: Option<f64>,
}

impl DeckObservation {
    fn audible(&self) -> bool {
        self.track_id != 0 && self.playing && self.gain > SILENT_GAIN
    }

    fn hold_secs(&self) -> f64 {
        self.bpm.filter(|b| *b > 0.0).map_or(HOLD_WITHOUT_GRID_SECS, |b| 60.0 / b)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observation {
    pub clock_secs: f64,
    pub decks: [DeckObservation; DECKS],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackerEvent {
    SetStarted { at: f64 },
    Audible { deck: usize, track_id: i64, at: f64 },
    TookOver { deck: usize, track_id: i64, at: f64 },
    SetEnded { at: f64 },
}

#[derive(Debug, Clone, Copy, Default)]
struct Entry {
    took_over: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct DeckState {
    track_id: i64,
    entry: Option<Entry>,
    raw_since: Option<f64>,
    confirmed: bool,
    silent_since: f64,
}

/// Turns per-snapshot deck observations into set events: when each load became audible and when it took over.
#[derive(Debug, Clone)]
pub struct SetTracker {
    decks: [DeckState; DECKS],
    owner: Option<usize>,
    in_set: bool,
    recording: bool,
    quiet_since: Option<f64>,
}

impl Default for SetTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl SetTracker {
    pub fn new() -> Self {
        Self {
            decks: [DeckState { silent_since: f64::NEG_INFINITY, ..DeckState::default() }; DECKS],
            owner: None,
            in_set: false,
            recording: false,
            quiet_since: None,
        }
    }

    pub fn in_set(&self) -> bool {
        self.in_set
    }

    pub fn set_recording(&mut self, recording: bool) {
        self.recording = recording;
    }

    pub fn force_start(&mut self, at: f64) -> Option<TrackerEvent> {
        if self.in_set {
            return None;
        }
        self.in_set = true;
        self.quiet_since = Some(at);
        Some(TrackerEvent::SetStarted { at })
    }

    pub fn end(&mut self, at: f64) -> Option<TrackerEvent> {
        if !self.in_set {
            return None;
        }
        self.in_set = false;
        self.owner = None;
        self.quiet_since = None;
        for d in &mut self.decks {
            d.entry = None;
            d.raw_since = None;
            d.confirmed = false;
        }
        Some(TrackerEvent::SetEnded { at })
    }

    pub fn observe(&mut self, obs: &Observation) -> Vec<TrackerEvent> {
        let now = obs.clock_secs;
        let mut events = Vec::new();
        for (i, o) in obs.decks.iter().enumerate() {
            let d = &mut self.decks[i];
            if o.track_id != d.track_id {
                *d = DeckState { track_id: o.track_id, silent_since: now, ..DeckState::default() };
            }
            if o.audible() {
                let since = *d.raw_since.get_or_insert(now);
                if !d.confirmed && now - since + 1e-9 >= o.hold_secs() {
                    d.confirmed = true;
                    if !self.in_set {
                        self.in_set = true;
                        events.push(TrackerEvent::SetStarted { at: since });
                    }
                    let replaced = d.entry.is_some_and(|e| e.took_over) && self.owner != Some(i);
                    if d.entry.is_none() || replaced {
                        d.entry = Some(Entry::default());
                        events.push(TrackerEvent::Audible { deck: i, track_id: o.track_id, at: since });
                    }
                }
            } else {
                if d.raw_since.is_some() || d.confirmed {
                    d.silent_since = now;
                }
                d.raw_since = None;
                d.confirmed = false;
            }
        }
        for i in 0..DECKS {
            let d = self.decks[i];
            let Some(entry) = d.entry else { continue };
            if entry.took_over || !d.confirmed {
                continue;
            }
            let others_silent = (0..DECKS).filter(|&j| j != i).all(|j| !obs.decks[j].audible());
            if !others_silent {
                continue;
            }
            let others_out = (0..DECKS).filter(|&j| j != i).map(|j| self.decks[j].silent_since).fold(f64::NEG_INFINITY, f64::max);
            let at = d.raw_since.unwrap_or(now).max(others_out);
            self.decks[i].entry = Some(Entry { took_over: true });
            self.owner = Some(i);
            events.push(TrackerEvent::TookOver { deck: i, track_id: d.track_id, at });
        }
        if self.in_set {
            if obs.decks.iter().any(DeckObservation::audible) {
                self.quiet_since = None;
            } else {
                let quiet = *self.quiet_since.get_or_insert(now);
                if !self.recording && now - quiet >= SET_IDLE_SECS {
                    events.extend(self.end(quiet));
                }
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TrackerEvent::*;

    const STEP: f64 = 0.01;

    fn deck(track_id: i64, gain: f32) -> DeckObservation {
        DeckObservation { track_id, playing: true, gain, bpm: Some(128.0) }
    }

    fn off(track_id: i64) -> DeckObservation {
        DeckObservation { track_id, playing: true, gain: 0.0, bpm: Some(128.0) }
    }

    fn run(tracker: &mut SetTracker, from: f64, to: f64, at: impl Fn(f64) -> [DeckObservation; 2]) -> Vec<TrackerEvent> {
        let mut events = Vec::new();
        let (first, last) = ((from / STEP).round() as i64, (to / STEP).round() as i64);
        for k in first..last {
            let t = k as f64 * STEP;
            events.extend(tracker.observe(&Observation { clock_secs: t, decks: at(t) }));
        }
        events
    }

    fn approx(events: &[TrackerEvent]) -> Vec<TrackerEvent> {
        let r = |x: f64| (x * 100.0).round() / 100.0;
        events
            .iter()
            .map(|e| match *e {
                SetStarted { at } => SetStarted { at: r(at) },
                Audible { deck, track_id, at } => Audible { deck, track_id, at: r(at) },
                TookOver { deck, track_id, at } => TookOver { deck, track_id, at: r(at) },
                SetEnded { at } => SetEnded { at: r(at) },
            })
            .collect()
    }

    #[test]
    fn a_straight_blend_takes_over_when_the_old_deck_is_down() {
        let mut t = SetTracker::new();
        let events = run(&mut t, 0.0, 80.0, |s| {
            let a = if s < 75.0 { 1.0 } else { 0.0 };
            let b = if s < 60.0 { 0.0 } else { 0.3 };
            [deck(1, a), deck(2, b)]
        });
        assert_eq!(
            approx(&events),
            vec![
                SetStarted { at: 0.0 },
                Audible { deck: 0, track_id: 1, at: 0.0 },
                TookOver { deck: 0, track_id: 1, at: 0.0 },
                Audible { deck: 1, track_id: 2, at: 60.0 },
                TookOver { deck: 1, track_id: 2, at: 75.0 },
            ]
        );
    }

    #[test]
    fn a_quiet_fader_still_takes_over() {
        let mut t = SetTracker::new();
        let events = run(&mut t, 0.0, 95.0, |s| {
            let a = DeckObservation { track_id: 1, playing: s < 90.0, gain: 1.0, bpm: Some(128.0) };
            [a, deck(2, if s < 70.0 { 0.0 } else { 0.5 })]
        });
        assert!(approx(&events).contains(&TookOver { deck: 1, track_id: 2, at: 90.0 }), "{events:?}");
    }

    #[test]
    fn looping_or_jumping_within_a_load_adds_nothing() {
        let mut t = SetTracker::new();
        let events = run(&mut t, 0.0, 300.0, |_| [deck(1, 0.8), off(0)]);
        assert_eq!(events.len(), 3, "{events:?}");
    }

    #[test]
    fn a_tease_then_real_mix_is_one_entry() {
        let mut t = SetTracker::new();
        let events = run(&mut t, 0.0, 30.0, |s| {
            let a = if s < 25.0 { 1.0 } else { 0.0 };
            let b = if (10.0..12.0).contains(&s) || s >= 20.0 { 0.6 } else { 0.0 };
            [deck(1, a), deck(2, b)]
        });
        let b: Vec<_> = approx(&events).into_iter().filter(|e| matches!(e, Audible { deck: 1, .. } | TookOver { deck: 1, .. })).collect();
        assert_eq!(b, vec![Audible { deck: 1, track_id: 2, at: 10.0 }, TookOver { deck: 1, track_id: 2, at: 25.0 }]);
    }

    #[test]
    fn a_flip_back_is_a_new_entry() {
        let mut t = SetTracker::new();
        let events = run(&mut t, 0.0, 55.0, |s| {
            let a = if s < 30.0 || s >= 40.0 { 1.0 } else { 0.0 };
            let b = if (20.0..50.0).contains(&s) { 1.0 } else { 0.0 };
            [deck(1, a), deck(2, b)]
        });
        let tail: Vec<_> = approx(&events).into_iter().skip(5).collect();
        assert_eq!(tail, vec![Audible { deck: 0, track_id: 1, at: 40.0 }, TookOver { deck: 0, track_id: 1, at: 50.0 }], "{events:?}");
    }

    #[test]
    fn a_wobble_shorter_than_a_beat_is_ignored() {
        let mut t = SetTracker::new();
        let events = run(&mut t, 0.0, 40.0, |s| {
            let a = if s < 20.0 || (30.0..30.2).contains(&s) { 1.0 } else { 0.0 };
            let b = if s >= 10.0 { 1.0 } else { 0.0 };
            [deck(1, a), deck(2, b)]
        });
        assert_eq!(events.len(), 5, "{events:?}");
    }

    #[test]
    fn headphone_preview_never_appears() {
        let mut t = SetTracker::new();
        let events = run(&mut t, 0.0, 30.0, |_| [off(1), off(2)]);
        assert!(events.is_empty());
        assert!(!t.in_set());
    }

    #[test]
    fn thirty_minutes_of_silence_ends_the_set_but_not_while_recording() {
        let silent_after_100 = |s: f64| [deck(1, if s < 100.0 { 1.0 } else { 0.0 }), off(0)];
        let mut t = SetTracker::new();
        let events = run(&mut t, 0.0, 1901.0, silent_after_100);
        assert_eq!(approx(&events).last(), Some(&SetEnded { at: 100.0 }));
        assert!(!t.in_set());

        let mut t = SetTracker::new();
        t.set_recording(true);
        let events = run(&mut t, 0.0, 1901.0, silent_after_100);
        assert!(!events.iter().any(|e| matches!(e, SetEnded { .. })));
        assert!(t.in_set());
    }

    #[test]
    fn force_start_opens_a_set_before_anything_is_audible() {
        let mut t = SetTracker::new();
        let started = t.force_start(5.0);
        assert_eq!(started, Some(SetStarted { at: 5.0 }));
        let events = run(&mut t, 5.0, 12.0, |s| [deck(1, if s < 9.0 { 0.0 } else { 1.0 }), off(0)]);
        assert_eq!(approx(&events), vec![Audible { deck: 0, track_id: 1, at: 9.0 }, TookOver { deck: 0, track_id: 1, at: 9.0 }]);
        assert_eq!(t.end(20.0), Some(SetEnded { at: 20.0 }));
        assert_eq!(t.end(21.0), None);
    }

    #[test]
    fn a_new_set_starts_fresh_entries_for_the_same_load() {
        let mut t = SetTracker::new();
        run(&mut t, 0.0, 10.0, |_| [deck(1, 1.0), off(0)]);
        t.end(10.0);
        let events = run(&mut t, 10.0, 12.0, |_| [deck(1, 1.0), off(0)]);
        assert_eq!(approx(&events), vec![SetStarted { at: 10.0 }, Audible { deck: 0, track_id: 1, at: 10.0 }, TookOver { deck: 0, track_id: 1, at: 10.0 }]);
    }
}
