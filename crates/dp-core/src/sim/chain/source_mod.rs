//! Per-aircraft draws of `[chain.source]` realism (drift, step, AM, half-order line), merged into the
//! aircraft's own `[aircraft.source]`, and the record of what was applied.

use serde::{Deserialize, Serialize};

use super::config::SourceMod;
use crate::sim::noise::Pcg;
use crate::sim::scenario::Source;

/// Window around the CPA arrival in which a random step is placed.
const STEP_WINDOW_S: f64 = 60.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceTruth {
    pub track_id: String,
    pub f0: f64,
    pub drift_per_s: f64,
    pub drift_pct_per_min: f64,
    pub step_frac: f64,
    pub step_time_s: f64,
    pub step_ramp_s: f64,
    pub am_depth: f64,
    pub am_rate_hz: f64,
    pub half_harmonic: f64,
    pub jitter_frac: f64,
}

/// `cpa_arrival_s` and `duration_s` bound where a drawn step can land.
pub fn effective(src: &Source, m: Option<&SourceMod>, cpa_arrival_s: f64, duration_s: f64, rng: &mut Pcg) -> Source {
    let Some(m) = m else { return src.clone() };
    let mut s = src.clone();
    s.drift_per_s += m.drift_pct_per_min_sd * rng.gauss() / 6000.0;
    let lo = (cpa_arrival_s - STEP_WINDOW_S).clamp(0.0, duration_s);
    let hi = (cpa_arrival_s + STEP_WINDOW_S).clamp(0.0, duration_s);
    let (u_step, g_step, u_time) = (rng.uniform(), rng.gauss(), rng.uniform());
    if s.step_frac == 0.0 && u_step < m.step_prob {
        s.step_frac = m.step_pct_sd * g_step / 100.0;
        s.step_time_s = lo + (hi - lo) * u_time;
        s.step_ramp_s = m.step_ramp_s;
    }
    if m.am_depth > s.am_depth {
        s.am_depth = m.am_depth;
        s.am_rate_hz = m.am_rate_hz;
    }
    s.half_harmonic = s.half_harmonic.max(m.half_harmonic);
    s
}

pub fn truth(track_id: &str, s: &Source) -> SourceTruth {
    SourceTruth {
        track_id: track_id.to_string(),
        f0: s.f0,
        drift_per_s: s.drift_per_s,
        drift_pct_per_min: s.drift_per_s * 6000.0,
        step_frac: s.step_frac,
        step_time_s: s.step_time_s,
        step_ramp_s: s.step_ramp_s,
        am_depth: s.am_depth,
        am_rate_hz: s.am_rate_hz,
        half_harmonic: s.half_harmonic,
        jitter_frac: s.jitter_frac,
    }
}
