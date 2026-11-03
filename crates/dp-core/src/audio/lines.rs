//! Persistent narrowband lines (mains harmonics, motor slip, local tones such as 58.25/25 Hz): bins whose
//! time-median power stands above the surrounding spectrum in at least 3 of 4 quarters of the recording.
//! A Doppler ridge sweeps through bins, and even a long flat asymptote holds a bin only before or after CPA;
//! a stationary line holds it throughout. Discovery masks these bins (see docs/AUDIO.md).

use rayon::prelude::*;

use super::whiten::median;

/// 3 dB in natural-log power.
const MIN_EXCESS: f32 = 0.69;
const MAD_K: f32 = 5.0;
/// Background annulus around each bin: at least this many Hz and log bins away, out to the outer bound.
const RING_IN: (f64, f64) = (1.5, 2.0);
const RING_OUT: (f64, f64) = (5.0, 8.0);
const SEGMENTS: usize = 4;
const MIN_VOTES: usize = 3;
/// Shorter recordings cannot tell a line from a slow asymptote.
pub const MIN_DURATION_S: f64 = 30.0;

fn ring(freqs: &[f64], b: usize, (hz, bins): (f64, f64)) -> usize {
    let df_bin = freqs[(b + 1).min(freqs.len() - 1)] - freqs[b.saturating_sub(1)];
    ((hz / (0.5 * df_bin).max(1e-9)).ceil() as usize).max(bins as usize)
}

/// Time-median log power per bin over frames `t0..t1`, minus its annulus median.
fn excess(power: &[f32], (t0, t1): (usize, usize), freqs: &[f64], floor: f32) -> Vec<f32> {
    let n_f = freqs.len();
    let m: Vec<f32> = (0..n_f)
        .into_par_iter()
        .map_init(Vec::new, |s, b| median(&(t0..t1).map(|t| (power[t * n_f + b] + floor).ln()).collect::<Vec<_>>(), s))
        .collect();
    let mut s = Vec::new();
    (0..n_f)
        .map(|b| {
            let (i, o) = (ring(freqs, b, RING_IN), ring(freqs, b, RING_OUT));
            let ann: Vec<f32> = (b.saturating_sub(o)..=(b + o).min(n_f - 1))
                .filter(|&k| k.abs_diff(b) >= i)
                .map(|k| m[k])
                .collect();
            m[b] - median(&ann, &mut s)
        })
        .collect()
}

/// Line bins of one excess spectrum: each peak above the robust threshold, grown while above half of it.
fn line_bins(e: &[f32]) -> Vec<bool> {
    let n_f = e.len();
    let mut s = Vec::new();
    let c = median(e, &mut s);
    let sd = 1.4826 * median(&e.iter().map(|x| (x - c).abs()).collect::<Vec<_>>(), &mut s);
    let thr = MIN_EXCESS.max(c + MAD_K * sd);
    let mut on = vec![false; n_f];
    for b in 1..n_f - 1 {
        if e[b] <= thr || e[b] < e[b - 1] || e[b] < e[b + 1] {
            continue;
        }
        let (mut lo, mut hi) = (b, b);
        while lo > 0 && e[lo - 1] > 0.5 * thr {
            lo -= 1;
        }
        while hi + 1 < n_f && e[hi + 1] > 0.5 * thr {
            hi += 1;
        }
        on[lo..=hi].iter_mut().for_each(|x| *x = true);
    }
    on
}

/// Bins to mask: line bins in ≥ `MIN_VOTES` of `SEGMENTS` time quarters, widened by `half_hz` so the
/// shortest analysis window's main lobe is covered too.
pub fn persistent_lines(power: &[f32], n_t: usize, freqs: &[f64], half_hz: f64) -> Vec<bool> {
    let n_f = freqs.len();
    let mut mask = vec![false; n_f];
    if n_f < 3 || n_t < 8 * SEGMENTS {
        return mask;
    }
    let mean = power.iter().map(|&x| x as f64).sum::<f64>() / power.len().max(1) as f64;
    let floor = (1e-10 * mean).max(1e-30) as f32;
    let mut votes = vec![0usize; n_f];
    for k in 0..SEGMENTS {
        let range = (k * n_t / SEGMENTS, (k + 1) * n_t / SEGMENTS);
        for (v, on) in votes.iter_mut().zip(line_bins(&excess(power, range, freqs, floor))) {
            *v += on as usize;
        }
    }
    for (b, _) in votes.iter().enumerate().filter(|(_, v)| **v >= MIN_VOTES) {
        let (f_lo, f_hi) = (freqs[b] - half_hz, freqs[b] + half_hz);
        for (k, m) in mask.iter_mut().enumerate() {
            *m |= (f_lo..=f_hi).contains(&freqs[k]);
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use crate::audio::{Audio, FrontEnd};

    #[test]
    fn stationary_lines_masked_doppler_sweep_not() {
        let (sr, dur) = (2000u32, 120.0);
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut uniform = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        let samples = (0..(dur * sr as f64) as usize)
            .map(|i| {
                let t = i as f64 / sr as f64;
                let sweep = 300.0 * t - 0.25 * t * t;
                let x = 0.02 * (TAU * 120.0 * t).sin() + 0.02 * (TAU * 58.25 * t).sin() + 0.05 * (TAU * sweep).sin();
                (x + 0.1 * uniform()) as f32
            })
            .collect();
        let fe = FrontEnd::default();
        let mask = fe.line_mask(&Audio { sample_rate: sr, samples });
        let at = |f: f64| mask[((f / fe.f_min).log2() * fe.bins_per_octave as f64).round() as usize];
        assert!(at(120.0) && at(58.25), "hum and motor-slip lines masked");
        assert!(!at(250.0) && !at(200.0) && !at(90.0), "the sweep (300→240 Hz) and empty bands stay");
        assert!(mask.iter().filter(|m| **m).count() < 40, "{} bins masked", mask.iter().filter(|m| **m).count());
    }
}
