//! Harmonic comb on the log axis: harmonic h of a base at bin u sits at u + bpo·log2(h).
//! Statistic = max(single bin, full comb sum/√n). The full sum (not a best-subset) is deliberate: a subset max
//! lets the tracker hop between unrelated lines that happen to sit on different teeth of a low base.

use rayon::prelude::*;

use crate::audio::whiten::median;

pub(super) struct Comb {
    pub shifts: Vec<usize>,
    n_f: usize,
    center: Vec<f32>,
    scale: Vec<f32>,
}

pub(super) fn harmonic_shift(bpo: u32, ratio: f64) -> f64 {
    bpo as f64 * ratio.log2()
}

impl Comb {
    pub fn new(bpo: u32, harmonics: u32, n_f: usize) -> Self {
        let shifts = (1..=harmonics.max(1))
            .map(|h| harmonic_shift(bpo, h as f64).round() as usize)
            .collect();
        Self {
            shifts,
            n_f,
            center: vec![0.0; n_f],
            scale: vec![1.0; n_f],
        }
    }

    fn raw_row(&self, e: &[f32], out: &mut [f32]) {
        let n = self.n_f;
        for (u, o) in out.iter_mut().enumerate() {
            let (mut acc, mut k) = (e[u], 1);
            for &s in &self.shifts[1..] {
                let p = u + s;
                if p >= n {
                    break;
                }
                // ±1 bin pooling above h=1 absorbs the rounding of log2(h).
                acc += e[p - 1].max(e[p]).max(e[(p + 1).min(n - 1)]);
                k += 1;
            }
            *o = e[u].max(acc / (k as f32).sqrt());
        }
    }

    /// Per-bin standardisation: the subset-max null depends on how many harmonics fit below f_max.
    pub fn image(&mut self, e: &[f32], n_t: usize) -> Vec<f32> {
        let n_f = self.n_f;
        let mut c = vec![0f32; n_t * n_f];
        c.par_chunks_mut(n_f)
            .zip(e.par_chunks(n_f))
            .for_each(|(o, row)| self.raw_row(row, o));
        let step = (n_t / 4000).max(1);
        let stats: Vec<(f32, f32)> = (0..n_f)
            .into_par_iter()
            .map(|u| {
                let mut s = Vec::new();
                let col: Vec<f32> = (0..n_t).step_by(step).map(|t| c[t * n_f + u]).collect();
                let m = median(&col, &mut s);
                let dev: Vec<f32> = col.iter().map(|x| (x - m).abs()).collect();
                (m, (1.4826 * median(&dev, &mut s)).max(1e-3))
            })
            .collect();
        (self.center, self.scale) = stats.into_iter().unzip();
        c.par_chunks_mut(n_f).for_each(|row| self.normalise(row));
        c
    }

    pub fn recompute_row(&self, e: &[f32], out: &mut [f32]) {
        self.raw_row(e, out);
        self.normalise(out);
    }

    fn normalise(&self, row: &mut [f32]) {
        for ((x, m), s) in row.iter_mut().zip(&self.center).zip(&self.scale) {
            *x = ((*x - m) / s).clamp(-2.0, 6.0);
        }
    }
}
