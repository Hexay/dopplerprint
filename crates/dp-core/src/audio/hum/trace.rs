//! Per-frame ENF trace: zero-padded Hann frames, sub-bin peaks per harmonic, SNR-weighted combination.

use rayon::prelude::*;

use super::spectrum::{FLOOR_HALF_HZ, Periodogram, Spectrum, combine};
use super::{HumFrame, HumParams};

/// A harmonic whose estimate of the fundamental disagrees by more than this with the strongest one is
/// dropped for the frame (typically an aircraft tone crossing that hum line).
const MAX_DISAGREE_HZ: f64 = 0.05;

pub(super) struct Trace {
    pub frames: Vec<HumFrame>,
    /// Per entry of `ks`: frames in which that harmonic alone clears `frame_snr_db`.
    pub k_valid: Vec<usize>,
}

pub(super) fn track(x: &[f32], sr: f64, f0: f64, ks: &[u32], p: &HumParams) -> Trace {
    let n = (p.frame_s * sr).round() as usize;
    let hop = ((p.hop_s * sr).round() as usize).max(1);
    if n < 16 || x.len() < n || ks.is_empty() {
        return Trace { frames: Vec::new(), k_valid: vec![0; ks.len()] };
    }
    let pg = Periodogram::new(n);
    let starts: Vec<usize> = (0..=x.len() - n).step_by(hop).collect();
    let per: Vec<(HumFrame, Vec<bool>)> = starts
        .par_iter()
        .map_init(
            || pg.scratch(),
            |s, &st| {
                let spec = pg.spectrum(&x[st..st + n], sr, s);
                frame(&spec, (st as f64 + 0.5 * n as f64) / sr, f0, ks, p)
            },
        )
        .collect();
    let mut k_valid = vec![0; ks.len()];
    for (_, ok) in &per {
        k_valid.iter_mut().zip(ok).for_each(|(c, &o)| *c += o as usize);
    }
    Trace { frames: per.into_iter().map(|(f, _)| f).collect(), k_valid }
}

fn frame(spec: &Spectrum, t_s: f64, f0: f64, ks: &[u32], p: &HumParams) -> (HumFrame, Vec<bool>) {
    let mut hits: Vec<(u32, f64, f64)> = Vec::new();
    let mut k_ok = vec![false; ks.len()];
    for (ok, &k) in k_ok.iter_mut().zip(ks) {
        let kf = k as f64 * f0;
        let hw = (k as f64 * p.search_hz).max(2.0 * spec.df);
        let Some(pk) = spec.peak(kf - hw, kf + hw, spec.floor(kf, FLOOR_HALF_HZ)) else { continue };
        let db = 10.0 * pk.snr.log10();
        if !pk.interior || db < p.min_line_snr_db {
            continue;
        }
        *ok = db >= p.frame_snr_db;
        hits.push((k, pk.f_hz, pk.snr));
    }
    let strongest = hits
        .iter()
        .max_by(|a, b| ((a.0 * a.0) as f64 * a.2).total_cmp(&((b.0 * b.0) as f64 * b.2)))
        .map(|&(k, f, _)| f / k as f64);
    if let Some(r) = strongest {
        hits.retain(|&(k, f, _)| (f / k as f64 - r).abs() <= MAX_DISAGREE_HZ);
    }
    let snr_db = hits.iter().map(|h| 10.0 * h.2.log10()).fold(0.0, f64::max);
    let (f_hz, spread_hz) = if hits.is_empty() { (f0, 0.0) } else { combine(&hits) };
    let fr = HumFrame {
        t_s,
        f_hz,
        snr_db,
        spread_hz,
        n_harmonics: hits.len() as u32,
        valid: snr_db >= p.frame_snr_db,
    };
    (fr, k_ok)
}
