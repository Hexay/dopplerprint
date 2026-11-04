//! Robust fit of the constant-velocity curve (`cvlm`), with widened pruning bounds.

use super::cvlm::{Problem, Th, sigmas};
use super::stats::{median_abs, quantile, seeds};
use super::window::{Spread, window_spread};
use super::{CvFit, Ridge};
use crate::physics::cv::CvParams;

pub(crate) use super::cvlm::cv_freq;

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
    /// Minimum half-width of the κ = v²/d range in ln units (drift biases d, see matching_mathematical_methods.md).
    pub kappa_floor_ln: f64,
    pub f0_floor: f64,
    pub t_floor_s: f64,
    pub huber_k: f64,
    /// v, d and t_cpa bounds are at least this multiple of their sub-window refit spread (`window`); 0 disables.
    /// 2 = plateau of gate recall on the Livermore dev split (fix_event_select_fitbias.md).
    pub window_k: f64,
    /// Class prior σ of linear f0 drift (1/s; 1e-4 = 0.6 %/min), widening bounds via `Problem::covariance`.
    pub drift_sigma_per_s: f64,
}

impl Default for FitParams {
    fn default() -> Self {
        Self {
            sigma_k: 3.0,
            c_rel_unc: 0.03,
            v_floor: 0.2,
            d_floor: 1.0,
            kappa_floor_ln: 0.3,
            f0_floor: 0.01,
            t_floor_s: 3.0,
            huber_k: 1.345,
            window_k: 2.0,
            drift_sigma_per_s: 5e-5,
        }
    }
}

impl FitParams {
    /// For callers that use only `best` and `rmse_hz`: skips the sub-window refits.
    pub(super) fn point_only(&self) -> Self {
        Self { window_k: 0.0, ..*self }
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
    let prob = Problem { t: &t, f: &f, c, k: p.huber_k, scale_floor: 1e-5 * fmed, drift_sigma: p.drift_sigma_per_s };
    let fits = seeds(&t, &f, c).into_iter().map(|s| prob.lm(s));
    let best = fits
        .min_by(|a, b| median_abs(&prob.residuals(a)).total_cmp(&median_abs(&prob.residuals(b))))
        .expect("9 seeds");
    let spread = if p.window_k > 0.0 { window_spread(&prob, &best, p.sigma_k) } else { Spread::default() };
    bounds(&prob, best, spread, p)
}

/// Covariance bounds (autocorrelation-inflated), floored and widened by the window spread.
fn bounds(prob: &Problem, best: Th, spread: Spread, p: &FitParams) -> CvFit {
    let c = prob.c;
    let r = prob.residuals(&best);
    let s = prob.scale(&r);
    let [f0, v, d, ta] = best;
    // ln κ = 2 ln v − ln d: the CPA curvature, far better determined than v or d alone.
    let (sig, sig_ln_kappa) = sigmas(prob.covariance(&best), &best);
    let inl: Vec<f64> = r.iter().copied().filter(|x| x.abs() <= 3.0 * s).collect();
    let rmse_hz = (inl.iter().map(|x| x * x).sum::<f64>() / inl.len().max(1) as f64).sqrt();

    let (k, wk) = (p.sigma_k, p.window_k);
    let rv = (k * sig[1] / v + p.c_rel_unc).max(p.v_floor).max(wk * spread.v).min(10.0);
    let rd = (k * sig[2] / d + p.c_rel_unc).max(p.d_floor).max((wk * spread.ln_d).exp() - 1.0).min(100.0);
    let hk = (k * sig_ln_kappa + p.c_rel_unc).max(p.kappa_floor_ln).max(wk * spread.ln_kappa).min(10.0);
    let rf = (k * sig[0] / f0).max(p.f0_floor).min(0.9);
    let ht = (k * sig[3]).max(p.t_floor_s).max(wk * spread.t).min(1e4);
    let kappa = v * v / d;
    CvFit {
        best: CvParams { f0, v, d_cpa: d, t_cpa_arrive: ta, c },
        v_range: ((v * (1.0 - rv)).max(0.5), (v * (1.0 + rv)).min(0.97 * c * (1.0 + p.c_rel_unc))),
        d_range: (d / (1.0 + rd), d * (1.0 + rd)),
        kappa_range: Some((kappa * (-hk).exp(), kappa * hk.exp())),
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
        best: CvParams { f0, v: 0.0, d_cpa: 0.0, t_cpa_arrive: 0.5 * (ta + te), c },
        v_range: (0.5, 0.97 * c),
        d_range: (1.0, 2e5),
        kappa_range: None,
        f0_range: (0.5 * f0, 1.5 * f0),
        t_cpa_range: (ta - 60.0, te + 60.0),
        // Finite on purpose: JSON has no infinity.
        rmse_hz: 1e9,
    }
}
