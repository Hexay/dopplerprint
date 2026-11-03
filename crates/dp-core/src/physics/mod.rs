//! Forward acoustic model: emission (retarded) time, Doppler factor, CPA geometry.
//! Equations and their validation: see docs/PHYSICS.md.

pub mod column;
pub mod cpa;
pub mod cv;
pub mod emission;

pub use column::{Column, Medium};

use crate::geo::Vec3;
use crate::track::Track;

/// Propagation medium for a recording. `c` and `wind` (ECEF, m/s) are uniform; with a `column` each
/// emission instead sees them plus the column's height-layer departure (see `physics::column`).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Atmosphere {
    pub c: f64,
    pub wind: Vec3,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<Column>,
}

impl Default for Atmosphere {
    fn default() -> Self {
        Self::uniform(343.0, Vec3::ZERO)
    }
}

impl Atmosphere {
    pub fn uniform(c: f64, wind: Vec3) -> Self {
        Self { c, wind, column: None }
    }

    /// The uniform medium for sound from `src` to `obs`.
    pub fn medium(&self, obs: Vec3, src: Vec3) -> Medium {
        match &self.column {
            None => Medium { c: self.c, wind: self.wind },
            Some(col) => {
                let d = col.anomaly(obs, src);
                Medium { c: self.c + d.c, wind: self.wind + d.wind }
            }
        }
    }
}

/// Where the aircraft was when it emitted the sound heard at `t_obs`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emission {
    pub tau: f64,
    pub pos: Vec3,
    pub vel: Vec3,
    /// Geometric distance |obs − pos| (with wind the acoustic path is c·(t_obs − tau)).
    pub range_m: f64,
    /// Received/emitted frequency ratio (>1 approaching).
    pub factor: f64,
}

/// Closest point of approach for an observer.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cpa {
    /// Emission time of minimum slant range (UNIX s).
    pub t_emit: f64,
    /// Arrival time of that sound at the observer (the "delayed CPA").
    pub t_arrive: f64,
    pub slant_m: f64,
    pub ground_speed: f64,
}

/// Cold-start emission solve. For sequences use `doppler_factors` or `emission::emission_warm`.
pub fn emission(track: &Track, obs: Vec3, t_obs: f64, atm: &Atmosphere) -> Option<Emission> {
    let guess = emission::cold_guess(track, obs, t_obs, atm)?;
    emission::emission_iters(track, obs, t_obs, atm, guess).map(|(e, _)| e)
}

/// Doppler factors at each observation time (NaN where the track does not cover it).
/// Multiplying by f0 gives Hz; in log-frequency this is a pure shift (see search::score).
pub fn doppler_factors(track: &Track, obs: Vec3, t_obs: &[f64], atm: &Atmosphere) -> Vec<f64> {
    emission::emissions(track, obs, t_obs, atm)
        .map(|e| e.map_or(f64::NAN, |e| e.factor))
        .collect()
}

pub fn cpa(track: &Track, obs: Vec3, atm: &Atmosphere) -> Option<Cpa> {
    cpa::closest_approach(track, obs, atm)
}

/// `cpa` using precomputed chunk bounds (identical result, sublinear in track length).
pub fn cpa_bounded(track: &Track, bounds: &crate::track::TrackBounds, obs: Vec3, atm: &Atmosphere) -> Option<Cpa> {
    cpa::closest_approach_bounded(track, bounds, obs, atm)
}
