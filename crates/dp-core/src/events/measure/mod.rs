//! Measurement harness: `#[ignore]` tests that print markdown tables, not assertions.
//! Run: cargo test --release -p dp-core events::measure::<name> -- --ignored --nocapture --test-threads=1
//! Results and method: research_notes/Doppler localization verification/pipeline_bias_measurements.md

mod candan;
mod codec;
mod corpus;
mod decode_align;
mod drift;
mod fitbias;
mod fitvariants;
mod passdiag;
mod picket;
mod selecteval;

use super::testsynth::Rng;

pub(super) const C: f64 = 343.0;

/// Harmonic tone with received fundamental `f(t)`, phase integrated per sample (trapezoid, f64), plus white noise.
pub(super) fn synth(sr: u32, dur: f64, f: &dyn Fn(f64) -> f64, harm: &[f64], amp: f64, noise: f64, seed: u64) -> Vec<f32> {
    let n = (dur * sr as f64) as usize;
    let dt = 1.0 / sr as f64;
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let (mut ph, mut f_prev) = (0.0f64, f(0.0));
    (0..n)
        .map(|i| {
            if i > 0 {
                let fi = f(i as f64 * dt);
                ph += std::f64::consts::PI * (f_prev + fi) * dt;
                f_prev = fi;
            }
            let s: f64 = harm.iter().enumerate().map(|(k, a)| a * ((k + 1) as f64 * ph).sin()).sum();
            (amp * s + noise * rng.gauss()) as f32
        })
        .collect()
}

/// Tone amplitude for a given SNR (dB) of the tone against white noise power in a 1 Hz band (unit noise σ).
pub(super) fn amp_for_snr_1hz(sr: u32, snr_db: f64) -> f64 {
    (2.0 * 10f64.powf(snr_db / 10.0) / (0.5 * sr as f64)).sqrt()
}

pub(super) fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len().max(1) as f64
}

pub(super) fn sd(x: &[f64]) -> f64 {
    let m = mean(x);
    (x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (x.len().max(2) - 1) as f64).sqrt()
}

pub(super) fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

pub(super) fn autocorr(x: &[f64], lag: usize) -> f64 {
    if x.len() <= lag + 2 {
        return f64::NAN;
    }
    let m = mean(x);
    let v: f64 = x.iter().map(|a| (a - m).powi(2)).sum();
    x.windows(lag + 1).map(|w| (w[0] - m) * (w[lag] - m)).sum::<f64>() / v.max(1e-300)
}

pub(super) const N_OFFSET_BINS: usize = 10;

/// Error vs fractional log-bin offset of the true frequency (offset in [-0.5, 0.5)).
pub(super) struct Picket {
    /// Mean error per offset bin.
    pub curve: [f64; N_OFFSET_BINS],
    /// −d(err)/d(offset): 0 = unbiased, 1 = fully snapped to bin centres.
    pub pull: f64,
    /// Fraction of error variance explained by the offset curve (deterministic, track-correlated part).
    pub share: f64,
}

pub(super) fn offset_of(pos: f64) -> f64 {
    pos - pos.round()
}

fn slot(off: f64) -> usize {
    (((off + 0.5) * N_OFFSET_BINS as f64) as usize).min(N_OFFSET_BINS - 1)
}

pub(super) fn picket(off: &[f64], err: &[f64]) -> Picket {
    let (mut s, mut n) = ([0.0; N_OFFSET_BINS], [0usize; N_OFFSET_BINS]);
    for (&o, &e) in off.iter().zip(err) {
        s[slot(o)] += e;
        n[slot(o)] += 1;
    }
    let curve: [f64; N_OFFSET_BINS] = std::array::from_fn(|i| if n[i] > 0 { s[i] / n[i] as f64 } else { f64::NAN });
    let (mo, me) = (mean(off), mean(err));
    let cov: f64 = off.iter().zip(err).map(|(o, e)| (o - mo) * (e - me)).sum();
    let var_o: f64 = off.iter().map(|o| (o - mo).powi(2)).sum();
    let fitted: Vec<f64> = off.iter().map(|&o| curve[slot(o)]).collect();
    let var_e: f64 = err.iter().map(|e| (e - me).powi(2)).sum();
    let var_f: f64 = fitted.iter().map(|f| (f - me).powi(2)).sum();
    Picket { curve, pull: -cov / var_o.max(1e-300), share: var_f / var_e.max(1e-300) }
}
