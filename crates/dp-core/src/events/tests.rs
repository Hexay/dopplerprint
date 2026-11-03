use super::fit::cv_freq;
use super::testsynth::{Flyover, Rng};
use super::*;
use crate::audio::{Audio, SpecParams, log_spectrogram};

const C: f64 = 343.0;

fn within(x: f64, r: (f64, f64)) -> bool {
    r.0 <= x && x <= r.1
}

/// `truth` = (f0, v, d, t_cpa_arrive); the ridge spans [t0, t1] at 0.25 s.
pub(super) fn ridge_from_model(truth: (f64, f64, f64, f64), t0: f64, t1: f64, noise_hz: f64, seed: u64) -> Ridge {
    let (f0, v, d, ta) = truth;
    let mut rng = Rng(seed);
    let t: Vec<f64> = (0..).map(|k| t0 + 0.25 * k as f64).take_while(|&t| t <= t1).collect();
    let f_hz = t
        .iter()
        .map(|&t| cv_freq(f0, v, d, ta, C, t) + noise_hz * rng.gauss())
        .collect();
    let snr = vec![3.0; t.len()];
    Ridge { t, f_hz, snr }
}

#[test]
fn cv_formula_limits() {
    let (f0, v, d, ta) = (100.0, 60.0, 500.0, 50.0);
    assert!(
        (cv_freq(f0, v, d, ta, C, ta) - f0).abs() < 1e-9,
        "f = f0 when the CPA sound arrives"
    );
    assert!((cv_freq(f0, v, d, ta, C, -1e6) - f0 * C / (C - v)).abs() < 1e-3);
    assert!((cv_freq(f0, v, d, ta, C, 1e6) - f0 * C / (C + v)).abs() < 1e-3);
}

#[test]
fn fit_recovers_full_pass_with_outliers() {
    let (f0, v, d, ta) = (112.0, 70.0, 900.0, 60.0);
    let mut r = ridge_from_model((f0, v, d, ta), 0.0, 120.0, 0.15, 5);
    for i in (0..r.f_hz.len()).step_by(17) {
        r.f_hz[i] += 6.0;
    }
    let fit = fit_cv(&r, C);
    let b = &fit.best;
    assert!((b.f0 / f0 - 1.0).abs() < 0.003, "f0 {}", b.f0);
    assert!((b.v / v - 1.0).abs() < 0.03, "v {}", b.v);
    assert!((b.d_cpa / d - 1.0).abs() < 0.08, "d {}", b.d_cpa);
    assert!((b.t_cpa_arrive - ta).abs() < 0.5, "ta {}", b.t_cpa_arrive);
    assert!(fit.rmse_hz < 0.3, "rmse {}", fit.rmse_hz);
    assert!(
        within(v, fit.v_range) && within(d, fit.d_range) && within(f0, fit.f0_range) && within(ta, fit.t_cpa_range)
    );
    assert!(
        fit.v_range.0 <= v * 0.8 && fit.d_range.1 >= 2.0 * d * 0.99,
        "manoeuvre floors applied: {fit:?}"
    );
}

#[test]
fn fit_partial_receding_ridge_keeps_truth_in_bounds() {
    let (f0, v, d, ta) = (85.0, 50.0, 400.0, 20.0);
    let r = ridge_from_model((f0, v, d, ta), 14.0, 70.0, 0.1, 9);
    let fit = fit_cv(&r, C);
    assert!(
        within(v, fit.v_range) && within(d, fit.d_range) && within(f0, fit.f0_range),
        "{fit:?}"
    );
}

fn flyover(f0: f64, v: f64, d: f64, ta: f64, harmonics: Vec<(u32, f64)>) -> Flyover {
    Flyover {
        f0,
        v,
        d,
        ta,
        alt_frac: 0.7,
        mic_h: 4.0,
        reflect: 0.8,
        harmonics,
        c: C,
    }
}

fn discover_audio(samples: Vec<f32>, sr: u32) -> Vec<Event> {
    let spec = log_spectrogram(
        &Audio {
            sample_rate: sr,
            samples,
        },
        &SpecParams::default(),
    );
    discover(&spec)
}

#[test]
fn discovers_faint_flyover_with_missing_fundamental_and_fringes() {
    let sr = 4000;
    let (f0, v, d, ta) = (72.0, 55.0, 700.0, 80.0);
    let fly = flyover(f0, v, d, ta, vec![(2, 0.02), (3, 0.016), (4, 0.012), (5, 0.008)]);
    let audio = fly.render(sr, 170.0, 0.25, &mut Rng(42));
    let events = discover_audio(audio, sr);
    assert_eq!(
        events.len(),
        1,
        "{:#?}",
        events.iter().map(|e| (&e.harmonics, &e.fit)).collect::<Vec<_>>()
    );
    let e = &events[0];
    let h = e.ridge_harmonic as f64;
    let fund = e.fit.best.f0 / h;
    eprintln!(
        "harmonics {:?} ridge_h {} snr {:.1} fit {:?}",
        e.harmonics, h, e.snr, e.fit
    );
    assert!(!e.harmonics.contains(&1), "fundamental was never emitted");
    assert!(e.harmonics.len() >= 2);
    assert!((fund / f0 - 1.0).abs() < 0.01, "fundamental {fund}");
    assert!(within(h * f0, e.fit.f0_range), "f0 range {:?}", e.fit.f0_range);
    assert!(within(v, e.fit.v_range), "v range {:?}", e.fit.v_range);
    assert!(within(d, e.fit.d_range), "d range {:?}", e.fit.d_range);
    assert!(within(ta, e.fit.t_cpa_range), "t range {:?}", e.fit.t_cpa_range);
    assert!((e.fit.best.v / v - 1.0).abs() < 0.1, "v {}", e.fit.best.v);
}

#[test]
fn separates_two_overlapping_aircraft_and_finds_low_doppler_pass() {
    let sr = 4000;
    let a = flyover(95.0, 65.0, 600.0, 60.0, vec![(1, 0.03), (2, 0.02), (3, 0.01)]);
    let b = flyover(130.0, 70.0, 900.0, 75.0, vec![(1, 0.03), (2, 0.015)]);
    let slow = flyover(160.0, 12.0, 150.0, 150.0, vec![(1, 0.03), (2, 0.02)]);
    let mut rng = Rng(7);
    let mut s = a.render(sr, 190.0, 0.2, &mut rng);
    for other in [
        b.render(sr, 190.0, 0.0, &mut rng),
        slow.render(sr, 190.0, 0.0, &mut rng),
    ] {
        s.iter_mut().zip(other).for_each(|(x, y)| *x += y);
    }
    let events = discover_audio(s, sr);
    let summary: Vec<_> = events
        .iter()
        .map(|e| {
            (
                e.fit.best.t_cpa_arrive,
                e.fit.best.f0 / e.ridge_harmonic as f64,
                e.fit.best.v,
                e.snr,
            )
        })
        .collect();
    eprintln!("{summary:?}");
    for (f0, v, ta) in [(95.0, 65.0, 60.0), (130.0, 70.0, 75.0), (160.0, 12.0, 150.0)] {
        // Harmonic number is unknowable when only one line survives (here a Lloyd null sits on the slow
        // pass's fundamental at CPA), so accept integer multiples.
        let hit = events.iter().filter(|e| {
            let k = e.fit.best.f0 / e.ridge_harmonic as f64 / f0;
            (k - k.round()).abs() < 0.02 * k
                && (1.0..=3.0).contains(&k.round())
                && within(ta, e.fit.t_cpa_range)
                && within(v, e.fit.v_range)
        });
        assert_eq!(hit.count(), 1, "f0 {f0}: {summary:?}");
    }
    assert_eq!(events.len(), 3, "{summary:?}");
}

#[test]
fn pure_noise_yields_no_events() {
    let sr = 4000;
    let mut rng = Rng(99);
    let s: Vec<f32> = (0..sr as usize * 300).map(|_| (0.2 * rng.gauss()) as f32).collect();
    let events = discover_audio(s, sr);
    assert!(
        events.is_empty(),
        "{:?}",
        events.iter().map(|e| (e.snr, &e.fit)).collect::<Vec<_>>()
    );
}

#[test]
fn clipped_flat_top_reads_as_its_centre() {
    let row = [0.0, 1.0, 8.0, 8.0, 8.0, 2.0, 0.0, 0.0];
    assert_eq!(candidate::peak_near(&row, 2.0), Some((3.0, 8.0)));
    let row = [0.0, 1.0, 8.0, 8.0, 1.0, 0.0];
    assert_eq!(candidate::peak_near(&row, 3.0), Some((2.5, 8.0)));
}
