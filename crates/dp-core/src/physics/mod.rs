//! Forward acoustic model: emission (retarded) time, Doppler factor, CPA geometry.
//! Equations and their validation: see docs/PHYSICS.md.

pub mod cpa;
pub mod cv;
pub mod emission;

use crate::geo::Vec3;
use crate::track::Track;

/// Effective propagation medium for one event. Wind is a uniform ECEF vector (m/s).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Atmosphere {
    pub c: f64,
    pub wind: Vec3,
}

impl Default for Atmosphere {
    fn default() -> Self {
        Self { c: 343.0, wind: Vec3::ZERO }
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
