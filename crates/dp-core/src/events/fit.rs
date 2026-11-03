//! Robust (Huber IRLS) Levenberg–Marquardt fit of the constant-velocity curve, with widened pruning bounds.

use super::linalg::{invert4, solve4};
use super::stats::{lag1_autocorr, median_abs, quantile, seeds};
use super::{CvFit, Ridge};
use crate::physics::cv::CvParams;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FitParams {
    /// Bound half-width in residual-covariance sigmas.
    pub sigma_k: f64,
    /// Relative sound-speed uncertainty; v and d scale with c exactly (only v/c and d/c are observable).
    pub c_rel_unc: f64,
    /// Minimum relative half-width on v (manoeuvres, rpm drift).
    pub v_floor: f64,
    /// Minimum d range as a factor: [d/(1+x), d(1+x)]. Curved tracks bias a straight-pass d badly.
    pub d_floor: f64,
    pub f0_floor: f64,
    pub t_floor_s: f64,
    pub huber_k: f64,
}

impl Default for FitParams {
    fn default() -> Self {
        Self {
            sigma_k: 3.0,
            c_rel_unc: 0.03,
            v_floor: 0.2,
            d_floor: 1.0,
            f0_floor: 0.01,
            t_floor_s: 3.0,
            huber_k: 1.345,
        }
    }
}

/// `t_arr` is the CPA arrival time; the CPA emission time is `t_arr - d/c`.
pub(crate) fn cv_freq(f0: f64, v: f64, d: f64, t_arr: f64, c: f64, t: f64) -> f64 {
    crate::physics::cv::frequency(&crate::physics::cv::CvParams { f0, v, d_cpa: d, t_cpa_arrive: t_arr, c }, t)
}

type Th = [f64; 4];

struct Problem<'a> {
    t: &'a [f64],
    f: &'a [f64],
    c: f64,
    k: f64,
    scale_floor: f64,
}

impl Problem<'_> {
    fn model(&self, th: &Th, t: f64) -> f64 {
        cv_freq(th[0], th[1], th[2], th[3], self.c, t)
    }
    fn residuals(&self, th: &Th) -> Vec<f64> {
        self.t
            .iter()
            .zip(self.f)
            .map(|(&t, &f)| f - self.model(th, t))
            .collect()
    }
    fn scale(&self, r: &[f64]) -> f64 {
        (1.4826 * median_abs(r)).max(self.scale_floor)
    }
    fn cost(&self, r: &[f64], s: f64) -> f64 {
        let ks = self.k * s;
        r.iter()
            .map(|&x| {
                if x.abs() <= ks {
                    0.5 * x * x
                } else {
                    ks * (x.abs() - 0.5 * ks)
                }
            })
            .sum()
    }
    fn clamp(&self, th: Th) -> Th {
        [
            th[0].max(1e-3),
            th[1].clamp(0.3, 0.97 * self.c),
            th[2].clamp(1.0, 2e5),
            th[3],
        ]
    }
    fn typical(th: &Th) -> Th {
        [th[0].abs().max(1e-3), th[1].abs().max(1.0), th[2].abs().max(1.0), 1.0]
    }
    /// Weighted normal equations `(JᵀWJ, JᵀWr)` with central-difference Jacobian.
    fn normal(&self, th: &Th, r: &[f64], s: f64) -> ([[f64; 4]; 4], Th) {
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

    fn lm(&self, mut th: Th) -> Th {
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

pub fn fit_cv_with(ridge: &Ridge, c: f64, p: &FitParams) -> CvFit {
    let (t, f): (Vec<f64>, Vec<f64>) = ridge
        .t
        .iter()
        .zip(&ridge.f_hz)
        .filter(|(a, b)| a.is_finite() && b.is_finite() && **b > 0.0)
        .map(|(a, b)| (*a, *b))
        .unzip();
    if t.len() < 6 {
        return degenerate(&t, &f, c);
    }
    let fmed = quantile(&f, 0.5);
    let prob = Problem {
        t: &t,
        f: &f,
        c,
        k: p.huber_k,
        scale_floor: 1e-5 * fmed,
    };
    let fits = seeds(&t, &f, c).into_iter().map(|s| prob.lm(s));
    let best = fits
        .min_by(|a, b| median_abs(&prob.residuals(a)).total_cmp(&median_abs(&prob.residuals(b))))
        .expect("9 seeds");

    let r = prob.residuals(&best);
    let s = prob.scale(&r);
    let (a, _) = prob.normal(&best, &r, s);
    let rho = lag1_autocorr(&r).clamp(0.0, 0.98);
    let infl = (1.0 + rho) / (1.0 - rho);
    let sig: Th = match invert4(a) {
        Some(inv) => std::array::from_fn(|q| (s * s * inv[q][q] * infl).max(0.0).sqrt()),
        None => [f64::INFINITY; 4],
    };
    let inl: Vec<f64> = r.iter().copied().filter(|x| x.abs() <= 3.0 * s).collect();
    let rmse_hz = (inl.iter().map(|x| x * x).sum::<f64>() / inl.len().max(1) as f64).sqrt();

    let [f0, v, d, ta] = best;
    let k = p.sigma_k;
    let rv = (k * sig[1] / v + p.c_rel_unc).max(p.v_floor).min(10.0);
    let rd = (k * sig[2] / d + p.c_rel_unc).max(p.d_floor).min(100.0);
    let rf = (k * sig[0] / f0).max(p.f0_floor).min(0.9);
    let ht = (k * sig[3]).max(p.t_floor_s).min(1e4);
    CvFit {
        best: CvParams {
            f0,
            v,
            d_cpa: d,
            t_cpa_arrive: ta,
            c,
        },
        v_range: (
            (v * (1.0 - rv)).max(0.5),
            (v * (1.0 + rv)).min(0.97 * c * (1.0 + p.c_rel_unc)),
        ),
        d_range: (d / (1.0 + rd), d * (1.0 + rd)),
        f0_range: (f0 * (1.0 - rf), f0 * (1.0 + rf)),
        t_cpa_range: (ta - ht, ta + ht),
        rmse_hz,
    }
}

fn degenerate(t: &[f64], f: &[f64], c: f64) -> CvFit {
    let f0 = if f.is_empty() { 0.0 } else { quantile(f, 0.5) };
    let ta = t.first().copied().unwrap_or(0.0);
    let te = t.last().copied().unwrap_or(ta);
    CvFit {
        best: CvParams {
            f0,
            v: 0.0,
            d_cpa: 0.0,
            t_cpa_arrive: 0.5 * (ta + te),
            c,
        },
        v_range: (0.5, 0.97 * c),
        d_range: (1.0, 2e5),
        f0_range: (0.5 * f0, 1.5 * f0),
        t_cpa_range: (ta - 60.0, te + 60.0),
        // Finite on purpose: JSON has no infinity.
        rmse_hz: 1e9,
    }
}
