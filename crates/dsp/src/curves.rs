use std::f32::consts::FRAC_PI_2;

use deckforge_common::CrossfaderCurve;

/// Gains for decks A and B; `position` runs 0.0 (full A) to 1.0 (full B).
pub fn crossfader_gains(position: f32, curve: CrossfaderCurve) -> (f32, f32) {
    let x = position.clamp(0.0, 1.0);
    match curve {
        CrossfaderCurve::Linear => (1.0 - x, x),
        CrossfaderCurve::EqualPower => ((x * FRAC_PI_2).cos(), (x * FRAC_PI_2).sin()),
        CrossfaderCurve::Sharp => {
            const CUT: f32 = 0.04;
            (((1.0 - x) / CUT).min(1.0), (x / CUT).min(1.0))
        }
        CrossfaderCurve::Off => (1.0, 1.0),
    }
}

/// Audio-taper channel fader: unity at the top, roughly -12 dB at the midpoint.
pub fn channel_fader_gain(value: f32) -> f32 {
    let v = value.clamp(0.0, 1.0);
    v * v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_power_keeps_constant_power() {
        for i in 0..=20 {
            let (a, b) = crossfader_gains(i as f32 / 20.0, CrossfaderCurve::EqualPower);
            assert!((a * a + b * b - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn endpoints_isolate_each_deck() {
        for curve in [CrossfaderCurve::Linear, CrossfaderCurve::EqualPower, CrossfaderCurve::Sharp] {
            let (a, b) = crossfader_gains(0.0, curve);
            assert!((a - 1.0).abs() < 1e-6 && b.abs() < 1e-6, "{curve:?}");
            let (a, b) = crossfader_gains(1.0, curve);
            assert!(a.abs() < 1e-6 && (b - 1.0).abs() < 1e-6, "{curve:?}");
        }
    }

    #[test]
    fn sharp_curve_keeps_both_full_in_middle() {
        assert_eq!(crossfader_gains(0.5, CrossfaderCurve::Sharp), (1.0, 1.0));
        let (a, b) = crossfader_gains(0.5, CrossfaderCurve::Linear);
        assert!((a - 0.5).abs() < 1e-6 && (b - 0.5).abs() < 1e-6);
    }
}
