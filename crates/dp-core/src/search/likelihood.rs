//! Turn ranked search scores into a relative likelihood surface and highest-density levels, for maps.

use std::collections::HashMap;

use super::pipeline::Candidate;
use crate::geo::albers::{self, Cell};

/// z-units per e-fold of likelihood. Scores are matched-filter z over overlapping, clipped frames, and CV
/// model error moves them by several z, so the textbook exp(z²/2) is wildly over-confident. 3 z is the
/// scale the search already treats as one unit of evidence (`AssocParams::silence_penalty`).
pub const DEFAULT_TEMPERATURE: f64 = 3.0;

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

/// Level L such that samples with likelihood ≥ L hold `mass` of the total (samples are equal-area).
pub fn hpd_level(likelihood: &[f64], mass: f64) -> f64 {
    let mut v: Vec<f64> = likelihood.iter().copied().filter(|l| *l > 0.0).collect();
    v.sort_unstable_by(|a, b| b.total_cmp(a));
    let total: f64 = v.iter().sum();
    let mut acc = 0.0;
    for l in &v {
        acc += l;
        if acc >= mass * total {
            return *l;
        }
    }
    v.last().copied().unwrap_or(0.0)
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
        assert_eq!(hpd_level(&l, 0.5), 0.5);
        assert_eq!(hpd_level(&l, 0.8), 0.3);
        assert_eq!(hpd_level(&l, 0.95), 0.1);
    }
}
