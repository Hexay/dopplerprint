//! Robust Levenberg–Marquardt on the constant-velocity curve: Huber IRLS, numeric Jacobian.

use super::linalg::solve4;
use super::stats::median_abs;

pub(super) type Th = [f64; 4];

/// `t_arr` is the CPA arrival time; the CPA emission time is `t_arr - d/c`.
pub(crate) fn cv_freq(f0: f64, v: f64, d: f64, t_arr: f64, c: f64, t: f64) -> f64 {
    crate::physics::cv::frequency(&crate::physics::cv::CvParams { f0, v, d_cpa: d, t_cpa_arrive: t_arr, c }, t)
}

pub(super) struct Problem<'a> {
    pub t: &'a [f64],
    pub f: &'a [f64],
    pub c: f64,
    /// Huber threshold in σ.
    pub k: f64,
    pub scale_floor: f64,
}

impl Problem<'_> {
    fn model(&self, th: &Th, t: f64) -> f64 {
        cv_freq(th[0], th[1], th[2], th[3], self.c, t)
    }
    pub fn residuals(&self, th: &Th) -> Vec<f64> {
        self.t.iter().zip(self.f).map(|(&t, &f)| f - self.model(th, t)).collect()
    }
    pub fn scale(&self, r: &[f64]) -> f64 {
        (1.4826 * median_abs(r)).max(self.scale_floor)
    }
    fn cost(&self, r: &[f64], s: f64) -> f64 {
        let ks = self.k * s;
        r.iter().map(|&x| if x.abs() <= ks { 0.5 * x * x } else { ks * (x.abs() - 0.5 * ks) }).sum()
    }
    fn clamp(&self, th: Th) -> Th {
        [th[0].max(1e-3), th[1].clamp(0.3, 0.97 * self.c), th[2].clamp(1.0, 2e5), th[3]]
    }
    fn typical(th: &Th) -> Th {
        [th[0].abs().max(1e-3), th[1].abs().max(1.0), th[2].abs().max(1.0), 1.0]
    }
    /// Weighted normal equations `(JᵀWJ, JᵀWr)` with central-difference Jacobian.
    pub fn normal(&self, th: &Th, r: &[f64], s: f64) -> ([[f64; 4]; 4], Th) {
        let typ = Self::typical(th);
        let ks = self.k * s;
        let mut a = [[0.0; 4]; 4];
        let mut g = [0.0; 4];
        for (i, &t) in self.t.iter().enumerate() {
            let mut j = [0.0; 4];
            for (p, jp) in j.iter_mut().enumerate() {
                let h = 1e-6 * typ[p];
                let (mut hi, mut lo) = (*th, *th);
                hi[p] += h;
                lo[p] -= h;
                *jp = (self.model(&hi, t) - self.model(&lo, t)) / (2.0 * h);
            }
            let w = if r[i].abs() <= ks { 1.0 } else { ks / r[i].abs() };
            for p in 0..4 {
                g[p] += w * j[p] * r[i];
                for q in 0..4 {
                    a[p][q] += w * j[p] * j[q];
                }
            }
        }
        (a, g)
    }

    pub fn lm(&self, mut th: Th) -> Th {
        let mut lambda = 1e-3;
        for _ in 0..200 {
            let r = self.residuals(&th);
            let s = self.scale(&r);
            let cost = self.cost(&r, s);
            let (a, g) = self.normal(&th, &r, s);
            let mut improved = false;
            while lambda < 1e12 {
                let mut m = a;
                (0..4).for_each(|p| m[p][p] += lambda * a[p][p].max(1e-12));
                let Some(delta) = solve4(m, g) else {
                    lambda *= 4.0;
                    continue;
                };
                let cand = self.clamp([th[0] + delta[0], th[1] + delta[1], th[2] + delta[2], th[3] + delta[3]]);
                if self.cost(&self.residuals(&cand), s) < cost {
                    let typ = Self::typical(&th);
                    let step = (0..4).map(|p| ((cand[p] - th[p]) / typ[p]).abs()).fold(0.0, f64::max);
                    th = cand;
                    lambda = (lambda / 3.0).max(1e-12);
                    improved = step > 1e-10;
                    break;
                }
                lambda *= 4.0;
            }
            if !improved {
                break;
            }
        }
        th
    }
}
