const SNAP_SECS: f64 = 0.03;
const SMOOTHING_TAU: f64 = 0.08;
const MAX_EXTRAPOLATION: f64 = 2.0;
const RESYNC_GAP: f64 = 0.25;
const DRAG_RELEASE_TIMEOUT: f64 = 0.35;
const DRAG_RELEASE_TOLERANCE: f64 = 0.04;

/// One published engine state for a deck; `at` is the host-clock time (seconds) the position was valid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeckSample {
    pub pos: f64,
    pub rate: f64,
    pub moving: bool,
    pub token: u64,
    pub at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    None,
    Active { secs: f64, at: f64 },
    Releasing { secs: f64, anchor: f64, since: f64 },
}

/// Smoothly extrapolates a deck playhead between engine snapshots without visible jitter.
#[derive(Debug, Clone)]
pub struct DeckClock {
    sample: Option<DeckSample>,
    shown: f64,
    last_now: Option<f64>,
    drag: Drag,
    shifted: bool,
}

impl Default for DeckClock {
    fn default() -> Self {
        Self::new()
    }
}

impl DeckClock {
    pub fn new() -> Self {
        Self {
            sample: None,
            shown: 0.0,
            last_now: None,
            drag: Drag::None,
            shifted: false,
        }
    }

    pub fn push(&mut self, sample: DeckSample) {
        self.shifted = self.sample.is_some_and(|s| s.token == sample.token && s.pos != sample.pos && !sample.moving);
        self.sample = Some(sample);
    }

    /// Pins the playhead to `secs` while the user drags; `None` hands control back to the engine.
    pub fn set_drag(&mut self, secs: Option<f64>, now: f64) {
        self.drag = match (secs, self.drag) {
            (Some(s), _) => Drag::Active { secs: s, at: now },
            (None, Drag::Active { secs, at }) => Drag::Releasing {
                secs,
                anchor: at,
                since: now,
            },
            (None, other) => other,
        };
    }

    pub fn is_settling(&self) -> bool {
        !matches!(self.drag, Drag::None)
    }

    pub fn is_moving(&self) -> bool {
        matches!(self.drag, Drag::Active { .. }) || self.shifted || self.sample.is_some_and(|s| s.moving)
    }

    fn target(&self, now: f64) -> Option<f64> {
        let s = self.sample?;
        let age = (now - s.at).clamp(0.0, MAX_EXTRAPOLATION);
        Some(if s.moving {
            s.pos + age * s.rate
        } else {
            s.pos
        })
    }

    /// Playhead position in track seconds at host time `now`.
    pub fn position(&mut self, now: f64) -> f64 {
        let previous_now = self.last_now.replace(now);
        match self.drag {
            Drag::Active { secs, .. } => {
                self.shown = secs;
                return secs;
            }
            Drag::Releasing {
                secs,
                anchor,
                since,
            } => {
                let rate = self.sample.filter(|s| s.moving).map_or(0.0, |s| s.rate);
                let held = secs + (now - anchor).max(0.0) * rate;
                let converged = self.sample.is_some_and(|s| {
                    s.at >= since
                        && (self.target(now).unwrap_or(s.pos) - held).abs() < DRAG_RELEASE_TOLERANCE
                });
                self.shown = held;
                if converged || now - since > DRAG_RELEASE_TIMEOUT {
                    self.drag = Drag::None;
                } else {
                    return held;
                }
            }
            Drag::None => {}
        }
        let (Some(sample), Some(target)) = (self.sample, self.target(now)) else {
            return self.shown;
        };
        let dt = previous_now.map_or(f64::INFINITY, |p| (now - p).max(0.0));
        let predicted = self.shown + dt.min(RESYNC_GAP) * sample.rate;
        let error = target - predicted;
        let resync = !sample.moving || dt > RESYNC_GAP || error.abs() > SNAP_SECS;
        if resync {
            self.shown = target;
        } else {
            let next = predicted + error * (1.0 - (-dt / SMOOTHING_TAU).exp());
            self.shown = if sample.rate >= 0.0 {
                next.max(self.shown)
            } else {
                next.min(self.shown)
            };
        }
        self.shown
    }
}

/// Maps the audio clock (frames processed) onto the host clock, filtering poll and scheduling jitter.
#[derive(Debug, Clone, Default)]
pub struct AudioSync {
    offset: Option<f64>,
    last_frames: u64,
    late: u32,
    late_min: f64,
}

const OFFSET_RISE: f64 = 1e-4;
/// The audio clock fell this far behind the host clock (a dropout or stalled callback) ...
const LATE_SECS: f64 = 0.01;
/// ... for this many polls in a row, so the offset re-locks instead of creeping up.
const LATE_POLLS: u32 = 8;

impl AudioSync {
    /// Returns the estimated host time at which the snapshot with `frames` processed was produced.
    pub fn observe(&mut self, host_secs: f64, frames: u64, sample_rate: u32) -> f64 {
        let audio_secs = frames as f64 / f64::from(sample_rate.max(1));
        let raw = host_secs - audio_secs;
        if self.offset.is_some_and(|o| raw - o > LATE_SECS) {
            self.late_min = if self.late == 0 { raw } else { self.late_min.min(raw) };
            self.late += 1;
        } else {
            self.late = 0;
        }
        let offset = match self.offset {
            None => raw,
            Some(_) if frames < self.last_frames => raw,
            Some(_) if self.late >= LATE_POLLS => {
                self.late = 0;
                self.late_min
            }
            Some(o) if raw < o => raw,
            Some(o) => o + (raw - o) * OFFSET_RISE,
        };
        self.offset = Some(offset);
        self.last_frames = frames;
        offset + audio_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 1.0;

    fn sample(pos: f64, rate: f64, at: f64) -> DeckSample {
        DeckSample {
            pos,
            rate,
            moving: true,
            token: 1,
            at,
        }
    }

    #[test]
    fn the_audio_clock_recovers_quickly_after_a_dropout() {
        let mut sync = AudioSync::default();
        let rate = 48_000u32;
        let mut frames = 0u64;
        let mut host = 0.0;
        for _ in 0..200 {
            host += 0.005;
            frames += 240;
            sync.observe(host, frames, rate);
        }
        host += 0.4;
        let mut at = 0.0;
        for _ in 0..40 {
            host += 0.005;
            frames += 240;
            at = sync.observe(host, frames, rate);
        }
        assert!((host - at).abs() < 0.02, "still {:.3} s behind after a dropout", host - at);
    }

    #[test]
    fn a_paused_deck_being_searched_keeps_redrawing() {
        let mut clock = DeckClock::new();
        let still = |pos: f64, at: f64| DeckSample { pos, rate: 0.0, moving: false, token: 1, at };
        clock.push(still(10.0, 0.0));
        clock.push(still(10.0, 0.016));
        assert!(!clock.is_moving());
        clock.push(still(10.4, 0.032));
        assert!(clock.is_moving(), "position changed while paused");
        clock.push(still(10.4, 0.048));
        assert!(!clock.is_moving(), "search stopped");
    }

    #[test]
    fn advances_smoothly_between_sparse_snapshots() {
        let mut c = DeckClock::new();
        c.push(sample(10.0, RATE, 0.0));
        let mut prev = c.position(0.0);
        for i in 1..=120 {
            let now = i as f64 / 120.0;
            let p = c.position(now);
            assert!(
                (p - prev - 1.0 / 120.0).abs() < 1e-9,
                "step {i}: {}",
                p - prev
            );
            prev = p;
        }
    }

    #[test]
    fn tempo_change_takes_effect_without_a_jump() {
        let mut c = DeckClock::new();
        c.push(sample(0.0, 1.0, 0.0));
        let mut t = 0.0;
        let mut prev = c.position(t);
        for i in 0..240 {
            t += 1.0 / 120.0;
            if i == 60 {
                c.push(sample(1.0 / 2.0 + 0.0, 1.08, t - 0.001));
            }
            let p = c.position(t);
            assert!(p >= prev, "backwards at {i}");
            assert!(p - prev < 0.0125, "jump at {i}: {}", p - prev);
            prev = p;
        }
    }

    #[test]
    fn pause_freezes_exactly_at_the_published_position() {
        let mut c = DeckClock::new();
        c.push(sample(5.0, 1.0, 0.0));
        c.position(0.0);
        c.position(0.1);
        c.push(DeckSample {
            pos: 5.1,
            rate: 1.0,
            moving: false,
            token: 1,
            at: 0.1,
        });
        assert_eq!(c.position(0.116), 5.1);
        assert_eq!(c.position(0.5), 5.1);
    }

    #[test]
    fn seeks_and_cue_jumps_snap() {
        let mut c = DeckClock::new();
        c.push(sample(5.0, 1.0, 0.0));
        c.position(0.0);
        c.position(0.01);
        c.push(sample(60.0, 1.0, 0.02));
        let p = c.position(0.02);
        assert!((p - 60.0).abs() < 1e-9);
        c.push(DeckSample {
            token: 2,
            ..sample(1.0, 1.0, 0.03)
        });
        assert!((c.position(0.03) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn noisy_snapshot_timing_never_moves_backwards_and_stays_close() {
        let mut c = DeckClock::new();
        let mut seed = 0x1234_5678_u32;
        let mut rnd = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f64 / (1u32 << 24) as f64
        };
        let truth = |t: f64| 30.0 + t * 1.04;
        let mut prev = f64::MIN;
        let mut next_snapshot = 0.0;
        let mut worst: f64 = 0.0;
        for i in 0..2400 {
            let now = i as f64 / 240.0;
            if now >= next_snapshot {
                let jitter = (rnd() - 0.5) * 0.004;
                let stamped = (now + jitter).max(0.0);
                c.push(sample(truth(stamped), 1.04, stamped));
                next_snapshot += 0.003 + rnd() * 0.004;
            }
            let p = c.position(now);
            assert!(p >= prev, "went backwards at frame {i}: {} < {prev}", p);
            prev = p;
            if now > 0.5 {
                worst = worst.max((p - truth(now)).abs());
            }
        }
        assert!(worst < 0.006, "worst error {worst}");
    }

    #[test]
    fn drag_pins_then_hands_back_without_a_visible_jump() {
        let mut c = DeckClock::new();
        c.push(DeckSample {
            pos: 10.0,
            rate: 1.0,
            moving: false,
            token: 1,
            at: 0.0,
        });
        assert_eq!(c.position(0.0), 10.0);
        c.set_drag(Some(42.0), 0.1);
        assert_eq!(c.position(0.11), 42.0);
        c.push(DeckSample {
            pos: 12.0,
            rate: 1.0,
            moving: false,
            token: 1,
            at: 0.12,
        });
        assert_eq!(c.position(0.13), 42.0);
        c.set_drag(None, 0.14);
        assert!(c.is_settling());
        assert_eq!(
            c.position(0.15),
            42.0,
            "engine has not applied the seek yet"
        );
        c.push(DeckSample {
            pos: 42.0,
            rate: 1.0,
            moving: false,
            token: 1,
            at: 0.17,
        });
        assert_eq!(c.position(0.18), 42.0);
        assert!(!c.is_settling());
        c.push(DeckSample {
            pos: 42.0,
            rate: 1.0,
            moving: false,
            token: 1,
            at: 0.2,
        });
        assert_eq!(c.position(0.2), 42.0);
    }

    #[test]
    fn drag_release_times_out_if_the_engine_never_follows() {
        let mut c = DeckClock::new();
        c.push(DeckSample {
            pos: 10.0,
            rate: 1.0,
            moving: false,
            token: 1,
            at: 0.0,
        });
        c.position(0.0);
        c.set_drag(Some(99.0), 0.0);
        c.position(0.01);
        c.set_drag(None, 0.02);
        c.push(DeckSample {
            pos: 10.0,
            rate: 1.0,
            moving: false,
            token: 1,
            at: 0.03,
        });
        assert_eq!(c.position(0.2), 99.0);
        assert_eq!(c.position(0.5), 10.0);
        assert!(!c.is_settling());
    }

    #[test]
    fn playing_drag_release_converges_into_normal_extrapolation() {
        let mut c = DeckClock::new();
        c.push(sample(10.0, 1.0, 0.0));
        c.position(0.0);
        c.set_drag(Some(50.0), 0.05);
        c.position(0.06);
        c.set_drag(None, 0.1);
        c.push(sample(50.07, 1.0, 0.12));
        let mut prev = c.position(0.12);
        assert!((prev - 50.07).abs() < 0.01);
        for i in 1..30 {
            let now = 0.12 + i as f64 / 120.0;
            let p = c.position(now);
            assert!(p >= prev && p - prev < 0.02, "frame {i}: {}", p - prev);
            prev = p;
        }
    }

    #[test]
    fn audio_sync_filters_late_polls_toward_the_true_cadence() {
        let mut sync = AudioSync::default();
        let sr = 48_000;
        let mut estimates = Vec::new();
        for i in 0..2000u64 {
            let frames = i * 480;
            let audio_t = frames as f64 / sr as f64;
            let late = if i % 7 == 0 { 0.003 } else { 0.0005 };
            estimates.push(sync.observe(100.0 + audio_t + late, frames, sr) - (100.0 + audio_t));
        }
        let tail = &estimates[1000..];
        let spread = tail.iter().cloned().fold(f64::MIN, f64::max)
            - tail.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread < 1e-6, "spread {spread}");
    }
}
