//! Wind noise: 3rd-order low-passed Gaussian rumble gated by an on/off gust process whose on-time
//! covers `fraction` of the clip, with lognormal gust strengths (corpus_signatures.md §6).

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use super::config::Wind;
use super::tnr_amplitude;
use crate::sim::noise::Pcg;

const ENV_SMOOTH_S: f64 = 0.15;
const GUST_LOG_SD: f64 = 0.5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindTruth {
    /// Fraction of samples inside a gust.
    pub fraction: f64,
    /// Gust intervals [start_s, end_s] in audio time.
    pub gusts: Vec<[f64; 2]>,
}

/// Alternating exponential off/on durations with mean on `gust_s` and on-fraction `fraction`.
fn gust_intervals(w: &Wind, duration_s: f64, rng: &mut Pcg) -> Vec<[f64; 2]> {
    if w.fraction <= 0.0 {
        return Vec::new();
    }
    let mean_off = w.gust_s * (1.0 - w.fraction) / w.fraction;
    let starts_on = rng.uniform() < w.fraction;
    let mut exp = |m: f64| -m * (1.0 - rng.uniform()).ln();
    let mut t = if starts_on { 0.0 } else { exp(mean_off) };
    let mut out = Vec::new();
    while t < duration_s {
        let end = (t + exp(w.gust_s)).min(duration_s);
        out.push([t, end]);
        t = end + exp(mean_off);
    }
    out
}

pub fn add(buf: &mut [f64], fs: f64, w: &Wind, snr_db: f64, rng: &mut Pcg) -> WindTruth {
    let duration_s = buf.len() as f64 / fs;
    let gusts = gust_intervals(w, duration_s, rng);
    let strength: Vec<f64> = gusts.iter().map(|_| (GUST_LOG_SD * rng.gauss()).exp()).collect();
    // Unit-gain cascade, so the rumble PSD below the corner equals the white input PSD 2σ²/fs.
    let b = 1.0 - (-TAU * w.corner_hz / fs).exp();
    let psd = 0.5 * tnr_amplitude(w.level_db, snr_db).powi(2);
    let sigma = (psd * fs / 2.0).sqrt();
    let k_env = 1.0 - (-1.0 / (ENV_SMOOTH_S * fs)).exp();
    let (mut s1, mut s2, mut s3, mut env) = (0.0, 0.0, 0.0, 0.0);
    let mut g = 0;
    let mut inside = 0usize;
    for (i, y) in buf.iter_mut().enumerate() {
        let t = i as f64 / fs;
        while g < gusts.len() && gusts[g][1] <= t {
            g += 1;
        }
        let target = match gusts.get(g) {
            Some(iv) if iv[0] <= t => {
                inside += 1;
                strength[g]
            }
            _ => 0.0,
        };
        env += k_env * (target - env);
        s1 += b * (sigma * rng.gauss() - s1);
        s2 += b * (s1 - s2);
        s3 += b * (s2 - s3);
        *y += env * s3;
    }
    WindTruth { fraction: inside as f64 / buf.len().max(1) as f64, gusts }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{band_power, db};
    use super::*;

    #[test]
    fn gusts_cover_the_requested_fraction_and_are_low_frequency() {
        let fs = 4000.0;
        let w = Wind { fraction: 0.3, ..Wind::default() };
        let mut x = vec![0.0; (600.0 * fs) as usize];
        let t = add(&mut x, fs, &w, 30.0, &mut Pcg::new(5));
        assert!((t.fraction - 0.3).abs() < 0.08, "fraction {}", t.fraction);
        assert!(db(band_power(&x, fs, 20.0, 80.0)) - db(band_power(&x, fs, 300.0, 1500.0)) > 40.0);
        // Per 0.5 s frame: frames inside gusts carry far more 20–80 Hz energy than frames outside.
        let frame = (0.5 * fs) as usize;
        let e: Vec<(bool, f64)> = x
            .chunks(frame)
            .enumerate()
            .filter_map(|(i, c)| {
                let mid = (i as f64 + 0.5) * 0.5;
                let inside = t.gusts.iter().any(|g| g[0] + 0.3 < mid && mid < g[1] - 0.3);
                let near = t.gusts.iter().any(|g| g[0] - 1.0 < mid && mid < g[1] + 1.0);
                (inside || !near).then(|| (inside, c.iter().map(|v| v * v).sum::<f64>()))
            })
            .collect();
        let mean = |on: bool| {
            let v: Vec<f64> = e.iter().filter(|p| p.0 == on).map(|p| p.1).collect();
            v.iter().sum::<f64>() / v.len() as f64
        };
        assert!(db(mean(true)) - db(mean(false)) > 15.0);
    }
}
