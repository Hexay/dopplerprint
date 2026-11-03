//! Source-frame tone model: f0(τ) with linear drift, an optional ramped step and slow sinusoidal rpm
//! wander, integrated in closed form so φ(τ) is exact at any emission time (no accumulated error, no
//! shared state). Optional per-harmonic AM and a half-order line.

use std::f64::consts::TAU;

use super::chain::modulation::Am;
use super::noise::Pcg;
use super::scenario::Source;

const WANDER_TERMS: usize = 4;

#[derive(Clone, Debug)]
pub struct Tone {
    f0: f64,
    drift: f64,
    /// (amplitude as a fraction of f0, frequency Hz, phase rad)
    wander: [(f64, f64, f64); WANDER_TERMS],
    step: Step,
    pub harmonics: Vec<(f64, f64)>,
    /// (amplitude, phase) of the 0.5·f0 line.
    pub half: Option<(f64, f64)>,
    pub am: Am,
}

/// f0 multiplier term `frac · ramp(τ)`, ramp rising linearly from 0 to 1 over `ramp` centred at `at`.
#[derive(Clone, Copy, Debug)]
struct Step {
    frac: f64,
    at: f64,
    ramp: f64,
}

impl Step {
    fn level(&self, tau: f64) -> f64 {
        if self.ramp <= 1e-9 {
            return if tau >= self.at { self.frac } else { 0.0 };
        }
        self.frac * ((tau - self.at) / self.ramp + 0.5).clamp(0.0, 1.0)
    }

    /// ∫ of `level` from −∞ (where it is 0) to τ.
    fn cumulative(&self, tau: f64) -> f64 {
        let r = self.ramp.max(1e-9);
        let u = tau - (self.at - 0.5 * r);
        self.frac
            * if u <= 0.0 {
                0.0
            } else if u < r {
                u * u / (2.0 * r)
            } else {
                0.5 * r + (u - r)
            }
    }
}

impl Tone {
    /// `rng` draws exactly what the chain-free sim always drew; `mod_rng` feeds the optional extras.
    pub fn new(s: &Source, rng: &mut Pcg, mod_rng: &mut Pcg) -> Self {
        let a = s.jitter_frac * (2.0 / WANDER_TERMS as f64).sqrt();
        let wander = std::array::from_fn(|_| (a, 0.02 + 0.28 * rng.uniform(), TAU * rng.uniform()));
        let harmonics: Vec<(f64, f64)> = s.harmonics.iter().map(|&amp| (amp, TAU * rng.uniform())).collect();
        let half = (s.half_harmonic > 0.0).then(|| (s.half_harmonic, TAU * mod_rng.uniform()));
        let am = Am::new(mod_rng, harmonics.len() + 1, s.am_depth, s.am_rate_hz);
        let step = Step { frac: s.step_frac, at: s.step_time_s, ramp: s.step_ramp_s };
        Self { f0: s.f0, drift: s.drift_per_s, wander, step, harmonics, half, am }
    }

    /// Instantaneous source-frame fundamental at audio-relative time `tau`.
    pub fn f0_at(&self, tau: f64) -> f64 {
        let w: f64 = self.wander.iter().map(|&(a, nu, ph)| a * (TAU * nu * tau + ph).sin()).sum();
        self.f0 * (1.0 + self.drift * tau + self.step.level(tau) + w)
    }

    /// φ(τ) = 2π ∫₀^τ f0(s) ds.
    pub fn phase(&self, tau: f64) -> f64 {
        let w: f64 = self
            .wander
            .iter()
            .map(|&(a, nu, ph)| a / (TAU * nu) * (ph.cos() - (TAU * nu * tau + ph).cos()))
            .sum();
        let step = if self.step.frac == 0.0 { 0.0 } else { self.step.cumulative(tau) - self.step.cumulative(0.0) };
        TAU * self.f0 * (tau + 0.5 * self.drift * tau * tau + step + w)
    }
}

/// Pure-tone air absorption (dB/km), log-log interpolation of ISO 9613-1 at 20 °C / 70 % RH
/// (docs/research/aircraft_acoustics.md §2.3), ∝ f² outside the table.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn source(step_frac: f64, ramp: f64) -> Source {
        toml::from_str::<Source>(&format!(
            "f0 = 100.0\nharmonics = [1.0]\ndrift_per_s = 1e-4\nstep_frac = {step_frac}\nstep_time_s = 30.0\nstep_ramp_s = {ramp}"
        ))
        .unwrap()
    }

    #[test]
    fn phase_derivative_is_f0_through_drift_and_step() {
        for ramp in [0.0, 4.0] {
            let t = Tone::new(&source(0.02, ramp), &mut Pcg::new(1), &mut Pcg::new(2));
            for tau in [5.0, 29.0, 30.5, 31.9, 45.0] {
                let h = 1e-4;
                let f_num = (t.phase(tau + h) - t.phase(tau - h)) / (2.0 * h * TAU);
                assert!((f_num - t.f0_at(tau)).abs() < 1e-5, "ramp {ramp} τ {tau}: {f_num} vs {}", t.f0_at(tau));
            }
            assert!((t.f0_at(60.0) - 100.0 * (1.0 + 60e-4 + 0.02)).abs() < 1e-9);
        }
    }

    #[test]
    fn extras_do_not_change_the_primary_rng_draws() {
        let plain = Tone::new(&source(0.0, 3.0), &mut Pcg::new(7), &mut Pcg::new(1));
        let mut s = source(0.0, 3.0);
        s.am_depth = 0.3;
        s.half_harmonic = 0.5;
        let extra = Tone::new(&s, &mut Pcg::new(7), &mut Pcg::new(99));
        assert_eq!(plain.harmonics, extra.harmonics);
        assert_eq!(plain.phase(12.3), extra.phase(12.3));
        assert!(extra.half.is_some() && (extra.am.gain(0, 3.0) - 1.0).abs() > 0.0);
    }
}
