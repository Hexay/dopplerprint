//! Turn a base track into a harmonic hypothesis: which multiples are present, the true fundamental
//! (gcd of present multiples, so f0/2 comb ghosts collapse onto f0), and a sub-bin refined ridge.

use super::DiscoverParams;
use super::comb::harmonic_shift;
use crate::audio::LogSpectrogram;

#[derive(Clone, Debug)]
pub(super) struct Cand {
    pub start: usize,
    /// Fundamental position on the log axis (fractional bin) per frame.
    pub base: Vec<f32>,
    pub harmonics: Vec<u32>,
    pub strongest: u32,
    /// Stouffer-combined presence z over the detected harmonics.
    pub score: f64,
    /// Refined fundamental (Hz) per frame, folded from all detected harmonics.
    pub fund_hz: Vec<f64>,
    /// Per-frame whitened z at the strongest harmonic.
    pub peak_z: Vec<f32>,
}

impl Cand {
    pub fn end(&self) -> usize {
        self.start + self.base.len() - 1
    }
    pub fn base_at(&self, t: usize) -> Option<f32> {
        t.checked_sub(self.start).and_then(|i| self.base.get(i).copied())
    }
}

fn pooled(row: &[f32], pos: f64) -> Option<f32> {
    let i = pos.round();
    if i < 1.0 || i as usize + 1 >= row.len() {
        return None;
    }
    let i = i as usize;
    Some(row[i - 1].max(row[i]).max(row[i + 1]))
}

/// Presence z of multiple `h` of the base track: on-track pooled z minus the mean of two off-track controls.
/// The null spread comes from the controls' difference, so a line that fades (Lloyd nulls, crossings) is not
/// penalised for its own amplitude variation.
fn presence(spec: &LogSpectrogram, start: usize, base: &[f32], h: u32, p: &DiscoverParams) -> Option<f64> {
    let bpo = spec.bins_per_octave;
    let s = harmonic_shift(bpo, h as f64);
    let off = (0.4 * harmonic_shift(bpo, (h + 1) as f64 / h as f64)).max(3.0);
    let d: Vec<(f64, f64)> = base
        .iter()
        .enumerate()
        .filter_map(|(i, &u)| {
            let row = spec.row(start + i);
            let pos = u as f64 + s;
            let (on, lo, hi) = (pooled(row, pos)?, pooled(row, pos - off)?, pooled(row, pos + off)?);
            Some(((on - 0.5 * (lo + hi)) as f64, (lo - hi) as f64))
        })
        .collect();
    if d.len() < 4 || d.len() * 2 < base.len() {
        return None;
    }
    let n = d.len() as f64;
    let m = d.iter().map(|x| x.0).sum::<f64>() / n;
    let null_sd = (1.5 * d.iter().map(|x| x.1 * x.1).sum::<f64>() / (2.0 * n))
        .sqrt()
        .max(1e-3);
    Some(m / (null_sd / (n / p.frames_per_indep).max(1.0).sqrt()))
}

pub(super) fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Sub-bin peak (parabolic) of the whitened row within ±2 bins of `pos`.
fn peak_near(row: &[f32], pos: f64) -> Option<(f64, f32)> {
    let c = pos.round() as i64;
    let (lo, hi) = ((c - 2).max(1), (c + 2).min(row.len() as i64 - 2));
    if lo > hi {
        return None;
    }
    let i = (lo..=hi).max_by(|&a, &b| row[a as usize].total_cmp(&row[b as usize]))? as usize;
    let (a, b, d) = (row[i - 1] as f64, row[i] as f64, row[i + 1] as f64);
    let den = a - 2.0 * b + d;
    let delta = if den < 0.0 {
        (0.5 * (a - d) / den).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    Some((i as f64 + delta, row[i]))
}

/// (fundamental track, detected (harmonic, z), Stouffer score).
type Hypothesis = (Vec<f32>, Vec<(u32, f64)>, f64);

/// One sub-base hypothesis `base / m`.
fn hypothesis(spec: &LogSpectrogram, start: usize, base: &[f32], m: u32, p: &DiscoverParams) -> Option<Hypothesis> {
    let bpo = spec.bins_per_octave;
    let sub_shift = harmonic_shift(bpo, m as f64) as f32;
    let sub: Vec<f32> = base.iter().map(|u| u - sub_shift).collect();
    let present: Vec<(u32, f64)> = (1..=2 * p.harmonics)
        .filter_map(|h| presence(spec, start, &sub, h, p).map(|z| (h, z)))
        .filter(|&(_, z)| z >= p.harmonic_z)
        .collect();
    let g = present.iter().fold(0, |g, &(h, _)| gcd(g, h));
    if g == 0 {
        return None;
    }
    let shift = harmonic_shift(bpo, g as f64) as f32;
    let fund: Vec<f32> = sub.iter().map(|u| u + shift).collect();
    let harmonics: Vec<(u32, f64)> = present
        .iter()
        .map(|&(h, z)| (h / g, z))
        .filter(|&(h, _)| h <= p.harmonics)
        .collect();
    if harmonics.is_empty() {
        return None;
    }
    let score = harmonics.iter().map(|x| x.1).sum::<f64>() / (harmonics.len() as f64).sqrt();
    Some((fund, harmonics, score))
}

/// The track may sit on any harmonic (the lowest visible line need not be h1), so sub-bases base/m are tried
/// and the best-scoring explanation wins; ties go to the smallest m.
pub(super) fn analyze(spec: &LogSpectrogram, start: usize, base: &[f32], p: &DiscoverParams) -> Option<Cand> {
    let (fund, harmonics, score) = (1..=3)
        .filter_map(|m| hypothesis(spec, start, base, m, p))
        .reduce(|best, h| if h.2 > best.2 * 1.05 { h } else { best })?;
    let strongest = harmonics
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .expect("non-empty")
        .0;

    let bpo = spec.bins_per_octave;
    let mut fund_hz = Vec::with_capacity(fund.len());
    let mut peak_z = Vec::with_capacity(fund.len());
    for (i, &u) in fund.iter().enumerate() {
        let row = spec.row(start + i);
        let (mut num, mut den, mut zs) = (0.0, 0.0, 0.0f32);
        for &(k, _) in &harmonics {
            let Some((pk, zk)) = peak_near(row, u as f64 + harmonic_shift(bpo, k as f64)) else {
                continue;
            };
            if k == strongest {
                zs = zk;
            }
            let w = (zk.max(0.0) as f64).powi(2);
            num += w * spec.freq_of(pk) / k as f64;
            den += w;
        }
        fund_hz.push(if den > 0.0 { num / den } else { spec.freq_of(u as f64) });
        peak_z.push(zs);
    }
    Some(Cand {
        start,
        base: fund,
        harmonics: harmonics.iter().map(|x| x.0).collect(),
        strongest,
        score,
        fund_hz,
        peak_z,
    })
}
