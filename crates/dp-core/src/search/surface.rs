//! The refined cell set of a search as a piecewise-constant posterior over the region. Leaves (finest scored
//! cells) have different areas, so every mass is density × area. Posterior, see docs/research/calibration_2026-11-03.md:
//! p(x) = (1 − ε)·exp(s(x)/T)/Z + ε/A, with s the leaf score, A the region area and unscored ground at ε/A.

use std::collections::{HashMap, HashSet};

use super::pipeline::{Candidate, Region, coarse_cells};
use crate::geo::albers::{self, Cell};

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Leaf {
    pub cell: Cell,
    pub score: f64,
    /// Index into `Surface::tuples`.
    pub tuple: u32,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Surface {
    /// Scored cells none of whose children were scored; together with unscored ground they tile the region.
    pub leaves: Vec<Leaf>,
    /// Flight ids per event of each distinct best hypothesis.
    pub tuples: Vec<Vec<String>>,
    /// Region area (km²): every coarse cell, scored or not.
    pub region_km2: f64,
}

impl Surface {
    pub fn new(ranked: &[Candidate], region: &Region, coarse_step_m: u32) -> Self {
        let scored: HashSet<Cell> = ranked.iter().map(|c| c.cell).collect();
        let mut ids: HashMap<&[String], u32> = HashMap::new();
        let mut tuples = Vec::new();
        let leaves = ranked
            .iter()
            .filter(|c| c.cell.step_m < 2 || !c.cell.children().iter().any(|k| scored.contains(k)))
            .map(|c| {
                let tuple = *ids.entry(&c.track_ids).or_insert_with(|| {
                    tuples.push(c.track_ids.clone());
                    tuples.len() as u32 - 1
                });
                Leaf { cell: c.cell, score: c.score, tuple }
            })
            .collect();
        let n = coarse_cells(region, coarse_step_m).len() as f64;
        Surface { leaves, tuples, region_km2: n * km2(coarse_step_m) }
    }

    pub fn area_km2(&self, i: usize) -> f64 {
        km2(self.leaves[i].cell.step_m)
    }

    pub fn leaf_at(&self, lat: f64, lon: f64) -> Option<usize> {
        let xy = albers::forward(lat, lon);
        self.leaves.iter().position(|l| Cell::of(xy, l.cell.step_m) == l.cell)
    }

    pub fn posterior(&self, temperature: f64, floor: f64) -> Posterior {
        let scored_km2: f64 = (0..self.leaves.len()).map(|i| self.area_km2(i)).sum();
        let region = self.region_km2.max(scored_km2);
        let top = self.leaves.iter().map(|l| l.score).fold(f64::NEG_INFINITY, f64::max);
        let rel: Vec<f64> = self.leaves.iter().map(|l| ((l.score - top) / temperature).exp()).collect();
        let z: f64 = rel.iter().enumerate().map(|(i, r)| r * self.area_km2(i)).sum();
        let floor = if z > 0.0 { floor } else { 1.0 };
        let floor_density = floor / region;
        let density = rel.iter().map(|r| if z > 0.0 { (1.0 - floor) * r / z + floor_density } else { floor_density }).collect();
        let area = (0..self.leaves.len()).map(|i| self.area_km2(i)).collect();
        Posterior { density, area, floor_density, unscored_km2: region - scored_km2 }
    }
}

fn km2(step_m: u32) -> f64 {
    (step_m as f64 / 1000.0).powi(2)
}

/// Densities per km² of each leaf, and of unscored ground.
#[derive(Clone, Debug)]
pub struct Posterior {
    pub density: Vec<f64>,
    pub area: Vec<f64>,
    pub floor_density: f64,
    pub unscored_km2: f64,
}

impl Posterior {
    pub fn mass(&self, i: usize) -> f64 {
        self.density[i] * self.area[i]
    }

    pub fn density_at(&self, leaf: Option<usize>) -> f64 {
        leaf.map_or(self.floor_density, |i| self.density[i])
    }

    /// Posterior mass at density ≥ `d`, i.e. the HPD-split conformity score of a point at density `d`.
    pub fn mass_above(&self, d: f64) -> f64 {
        self.sum_above(d, |dens, area| dens * area)
    }

    pub fn area_above(&self, d: f64) -> f64 {
        self.sum_above(d, |_, area| area)
    }

    /// Density level of the highest-density region holding `mass`.
    pub fn level(&self, mass: f64) -> f64 {
        let mut dens = self.density.clone();
        let mut area = self.area.clone();
        dens.push(self.floor_density);
        area.push(self.unscored_km2);
        super::likelihood::hpd_level(&dens, &area, mass)
    }

    fn sum_above(&self, d: f64, f: impl Fn(f64, f64) -> f64) -> f64 {
        let leaves: f64 = self.density.iter().zip(&self.area).filter(|(x, _)| **x >= d).map(|(x, a)| f(*x, *a)).sum();
        leaves + if self.floor_density >= d { f(self.floor_density, self.unscored_km2) } else { 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(cell: Cell, score: f64) -> Candidate {
        let (lat_deg, lon_deg) = albers::inverse(cell.center());
        Candidate { cell, lat_deg, lon_deg, score, raw: score, clock: 0.0, track_ids: vec!["a".into()], events: Vec::new(), unexplained: 0, capped: false }
    }

    #[test]
    fn refined_parent_is_replaced_and_mass_sums_to_one() {
        let parent = Cell { ix: 10, iy: 20, step_m: 200 };
        let other = Cell { ix: 11, iy: 20, step_m: 200 };
        let kids = parent.children();
        let ranked = [cand(parent, 5.0), cand(other, 4.0), cand(kids[0], 6.0), cand(kids[1], 5.0), cand(kids[2], 5.0), cand(kids[3], 5.0)];
        let (lat, lon) = albers::inverse(parent.center());
        let s = Surface::new(&ranked, &Region { lat_deg: lat, lon_deg: lon, radius_m: 1000.0 }, 200);
        assert_eq!(s.leaves.len(), 5);
        assert_eq!(s.tuples.len(), 1);
        let p = s.posterior(1.0, 0.01);
        let total: f64 = (0..5).map(|i| p.mass(i)).sum::<f64>() + p.floor_density * p.unscored_km2;
        assert!((total - 1.0).abs() < 1e-9);
        assert!((p.mass_above(0.0) - 1.0).abs() < 1e-9);
        let best = s.leaves.iter().position(|l| l.cell == kids[0]).unwrap();
        assert_eq!(s.leaf_at(albers::inverse(kids[0].center()).0, albers::inverse(kids[0].center()).1), Some(best));
        assert!((p.mass_above(p.density[best]) - p.mass(best)).abs() < 1e-12);
        assert!(p.level(1e-6) == p.density[best]);
    }
}
