//! Mains hum: an OU-walk grid frequency (ENF) at 1 s steps, shared ×k by every harmonic, with weak
//! even-harmonic-heavy levels (corpus_signatures.md §3).

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use super::config::Hum;
use super::modulation::SlowRandom;
use super::tnr_amplitude;
use crate::sim::noise::Pcg;

pub const ENF_DT_S: f64 = 1.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HumTruth {
    pub mains_hz: f64,
    /// Grid frequency at audio times 0, dt, 2dt, ... (linear in between).
    pub enf_dt_s: f64,
    pub enf_hz: Vec<f64>,
    /// Mean amplitude of harmonic k = 1, 2, ... in the physical mix (amplitude-1 tone at 1 km = 1).
    pub amplitudes: Vec<f64>,
}

/// Grid frequency every `ENF_DT_S` covering `duration_s` (one extra sample for interpolation).
pub fn enf_trace(h: &Hum, duration_s: f64, rng: &mut Pcg) -> Vec<f64> {
    let n = (duration_s / ENF_DT_S).ceil() as usize + 2;
    let rho = (-ENF_DT_S / h.wander_tau_s.max(1e-9)).exp();
    let sigma = h.wander_mhz * 1e-3;
    let mut x = sigma * rng.gauss();
    (0..n)
        .map(|_| {
            let f = h.mains_hz + h.offset_mhz * 1e-3 + x + h.jitter_mhz * 1e-3 * rng.gauss();
            x = rho * x + sigma * (1.0 - rho * rho).sqrt() * rng.gauss();
            f
        })
        .collect()
}

pub fn add(buf: &mut [f64], fs: f64, h: &Hum, snr_db: f64, rng: &mut Pcg) -> HumTruth {
    let enf = enf_trace(h, buf.len() as f64 / fs, rng);
    let peak = h.harmonics.iter().fold(0.0_f64, |m, &a| m.max(a));
    let base = tnr_amplitude(h.tnr_db, snr_db) / peak;
    let amplitudes: Vec<f64> = h.harmonics.iter().map(|&a| a.max(0.0) * base).collect();
    let voices: Vec<(usize, f64, f64, SlowRandom)> = amplitudes
        .iter()
        .enumerate()
        .filter(|(_, a)| **a > 0.0)
        .map(|(k, &a)| (k + 1, a, TAU * rng.uniform(), SlowRandom::new(rng, 0.003, 0.03)))
        .collect();
    let wander = h.level_wander_db / 20.0 * std::f64::consts::LN_10;
    let mut phase = 0.0;
    for (i, y) in buf.iter_mut().enumerate() {
        let t = i as f64 / fs;
        let s = t / ENF_DT_S;
        let j = s as usize;
        let f = enf[j] + (enf[j + 1] - enf[j]) * (s - j as f64);
        *y += voices.iter().map(|(k, a, th, g)| a * (wander * g.at(t)).exp() * (*k as f64 * phase + th).sin()).sum::<f64>();
        phase = (phase + TAU * f / fs) % (TAU * 1e6);
    }
    HumTruth { mains_hz: h.mains_hz, enf_dt_s: ENF_DT_S, enf_hz: enf, amplitudes }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{db, line_power};
    use super::*;

    #[test]
    fn default_hum_is_even_harmonic_heavy_at_requested_level() {
        let fs = 2000.0;
        let mut x = vec![0.0; (120.0 * fs) as usize];
        let t = add(&mut x, fs, &Hum { level_wander_db: 0.0, ..Hum::default() }, 30.0, &mut Pcg::new(1));
        let p = |f: f64| db(line_power(&x, fs, f, 0.6));
        assert!(p(360.0) - p(60.0) > 20.0, "360 {} vs 60 {}", p(360.0), p(60.0));
        assert!(p(120.0) - p(60.0) > 18.0 && p(240.0) - p(180.0) > 8.0);
        // tnr 0 dB with snr 30 dB: strongest line amplitude 10^(-30/20), power a²/2.
        assert!((p(360.0) - db(0.5 * 1e-3)).abs() < 1.0, "360 Hz line at {} dB", p(360.0));
        assert!((t.amplitudes[5] - 10f64.powf(-1.5)).abs() < 1e-12);
    }

    #[test]
    fn enf_trace_has_requested_wander_statistics() {
        let h = Hum { jitter_mhz: 0.0, ..Hum::default() };
        let tr = enf_trace(&h, 36_000.0, &mut Pcg::new(9));
        let n = tr.len() as f64;
        let m = tr.iter().sum::<f64>() / n;
        let d: Vec<f64> = tr.iter().map(|f| f - m).collect();
        let var = d.iter().map(|v| v * v).sum::<f64>() / n;
        assert!((var.sqrt() * 1e3 - 14.0).abs() < 2.5, "std {} mHz", var.sqrt() * 1e3);
        let lag = 60;
        let ac = d.iter().zip(&d[lag..]).map(|(a, b)| a * b).sum::<f64>() / (n - lag as f64) / var;
        assert!((ac - (-1.0f64).exp()).abs() < 0.12, "lag-60 s autocorrelation {ac}");
    }

    #[test]
    fn hum_line_frequency_follows_truth_enf_times_k() {
        let fs = 2000.0;
        let h = Hum { harmonics: vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0], level_wander_db: 0.0, ..Hum::default() };
        let mut x = vec![0.0; (600.0 * fs) as usize];
        let t = add(&mut x, fs, &h, 0.0, &mut Pcg::new(4));
        // Complex demodulation at 360 Hz, 1 s boxcar per second, frequency from the phase step.
        let ph: Vec<f64> = (0..600)
            .map(|s| {
                let (mut re, mut im) = (0.0, 0.0);
                for (i, v) in x.iter().enumerate().skip(s * 2000).take(2000) {
                    let w = TAU * 360.0 * i as f64 / fs;
                    re += v * w.cos();
                    im -= v * w.sin();
                }
                im.atan2(re)
            })
            .collect();
        let errs: Vec<f64> = (0..599)
            .map(|s| {
                let dphi = (ph[s + 1] - ph[s] + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI;
                360.0 + dphi / TAU - 6.0 * t.enf_hz[s + 1]
            })
            .collect();
        let rms = (errs.iter().map(|e| e * e).sum::<f64>() / errs.len() as f64).sqrt();
        assert!(rms < 0.02, "ENF tracking rms {rms} Hz");
    }
}
