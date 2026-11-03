//! Track source for the search. In-memory for sims and regional runs; the ADS-B store's
//! segment index implements the same trait for national runs.

use crate::geo::Vec3;
use crate::track::Track;

pub trait Catalog: Sync {
    /// Tracks that pass within `radius_m` (straight-line, ECEF) of `p` at any time in [t0, t1].
    fn near(&self, p: Vec3, radius_m: f64, t0: f64, t1: f64) -> Vec<&Track>;
    fn all(&self) -> &[Track];
}

/// Brute-force catalogue over a few hundred tracks, with per-track bounding spheres.
pub struct MemCatalog {
    tracks: Vec<Track>,
    spheres: Vec<(Vec3, f64)>,
}

impl MemCatalog {
    pub fn new(tracks: Vec<Track>) -> Self {
        let spheres = tracks.iter().map(bounding_sphere).collect();
        Self { tracks, spheres }
    }
}

fn bounding_sphere(t: &Track) -> (Vec3, f64) {
    let n = t.points.len().max(1) as f64;
    let c = t.points.iter().fold(Vec3::ZERO, |a, p| a + p.pos) * (1.0 / n);
    let r = t.points.iter().map(|p| (p.pos - c).norm()).fold(0.0, f64::max);
    (c, r)
}

impl Catalog for MemCatalog {
    fn near(&self, p: Vec3, radius_m: f64, t0: f64, t1: f64) -> Vec<&Track> {
        self.tracks
            .iter()
            .zip(&self.spheres)
            .filter(|(t, (c, r))| t.t_end() >= t0 && t.t_start() <= t1 && (p - *c).norm() <= r + radius_m)
            .filter(|(t, _)| t.points.iter().any(|q| (q.pos - p).norm() <= radius_m + 2_000.0))
            .map(|(t, _)| t)
            .collect()
    }
    fn all(&self) -> &[Track] {
        &self.tracks
    }
}
