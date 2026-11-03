//! Zero-padded Hann periodograms, the Welch long-term spectrum, line peaks and harmonic combination.

use std::sync::Arc;

use rayon::prelude::*;
use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

/// Half-width (Hz) of the band whose median is a line's noise floor.
pub(super) const FLOOR_HALF_HZ: f64 = 4.0;
const PAD: usize = 4;

pub(super) struct Spectrum {
    pub p: Vec<f64>,
    pub df: f64,
}

pub(super) struct Peak {
    pub f_hz: f64,
    /// Peak power over the local median floor (linear).
    pub snr: f64,
    /// False when the maximum sits on the window edge, i.e. a slope rather than a line.
    pub interior: bool,
}

impl Spectrum {
    fn bin(&self, f: f64) -> usize {
        ((f / self.df).round().max(0.0) as usize).min(self.p.len() - 1)
    }

    pub fn floor(&self, f: f64, half_hz: f64) -> f64 {
        let mut v = self.p[self.bin(f - half_hz)..=self.bin(f + half_hz)].to_vec();
        median(&mut v).max(1e-300)
    }

    pub fn peak(&self, lo: f64, hi: f64, floor: f64) -> Option<Peak> {
        let (i0, i1) = (self.bin(lo).max(1), self.bin(hi).min(self.p.len() - 2));
        if i0 > i1 {
            return None;
        }
        let i = (i0..=i1).max_by(|&a, &b| self.p[a].total_cmp(&self.p[b]))?;
        let (a, b, c) = (self.p[i - 1].ln(), self.p[i].ln(), self.p[i + 1].ln());
        let den = a - 2.0 * b + c;
        let d = if den < 0.0 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
        Some(Peak {
            f_hz: (i as f64 + d) * self.df,
            snr: self.p[i] / floor,
            interior: i0 < i && i < i1,
        })
    }
}

pub(super) struct Periodogram {
    fft: Arc<dyn RealToComplex<f64>>,
    win: Vec<f64>,
}

pub(super) struct Scratch {
    inp: Vec<f64>,
    out: Vec<Complex<f64>>,
}

impl Periodogram {
    pub fn new(n: usize) -> Self {
        let n_fft = (PAD * n).next_power_of_two();
        let win = (0..n)
            .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos())
            .collect();
        Self {
            fft: RealFftPlanner::<f64>::new().plan_fft_forward(n_fft),
            win,
        }
    }

    pub fn scratch(&self) -> Scratch {
        Scratch {
            inp: self.fft.make_input_vec(),
            out: self.fft.make_output_vec(),
        }
    }

    pub fn df(&self, sr: f64) -> f64 {
        sr / self.fft.len() as f64
    }

    /// Adds `|FFT(win·x)|²` into `acc` (length n_fft/2 + 1).
    pub fn accumulate(&self, x: &[f32], s: &mut Scratch, acc: &mut [f64]) {
        s.inp.fill(0.0);
        for ((o, &v), &w) in s.inp.iter_mut().zip(x).zip(&self.win) {
            *o = v as f64 * w;
        }
        self.fft.process(&mut s.inp, &mut s.out).expect("fft buffer sizes");
        for (a, c) in acc.iter_mut().zip(&s.out) {
            *a += c.norm_sqr();
        }
    }

    pub fn spectrum(&self, x: &[f32], sr: f64, s: &mut Scratch) -> Spectrum {
        let mut p = vec![0.0; s.out.len()];
        self.accumulate(x, s, &mut p);
        Spectrum { p, df: self.df(sr) }
    }
}

/// Mean periodogram over half-overlapping `seg_s` segments; None if the audio is shorter than one segment.
pub(super) fn welch(x: &[f32], sr: f64, seg_s: f64) -> Option<Spectrum> {
    let n = (seg_s * sr).round() as usize;
    if n < 16 || x.len() < n {
        return None;
    }
    let pg = Periodogram::new(n);
    let starts: Vec<usize> = (0..=x.len() - n).step_by(n / 2).collect();
    let len = pg.scratch().out.len();
    let p = starts
        .par_iter()
        .fold(
            || (pg.scratch(), vec![0.0; len]),
            |(mut s, mut acc), &st| {
                pg.accumulate(&x[st..st + n], &mut s, &mut acc);
                (s, acc)
            },
        )
        .map(|(_, acc)| acc)
        .reduce(|| vec![0.0; len], |a, b| a.iter().zip(&b).map(|(x, y)| x + y).collect());
    Some(Spectrum { p, df: pg.df(sr) })
}

/// Harmonic-weighted fundamental (Ojowu-style): harmonic k's estimate of f/k has variance ∝ 1/(k²·SNR).
/// `hits` are (k, f_k, snr_linear). Returns (fundamental, weighted spread).
pub(super) fn combine(hits: &[(u32, f64, f64)]) -> (f64, f64) {
    let w: Vec<f64> = hits.iter().map(|&(k, _, s)| (k * k) as f64 * (s - 1.0).max(1e-6)).collect();
    let sw: f64 = w.iter().sum();
    let f = hits.iter().zip(&w).map(|(&(k, fk, _), w)| w * fk / k as f64).sum::<f64>() / sw;
    let var = hits.iter().zip(&w).map(|(&(k, fk, _), w)| w * (fk / k as f64 - f).powi(2)).sum::<f64>() / sw;
    (f, var.sqrt())
}

pub(super) fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mid = v.len() / 2;
    *v.select_nth_unstable_by(mid, f64::total_cmp).1
}

/// 1.4826·MAD: a robust σ.
pub(super) fn robust_sigma(v: &[f64]) -> f64 {
    let mut c = v.to_vec();
    let m = median(&mut c);
    let mut d: Vec<f64> = v.iter().map(|x| (x - m).abs()).collect();
    1.4826 * median(&mut d)
}
