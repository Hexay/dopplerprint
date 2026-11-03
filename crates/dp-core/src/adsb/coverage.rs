//! Coverage table: distinct aircraft, track-seconds and airborne appear/vanish counts per coarse cell ×
//! hour × altitude band. A detection-probability proxy for the silence stage (research/adsb_data.md §3):
//! many airborne vanishes at low bands mark the local reception floor.

use std::collections::HashMap;

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
    pub track_s: f32,
    /// Tracks whose first point is here and that did not start from a ground point.
    pub n_appear: u32,
    /// Tracks whose last point is here and that did not end at a ground point.
    pub n_vanish: u32,
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

/// One aircraft's contribution (`n_aircraft` = 1 per key it touches); each leg's dt goes to its start bin.
pub fn aircraft_coverage(tracks: &[CleanTrack], day0: f64) -> Vec<(CovKey, CovVal)> {
    let mut m: HashMap<CovKey, CovVal> = HashMap::new();
    let key = |p: &super::model::CleanPoint| cov_key(p.t, p.lat, p.lon, p.alt_m, day0);
    for tr in tracks {
        for (i, p) in tr.points.iter().enumerate() {
            let dt = tr.points.get(i + 1).map_or(0.0, |n| n.t - p.t);
            m.entry(key(p)).or_default().track_s += dt as f32;
        }
        if let (Some(first), Some(last)) = (tr.points.first(), tr.points.last()) {
            m.entry(key(first)).or_default().n_appear += u32::from(!tr.starts_on_ground);
            m.entry(key(last)).or_default().n_vanish += u32::from(!tr.ends_on_ground);
        }
    }
    m.into_iter().map(|(k, mut v)| (k, { v.n_aircraft = 1; v })).collect()
}

#[derive(Default)]
pub struct CoverageAgg {
    pub cells: HashMap<CovKey, CovVal>,
}

impl CoverageAgg {
    pub fn add_aircraft(&mut self, contrib: &[(CovKey, CovVal)]) {
        for (k, c) in contrib {
            let v = self.cells.entry(*k).or_default();
            v.n_aircraft += c.n_aircraft;
            v.track_s += c.track_s;
            v.n_appear += c.n_appear;
            v.n_vanish += c.n_vanish;
        }
    }

    /// Drain into key order, freeing the map as it goes.
    pub fn into_sorted(self) -> Vec<(CovKey, CovVal)> {
        let mut v: Vec<_> = self.cells.into_iter().collect();
        v.sort_unstable_by_key(|e| e.0);
        v
    }
}
