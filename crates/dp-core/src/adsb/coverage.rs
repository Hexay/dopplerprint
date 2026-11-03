//! Coverage table: distinct aircraft and track-seconds per coarse cell × hour × altitude band.
//! A detection-probability proxy for the silence stage (research/adsb_data.md §3).

use std::collections::{HashMap, HashSet};

use super::model::CleanTrack;
use super::pieces::hour_of;

pub const CELL_DEG: f64 = 0.1;
pub const BAND_FT: f64 = 1000.0;
/// Top band collects everything at or above `MAX_BAND · BAND_FT`.
pub const MAX_BAND: u8 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CovKey {
    pub hour: i16,
    pub lat_cell: i16,
    pub lon_cell: i16,
    pub band: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CovVal {
    pub n_aircraft: u32,
    pub track_s: f64,
}

pub fn cov_key(t: f64, lat: f64, lon: f64, alt_m: f64, day0: f64) -> CovKey {
    let band = (alt_m / 0.3048 / BAND_FT).floor().clamp(0.0, MAX_BAND as f64) as u8;
    CovKey {
        hour: hour_of(t, day0) as i16,
        lat_cell: (lat / CELL_DEG).floor() as i16,
        lon_cell: (lon / CELL_DEG).floor() as i16,
        band,
    }
}

/// One aircraft's contribution: each key it touches counts it once; each leg's dt goes to its start bin.
pub fn aircraft_coverage(tracks: &[CleanTrack], day0: f64) -> Vec<(CovKey, f64)> {
    let mut secs: HashMap<CovKey, f64> = HashMap::new();
    for tr in tracks {
        for (i, p) in tr.points.iter().enumerate() {
            let dt = tr.points.get(i + 1).map_or(0.0, |n| n.t - p.t);
            *secs.entry(cov_key(p.t, p.lat, p.lon, p.alt_m, day0)).or_default() += dt;
        }
    }
    secs.into_iter().collect()
}

#[derive(Default)]
pub struct CoverageAgg {
    pub cells: HashMap<CovKey, CovVal>,
}

impl CoverageAgg {
    pub fn add_aircraft(&mut self, contrib: &[(CovKey, f64)]) {
        debug_assert_eq!(contrib.iter().map(|c| c.0).collect::<HashSet<_>>().len(), contrib.len());
        for (k, s) in contrib {
            let v = self.cells.entry(*k).or_default();
            v.n_aircraft += 1;
            v.track_s += s;
        }
    }

    pub fn sorted(&self) -> Vec<(CovKey, CovVal)> {
        let mut v: Vec<_> = self.cells.iter().map(|(k, v)| (*k, *v)).collect();
        v.sort_by_key(|e| e.0);
        v
    }
}
