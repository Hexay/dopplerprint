//! Turn ranked search scores into a relative likelihood surface and highest-density levels, for maps.

use std::collections::HashMap;

use super::pipeline::Candidate;
use crate::geo::albers::{self, Cell};

/// z-units per e-fold of likelihood, fitted by log score (`search::calibration`). Scores are matched-filter
/// z over overlapping, clipped frames and CV model error moves them by several z, so exp(z²/2) is wildly
/// over-confident; the fit replaced the hand-set 3 z on 2026-11-03.
pub fn default_temperature() -> f64 {
    super::calibration::get().temperature
}

/// Score of the finest scored cell containing each (lat, lon); NaN where no level scored a cell there.
pub fn finest_score_at(ranked: &[Candidate], points: &[(f64, f64)]) -> Vec<f64> {
    let by_cell: HashMap<Cell, f64> = ranked.iter().map(|c| (c.cell, c.score)).collect();
    let mut steps: Vec<u32> = ranked.iter().map(|c| c.cell.step_m).collect();
    steps.sort_unstable();
    steps.dedup();
    points
        .iter()
        .map(|&(lat, lon)| {
            let xy = albers::forward(lat, lon);
            steps.iter().find_map(|&s| by_cell.get(&Cell::of(xy, s)).copied()).unwrap_or(f64::NAN)
        })
        .collect()
}

/// exp((s − s_max)/T), so the best cell is 1; NaN scores (unscored) give 0.
pub fn relative_likelihood(scores: &[f64], temperature: f64) -> Vec<f64> {
    let top = scores.iter().copied().filter(|s| s.is_finite()).fold(f64::NEG_INFINITY, f64::max);
    scores.iter().map(|&s| if s.is_finite() { ((s - top) / temperature).exp() } else { 0.0 }).collect()
}

/// Density level L such that samples with density ≥ L hold `mass` of the total, sample i covering
/// `area[i]` (any unit; `&[]` = equal areas). Densities, not per-sample masses: a 200 m cell and a 25 m
/// cell at the same density rank together.
pub fn hpd_level(density: &[f64], area: &[f64], mass: f64) -> f64 {
    assert!(area.is_empty() || area.len() == density.len(), "one area per sample");
    let a = |i: usize| if area.is_empty() { 1.0 } else { area[i] };
    let mut idx: Vec<usize> = (0..density.len()).filter(|&i| density[i] > 0.0 && a(i) > 0.0).collect();
    idx.sort_unstable_by(|&x, &y| density[y].total_cmp(&density[x]));
    let total: f64 = idx.iter().map(|&i| density[i] * a(i)).sum();
    let mut acc = 0.0;
    for &i in &idx {
        acc += density[i] * a(i);
        if acc >= mass * total {
            return density[i];
        }
    }
    idx.last().map_or(0.0, |&i| density[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(cell: Cell, score: f64) -> Candidate {
        let (lat_deg, lon_deg) = albers::inverse(cell.center());
        Candidate {
            cell,
            lat_deg,
            lon_deg,
            score,
            raw: score,
            clock: 0.0,
            track_ids: Vec::new(),
            events: Vec::new(),
            unexplained: 0,
        }
    }

    #[test]
    fn finest_cell_wins_and_unscored_is_nan() {
        let parent = Cell { ix: 10, iy: 20, step_m: 200 };
        let child = parent.children()[3];
        let ranked = [cand(parent, 5.0), cand(child, 9.0)];
        let (clat, clon) = albers::inverse(child.center());
        let other = parent.children()[0];
        let (olat, olon) = albers::inverse(other.center());
        let (flat, flon) = albers::inverse(Cell { ix: 50, iy: 50, step_m: 200 }.center());
        let s = finest_score_at(&ranked, &[(clat, clon), (olat, olon), (flat, flon)]);
        assert_eq!(&s[..2], &[9.0, 5.0]);
        assert!(s[2].is_nan());
    }

    #[test]
    fn likelihood_is_relative_to_best() {
        let l = relative_likelihood(&[10.0, 7.0, f64::NAN], 3.0);
        assert!((l[0] - 1.0).abs() < 1e-12);
        assert!((l[1] - (-1.0f64).exp()).abs() < 1e-12);
        assert_eq!(l[2], 0.0);
    }

    #[test]
    fn hpd_level_covers_requested_mass() {
        let l = [0.5, 0.3, 0.1, 0.1];
        assert_eq!(hpd_level(&l, &[], 0.5), 0.5);
        assert_eq!(hpd_level(&l, &[], 0.8), 0.3);
        assert_eq!(hpd_level(&l, &[], 0.95), 0.1);
    }

    #[test]
    fn hpd_level_weights_by_area() {
        // A coarse cell at lower density outweighs a small peak: 0.9 of the mass needs both.
        let (d, a) = ([4.0, 1.0], [1.0, 16.0]);
        assert_eq!(hpd_level(&d, &a, 0.15), 4.0);
        assert_eq!(hpd_level(&d, &a, 0.5), 1.0);
    }
}
