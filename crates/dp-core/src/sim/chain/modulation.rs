//! Smooth zero-mean random processes in closed form (sums of random sinusoids), so they can be
//! evaluated at any time without state: used for AM, line wander and turbulent path delay.

use std::f64::consts::TAU;

use crate::sim::noise::Pcg;

const TERMS: usize = 8;

#[derive(Clone, Debug)]
pub struct SlowRandom {
    /// (amplitude, frequency Hz, phase rad)
    terms: [(f64, f64, f64); TERMS],
}

impl SlowRandom {
    /// Unit-RMS process with spectral content uniform (in frequency) over [f_lo, f_hi].
    pub fn new(rng: &mut Pcg, f_lo: f64, f_hi: f64) -> Self {
        let a = (2.0 / TERMS as f64).sqrt();
        Self { terms: std::array::from_fn(|_| (a, f_lo + (f_hi - f_lo) * rng.uniform(), TAU * rng.uniform())) }
    }

    pub fn at(&self, t: f64) -> f64 {
        self.terms.iter().map(|&(a, f, p)| a * (TAU * f * t + p).sin()).sum()
    }

    /// ∫₀ᵗ of the process (for phase from a frequency wander).
    pub fn integral(&self, t: f64) -> f64 {
        self.terms.iter().map(|&(a, f, p)| a / (TAU * f) * (p.cos() - (TAU * f * t + p).cos())).sum()
    }
}

/// Fractional amplitude modulation per harmonic: gain(k, τ) = max(0, 1 + depth·g_k(τ)).
#[derive(Clone, Debug, Default)]
pub struct Am {
    depth: f64,
    per_harmonic: Vec<SlowRandom>,
}

impl Am {
    pub fn new(rng: &mut Pcg, n_harmonics: usize, depth: f64, rate_hz: f64) -> Self {
        if depth <= 0.0 {
            return Self::default();
        }
        let per_harmonic = (0..n_harmonics).map(|_| SlowRandom::new(rng, 0.3 * rate_hz, 1.7 * rate_hz)).collect();
        Self { depth, per_harmonic }
    }

    pub fn gain(&self, k: usize, tau: f64) -> f64 {
        self.per_harmonic.get(k).map_or(1.0, |g| (1.0 + self.depth * g.at(tau)).max(0.0))
    }
}

/// Turbulent excess delay on one path: phase of harmonic frequency f is 2π f δt(t).
#[derive(Clone, Debug)]
pub struct DelayJitter {
    rms_s: f64,
    g: SlowRandom,
}

/// Coherence is quoted at this frequency (validation_methodology.md §2 ablation axis).
pub const COHERENCE_REF_HZ: f64 = 250.0;

impl DelayJitter {
    /// RMS delay so that E[cos ψ] = `coherence` at 250 Hz for Gaussian ψ (σ² = −2 ln C).
    pub fn new(rng: &mut Pcg, coherence: f64, rate_hz: f64) -> Self {
        let sigma = (-2.0 * coherence.max(1e-6).ln()).sqrt();
        Self { rms_s: sigma / (TAU * COHERENCE_REF_HZ), g: SlowRandom::new(rng, 0.2 * rate_hz, 2.0 * rate_hz) }
    }

    pub fn phase(&self, f_hz: f64, t: f64) -> f64 {
        TAU * f_hz * self.rms_s * self.g.at(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_random_is_unit_rms_and_integral_matches() {
        let g = SlowRandom::new(&mut Pcg::new(3), 0.05, 0.5);
        let n = 200_000;
        let dt = 0.01;
        let ms = (0..n).map(|i| g.at(i as f64 * dt).powi(2)).sum::<f64>() / n as f64;
        assert!((ms.sqrt() - 1.0).abs() < 0.1, "rms {}", ms.sqrt());
        let numeric: f64 = (0..1000).map(|i| g.at((i as f64 + 0.5) * dt) * dt).sum();
        assert!((numeric - g.integral(10.0)).abs() < 1e-4);
    }

    #[test]
    fn delay_jitter_gives_requested_coherence() {
        for c in [0.2, 0.5, 0.8] {
            let mean: f64 = (0..32)
                .map(|s| {
                    let j = DelayJitter::new(&mut Pcg::new(s), c, 0.5);
                    (0..20_000).map(|i| j.phase(COHERENCE_REF_HZ, i as f64 * 0.05).cos()).sum::<f64>() / 20_000.0
                })
                .sum::<f64>()
                / 32.0;
            assert!((mean - c).abs() < 0.08, "coherence {c}: measured {mean}");
        }
    }
}
