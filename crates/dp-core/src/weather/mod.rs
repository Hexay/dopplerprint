//! Archived weather → model atmosphere. A profile of temperature, humidity and wind on height levels
//! becomes a `physics::Column` at the site, so each emission is modelled with the z-mean wind vector and
//! harmonic-mean c of its own receiver→aircraft layer (research: docs/research/weather_wind_2026-11-03.md).
//! Heights: profile levels are MSL (geopotential height ≈ MSL); the column is ellipsoidal (+ geoid N).

pub mod openmeteo;
pub mod thermo;

use std::path::Path;

use anyhow::Result;

use crate::dem::geoid_undulation;
use crate::geo::{Geodetic, east_north, geodetic_to_ecef, up_vector};
use crate::physics::{Atmosphere, Column, Medium};

/// The layer from the ground up to this height carries the `Atmosphere`'s headline c and wind.
pub const REF_DEPTH_M: f64 = 1000.0;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Level {
    pub z_msl: f64,
    pub t_k: f64,
    /// Specific humidity (kg/kg) where the source gives it.
    pub q: Option<f64>,
    /// Horizontal wind toward (east, north), m/s.
    pub wind_en: [f64; 2],
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    pub levels: Vec<Level>,
    /// Model ground height (MSL).
    pub ground_msl: f64,
    pub valid_unix: f64,
    pub source: String,
}

/// Where the column stands: its local vertical and ground (ellipsoidal) height.
#[derive(Clone, Copy, Debug)]
pub struct Site {
    pub lat: f64,
    pub lon: f64,
    pub ground_ell: f64,
}

/// Profile at (lat, lon, t) from the Open-Meteo archive under `cache_dir`; `offline` reads the cache only.
pub fn archive(cache_dir: &Path, lat: f64, lon: f64, t: f64, offline: bool) -> Result<Option<Profile>> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let date = crate::adsb::manifest::civil_date(t);
    Ok(openmeteo::day(cache_dir, lat, lon, &date, offline, now)?.and_then(|v| openmeteo::profile_at(&v, t)))
}

impl Profile {
    /// (z_msl, c, wind_en) per level, ascending; levels without humidity take it interpolated in height.
    pub fn nodes(&self) -> Vec<(f64, f64, [f64; 2])> {
        let mut lv = self.levels.clone();
        lv.sort_by(|a, b| a.z_msl.total_cmp(&b.z_msl));
        let known: Vec<(f64, f64)> = lv.iter().filter_map(|l| l.q.map(|q| (l.z_msl, q))).collect();
        lv.iter().map(|l| (l.z_msl, thermo::sound_speed(l.t_k, l.q.unwrap_or_else(|| interp(&known, l.z_msl))), l.wind_en)).collect()
    }

    /// The model atmosphere at `site`: headline c/wind = the mean over the first `REF_DEPTH_M` above
    /// ground, plus the column for per-emission layers.
    pub fn atmosphere(&self, site: &Site) -> Option<Atmosphere> {
        let n = geoid_undulation(site.lat, site.lon);
        let (east, north) = east_north(site.lat, site.lon);
        let nodes: Vec<_> = self.nodes().into_iter().map(|(z, c, [e, w])| (z + n, c, east * e + north * w)).collect();
        let origin = geodetic_to_ecef(Geodetic { lat_deg: site.lat, lon_deg: site.lon, h_ell_m: site.ground_ell });
        let up = up_vector(site.lat, site.lon);
        let col = Column::new(origin, up, site.ground_ell, &nodes, (site.ground_ell, site.ground_ell + REF_DEPTH_M))?;
        let r = col.reference();
        Some(Atmosphere { c: r.c, wind: r.wind, column: Some(col) })
    }

    pub fn ground_ell(&self, lat: f64, lon: f64) -> f64 {
        self.ground_msl + geoid_undulation(lat, lon)
    }
}

fn interp(known: &[(f64, f64)], z: f64) -> f64 {
    let i = known.partition_point(|k| k.0 <= z);
    match (i.checked_sub(1).map(|j| known[j]), known.get(i).copied()) {
        (Some(a), Some(b)) => a.1 + (b.1 - a.1) * (z - a.0) / (b.0 - a.0),
        (Some(a), None) | (None, Some(a)) => a.1,
        (None, None) => 0.0,
    }
}

/// The medium `atm` gives the layer between two ellipsoidal heights (its uniform values without a column).
pub fn layer(atm: &Atmosphere, a_ell: f64, b_ell: f64) -> Medium {
    match &atm.column {
        None => Medium { c: atm.c, wind: atm.wind },
        Some(col) => {
            let (m, r) = (col.layer_mean(a_ell, b_ell), col.reference());
            Medium { c: atm.c + m.c - r.c, wind: atm.wind + m.wind - r.wind }
        }
    }
}

/// "wind 6 m/s from 250° averaged 0–900 m; c 341.2 m/s" for the layer from `site` ground to `top_ell`.
pub fn describe(atm: &Atmosphere, site: &Site, top_ell: f64) -> String {
    let m = layer(atm, site.ground_ell, top_ell);
    let (east, north) = east_north(site.lat, site.lon);
    let (s, from) = thermo::speed_from([m.wind.dot(east), m.wind.dot(north)]);
    let top = ((top_ell - site.ground_ell).max(0.0) / 10.0).round() * 10.0;
    format!("wind {s:.0} m/s from {from:03.0}° averaged 0–{top:.0} m; c {:.1} m/s", m.c)
}

#[cfg(test)]
mod tests;
