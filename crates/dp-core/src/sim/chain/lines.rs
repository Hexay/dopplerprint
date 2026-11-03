//! Persistent local narrowband lines (motor slip, fans, unexplained 25 Hz): fixed nominal frequency,
//! small slow wander, optional harmonics (corpus_signatures.md §3–4).

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use super::config::Line;
use super::modulation::SlowRandom;
use super::tnr_amplitude;
use crate::sim::noise::Pcg;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LineTruth {
    pub hz: f64,
    pub wander_hz: f64,
    /// Amplitude of k·hz for k = 1, 2, ... in the physical mix.
    pub amplitudes: Vec<f64>,
}

pub fn add(buf: &mut [f64], fs: f64, lines: &[Line], snr_db: f64, rng: &mut Pcg) -> Vec<LineTruth> {
    lines.iter().map(|l| add_one(buf, fs, l, snr_db, rng)).collect()
}

fn add_one(buf: &mut [f64], fs: f64, l: &Line, snr_db: f64, rng: &mut Pcg) -> LineTruth {
    let peak = l.harmonics.iter().fold(0.0_f64, |m, &a| m.max(a)).max(1e-12);
    let amplitudes: Vec<f64> = l.harmonics.iter().map(|&a| a.max(0.0) / peak * tnr_amplitude(l.tnr_db, snr_db)).collect();
    let thetas: Vec<f64> = amplitudes.iter().map(|_| TAU * rng.uniform()).collect();
    let wander = SlowRandom::new(rng, 0.005, 0.1);
    for (i, y) in buf.iter_mut().enumerate() {
        let t = i as f64 / fs;
        let phase = TAU * (l.hz * t + l.wander_hz * wander.integral(t));
        *y += amplitudes
            .iter()
            .zip(&thetas)
            .enumerate()
            .filter(|(_, (a, _))| **a > 0.0)
            .map(|(k, (a, th))| a * ((k + 1) as f64 * phase + th).sin())
            .sum::<f64>();
    }
    LineTruth { hz: l.hz, wander_hz: l.wander_hz, amplitudes }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{db, line_power};
    use super::*;

    #[test]
    fn lines_sit_at_their_frequencies_and_levels() {
        let fs = 4000.0;
        let mut x = vec![0.0; (60.0 * fs) as usize];
        let lines = [
            Line { hz: 58.25, tnr_db: 6.0, harmonics: vec![1.0, 0.0, 0.5], wander_hz: 0.01 },
            Line { hz: 25.0, tnr_db: 0.0, harmonics: vec![1.0], wander_hz: 0.005 },
        ];
        let t = add(&mut x, fs, &lines, 40.0, &mut Pcg::new(2));
        let p = |f: f64| db(line_power(&x, fs, f, 0.5));
        let expect = |tnr: f64| db(0.5 * tnr_amplitude(tnr, 40.0).powi(2));
        assert!((p(58.25) - expect(6.0)).abs() < 1.0, "58.25 at {}", p(58.25));
        assert!((p(174.75) - expect(6.0) - db(0.25)).abs() < 1.0, "3x at {}", p(174.75));
        assert!((p(25.0) - expect(0.0)).abs() < 1.0);
        assert!(p(116.5) < expect(6.0) - 40.0, "empty 2x slot at {}", p(116.5));
        assert_eq!(t[0].amplitudes.len(), 3);
    }
}
