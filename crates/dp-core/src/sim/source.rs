//! Source-frame tone model: f0(τ) with linear drift and slow sinusoidal rpm wander, integrated in
//! closed form so φ(τ) is exact at any emission time (no accumulated error, no shared state).

use std::f64::consts::TAU;

use super::noise::Pcg;
use super::scenario::Source;

const WANDER_TERMS: usize = 4;

#[derive(Clone, Debug)]
pub struct Tone {
    f0: f64,
    drift: f64,
    /// (amplitude as a fraction of f0, frequency Hz, phase rad)
    wander: [(f64, f64, f64); WANDER_TERMS],
    pub harmonics: Vec<(f64, f64)>,
}

impl Tone {
    pub fn new(s: &Source, rng: &mut Pcg) -> Self {
        let a = s.jitter_frac * (2.0 / WANDER_TERMS as f64).sqrt();
        let wander = std::array::from_fn(|_| (a, 0.02 + 0.28 * rng.uniform(), TAU * rng.uniform()));
        let harmonics = s.harmonics.iter().map(|&amp| (amp, TAU * rng.uniform())).collect();
        Self { f0: s.f0, drift: s.drift_per_s, wander, harmonics }
    }

    /// Instantaneous source-frame fundamental at audio-relative time `tau`.
    pub fn f0_at(&self, tau: f64) -> f64 {
        let w: f64 = self.wander.iter().map(|&(a, nu, ph)| a * (TAU * nu * tau + ph).sin()).sum();
        self.f0 * (1.0 + self.drift * tau + w)
    }

    /// φ(τ) = 2π ∫₀^τ f0(s) ds.
    pub fn phase(&self, tau: f64) -> f64 {
        let w: f64 = self
            .wander
            .iter()
            .map(|&(a, nu, ph)| a / (TAU * nu) * (ph.cos() - (TAU * nu * tau + ph).cos()))
            .sum();
        TAU * self.f0 * (tau + 0.5 * self.drift * tau * tau + w)
    }
}

/// Pure-tone air absorption (dB/km), log-log interpolation of ISO 9613-1 at 20 °C / 70 % RH
/// (research/aircraft_acoustics.md §2.3), ∝ f² outside the table.
pub fn absorption_db_per_km(f: f64) -> f64 {
    const TABLE: [(f64, f64); 7] =
        [(63.0, 0.09), (125.0, 0.33), (250.0, 1.12), (500.0, 2.79), (1000.0, 4.98), (2000.0, 9.0), (4000.0, 23.1)];
    let (f_lo, a_lo) = TABLE[0];
    let (f_hi, a_hi) = TABLE[TABLE.len() - 1];
    if f <= f_lo {
        return a_lo * (f / f_lo).powi(2);
    }
    if f >= f_hi {
        return a_hi * (f / f_hi).powi(2);
    }
    let i = TABLE.partition_point(|&(fi, _)| fi <= f);
    let ((f0, a0), (f1, a1)) = (TABLE[i - 1], TABLE[i]);
    let x = (f / f0).ln() / (f1 / f0).ln();
    (a0.ln() + x * (a1 / a0).ln()).exp()
}
