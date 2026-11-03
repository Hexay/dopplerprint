//! The calibrated part of the locate report: the 90% region (conformal mass q̂ at the fitted temperature)
//! and its modes with their probability. Calibration and its limits: docs/research/calibration_2026-11-03.md.

use std::collections::HashSet;

use super::report::{offset_to, osm_url};
use crate::eval::coverage::{MIN_MODE_MASS, mirror_of_top};
use crate::eval::distance_m;
use crate::geo::albers::{self, Cell, Xy};
use crate::output::features::{RegionKind, RegionRow};
use crate::search::calibration;
use crate::search::modes::{Mode, answer, modes};
use crate::search::pipeline::{Candidate, Region};
use crate::search::surface::Surface;

pub const NOMINAL: f64 = 0.9;
const MAX_MODES: usize = 4;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Posterior {
    /// Nominal coverage of `area_km2` (0.9) and the HPD mass reported for it.
    pub nominal: f64,
    pub hpd_mass: f64,
    /// False when no calibration stratum covers this many passes: the region is then the raw 90% HPD.
    pub calibrated: bool,
    /// Where q̂ came from: "real", "sim" or "real+sim" recordings.
    pub source: String,
    pub area_km2: f64,
    /// Reported location: probability-weighted centre of the peak's core (`modes::answer`); the peak cell
    /// stays in the report's `location`.
    pub answer: Answer,
    /// Heaviest first; `[0]` holds the best spot.
    pub modes: Vec<ModeReport>,
    /// False when every pass is one aircraft: one straight pass cannot tell which side of its track the
    /// microphone was on, so the mirror's share is model error, not evidence.
    pub side_resolved: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ModeReport {
    /// Peak cell of the mode.
    pub lat: f64,
    pub lon: f64,
    /// Probability-weighted centre of the mode.
    pub centroid_lat: f64,
    pub centroid_lon: f64,
    pub probability: f64,
    pub region_km2: f64,
    pub distance_km: f64,
    pub direction: String,
    pub same_aircraft: bool,
    /// The mirror (same flights, other side) of the heaviest mode.
    pub mirror: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Answer {
    pub lat: f64,
    pub lon: f64,
    pub osm_url: String,
    /// Distance from the peak cell (m).
    pub from_peak_m: f64,
}

/// Leaves inside the region, for the cell layer.
pub struct Inside(pub HashSet<Cell>);

pub fn posterior(ranked: &[Candidate], region: &Region, coarse_step_m: u32, n_events: usize) -> Option<(Posterior, Inside, Vec<RegionRow>)> {
    let cal = calibration::get();
    let s = Surface::new(ranked, region, coarse_step_m);
    if s.leaves.is_empty() {
        return None;
    }
    let q = cal.q_hat(n_events, NOMINAL);
    let hpd_mass = q.unwrap_or(NOMINAL);
    let post = s.posterior(cal.temperature, cal.floor);
    let level = post.level(hpd_mass);
    let m = modes(&s, &post, cal.temperature, cal.persistence_nats, level);
    let total: f64 = m.modes.iter().map(|x| x.mass).sum();
    let mirror = mirror_of_top(&m);
    let top = &m.modes[0];
    let flights: HashSet<&String> = s.tuples[top.tuple as usize].iter().collect();
    let shown: Vec<(usize, &Mode)> =
        m.modes.iter().enumerate().filter(|(k, x)| x.mass >= MIN_MODE_MASS * total || Some(*k) == mirror).take(MAX_MODES).collect();
    let reports = shown
        .iter()
        .map(|&(k, x)| {
            let (d, dir) = offset_to(top.lat, top.lon, x.lat, x.lon);
            ModeReport {
                lat: x.lat,
                lon: x.lon,
                centroid_lat: x.centroid_lat,
                centroid_lon: x.centroid_lon,
                probability: x.mass / total,
                region_km2: x.region_km2,
                distance_km: d / 1000.0,
                direction: dir.into(),
                same_aircraft: x.tuple == top.tuple,
                mirror: Some(k) == mirror,
            }
        })
        .collect();
    let inside: HashSet<Cell> = s.leaves.iter().zip(&post.density).filter(|(_, d)| **d >= level).map(|(l, _)| l.cell).collect();
    let rows = shown
        .iter()
        .filter(|(_, x)| x.region_km2 > 0.0)
        .enumerate()
        .map(|(i, &(k, x))| {
            let cells: Vec<Cell> = s.leaves.iter().enumerate().filter(|(j, l)| m.label[*j] == k && inside.contains(&l.cell)).map(|(_, l)| l.cell).collect();
            RegionRow { rank: i as u32 + 1, kind: RegionKind::Credible90, mass: x.region_mass, area_km2: x.region_km2, ring: hull(&cells) }
        })
        .collect();
    let (lat, lon) = answer(&s, &post, &m, cal.temperature)?;
    let answer = Answer { lat, lon, osm_url: osm_url(lat, lon), from_peak_m: distance_m(lat, lon, ranked[0].lat_deg, ranked[0].lon_deg) };
    let p = Posterior {
        nominal: NOMINAL,
        hpd_mass,
        calibrated: q.is_some(),
        source: cal.stratum(n_events).map_or_else(String::new, |s| s.source.clone()),
        area_km2: post.area_above(level),
        answer,
        modes: reports,
        side_resolved: flights.len() > 1,
    };
    Some((p, Inside(inside), rows))
}

/// Convex hull of the cells' corners as a closed (lat, lon) ring: an outline for display; the region
/// itself is the cell set (`in_cr90`) and `area_km2` is its exact area.
fn hull(cells: &[Cell]) -> Vec<[f64; 2]> {
    let mut pts: Vec<(f64, f64)> = cells
        .iter()
        .flat_map(|c| {
            let (s, x, y) = (c.step_m as f64, c.ix as f64 * c.step_m as f64, c.iy as f64 * c.step_m as f64);
            [(x, y), (x + s, y), (x, y + s), (x + s, y + s)]
        })
        .collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    pts.dedup();
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut h: Vec<(f64, f64)> = Vec::new();
    for &p in &pts {
        while h.len() >= 2 && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
            h.pop();
        }
        h.push(p);
    }
    let lower = h.len() + 1;
    for &p in pts.iter().rev().skip(1) {
        while h.len() >= lower && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
            h.pop();
        }
        h.push(p);
    }
    h.iter()
        .map(|&(x, y)| {
            let (lat, lon) = albers::inverse(Xy { x, y });
            [lat, lon]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_of_a_block_is_its_square() {
        let cells: Vec<Cell> = (0..2).flat_map(|i| (0..2).map(move |j| Cell { ix: i, iy: j, step_m: 100 })).collect();
        let ring = hull(&cells);
        assert_eq!(ring.len(), 5);
        assert_eq!(ring[0], ring[4]);
    }
}
