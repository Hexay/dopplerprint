//! Trace statistics and the sample-rate / clock-drift check.

use super::grid::{COMMON_RATES, FINE_REL, rate_ratios};
use super::spectrum::{median, robust_sigma};
use super::{HumFrame, HumParams, RateCheck, RateStatus};

pub(super) struct TraceStats {
    pub mean_hz: f64,
    pub se_hz: f64,
    pub wander_hz: f64,
    pub drift_hz: f64,
}

/// None with fewer than three valid frames.
pub(super) fn trace_stats(frames: &[HumFrame], p: &HumParams) -> Option<TraceStats> {
    let v: Vec<&HumFrame> = frames.iter().filter(|f| f.valid).collect();
    if v.len() < 3 {
        return None;
    }
    let n = v.len() as f64;
    // Median-centred robust mean: a few frames biased by a crossing aircraft tone must not move it.
    let mut fs: Vec<f64> = v.iter().map(|f| f.f_hz).collect();
    let med = median(&mut fs);
    let sig = robust_sigma(&fs).max(1e-6);
    let inl: Vec<&&HumFrame> = v.iter().filter(|f| (f.f_hz - med).abs() <= 4.0 * sig).collect();
    let m = inl.len() as f64;
    let mean_hz = inl.iter().map(|f| f.f_hz).sum::<f64>() / m;
    let var = inl.iter().map(|f| (f.f_hz - mean_hz).powi(2)).sum::<f64>() / (m - 1.0).max(1.0);
    let n_eff = (n * p.hop_s / p.frame_s).max(1.0);
    let tm = inl.iter().map(|f| f.t_s).sum::<f64>() / m;
    let sxx: f64 = inl.iter().map(|f| (f.t_s - tm).powi(2)).sum();
    let sxy: f64 = inl.iter().map(|f| (f.t_s - tm) * (f.f_hz - mean_hz)).sum();
    let span = v.last().map_or(0.0, |l| l.t_s) - v[0].t_s;
    Some(TraceStats {
        mean_hz,
        se_hz: (var / n_eff).sqrt(),
        wander_hz: var.sqrt(),
        drift_hz: if sxx > 0.0 { sxy / sxx * span } else { 0.0 },
    })
}

pub(super) fn check(st: &TraceStats, nominal: f64, labelled_sr: u32, p: &HumParams) -> RateCheck {
    let scale = nominal / st.mean_hz;
    let rel_sigma = st.se_hz.hypot(p.grid_sigma_hz * st.mean_hz / nominal) / st.mean_hz;
    let tol = p.rate_tol.max(3.0 * rel_sigma);
    let implied = labelled_sr as f64 * scale;
    let dev = st.mean_hz / nominal - 1.0;
    let near = |a: f64, b: f64| (a / b - 1.0).abs() <= tol;
    let status = if dev.abs() <= tol {
        RateStatus::Ok
    } else if rate_ratios().iter().any(|&r| r != 1.0 && (scale * r - 1.0).abs() <= tol.max(FINE_REL)) {
        RateStatus::Mislabelled
    } else {
        RateStatus::ClockOffset
    };
    let likely_sr = (status == RateStatus::Mislabelled)
        .then(|| COMMON_RATES.iter().copied().find(|&r| r != labelled_sr && near(implied, r as f64)))
        .flatten();
    RateCheck {
        status,
        labelled_sr,
        deviation_ppm: dev * 1e6,
        implied_sr: implied,
        implied_sr_sigma: implied * rel_sigma,
        likely_sr,
        drift_hz: st.drift_hz,
        drift_flag: st.drift_hz.abs() > p.drift_tol_hz,
    }
}
