//! Linear source-f0 drift in the scorer: f_src(τ) = f0·(1 + β·(τ − τ_ref)), τ the emission time and τ_ref the
//! window's middle emission. Each β of `ScoreParams::drift_grid` adds bpo·log2(1 + β·(τ − τ_ref)) to an event's
//! Doppler offsets and the sweep keeps the best β (localization_verification.md, f0 drift: 1 %/min costs
//! 150–900 m cross-track without it). Zsum CPU sweep only: with a grid set, `Scorer::score` skips the GPU.

use super::fixed;
use super::hypothesis::GATE_Z_PER_NAT;

/// z charged per β: the Gaussian prior's 0.5·(β/σ)² nats. Without it, steady passes trade geometry for
/// drift (β = 0 cases moved 0.35–2.7 km cross-track in the `drift_bias` harness).
pub fn penalties(grid: &[f64], sigma: f64) -> Vec<f64> {
    grid.iter().map(|b| GATE_Z_PER_NAT * 0.5 * (b / sigma).powi(2)).collect()
}

/// Offset deltas per β (outer) and offsets index (inner) for `bpo` bins per octave; empty when `grid` is.
pub fn deltas(bpo: f64, taus: &[f64], grid: &[f64]) -> Vec<Vec<i64>> {
    if grid.is_empty() {
        return Vec::new();
    }
    let valid: Vec<f64> = taus.iter().copied().filter(|t| t.is_finite()).collect();
    let tau_ref = valid.get(valid.len() / 2).copied().unwrap_or(0.0);
    grid.iter()
        .map(|&b| {
            taus.iter()
                .map(|&t| if t.is_finite() { fixed::pos(bpo * (1.0 + b * (t - tau_ref)).max(1e-3).log2()) } else { 0 })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_at_reference_and_signed_by_beta() {
        let taus = [0.0, 10.0, 20.0, f64::NAN, 30.0, 40.0];
        let d = deltas(48.0, &taus, &[1e-3, -1e-3]);
        assert_eq!(d[0][2], 0, "τ_ref is the middle finite emission (20 s)");
        assert!(d[0][5] > 0 && d[1][5] < 0 && d[0][0] < 0);
        assert_eq!(d[0][3], 0);
        assert!(deltas(48.0, &taus, &[]).is_empty());
    }
}
