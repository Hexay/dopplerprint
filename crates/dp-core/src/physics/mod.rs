//! Forward acoustic model: emission (retarded) time, Doppler factor, CPA geometry.
//! Equations and their validation: see docs/PHYSICS.md.

pub mod cv;

use crate::geo::Vec3;
use crate::track::Track;

/// Effective propagation medium for one event. Wind is a uniform ECEF vector (m/s).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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

pub fn emission(_track: &Track, _obs: Vec3, _t_obs: f64, _atm: &Atmosphere) -> Option<Emission> {
    todo!("physics agent: Newton solve of t_obs = tau + |p(tau)-obs|/c with warm start")
}

/// Doppler factors at each observation time (NaN where the track does not cover it).
/// Multiplying by f0 gives Hz; in log-frequency this is a pure shift (see search::score).
pub fn doppler_factors(_track: &Track, _obs: Vec3, _t_obs: &[f64], _atm: &Atmosphere) -> Vec<f64> {
    todo!("physics agent")
}

pub fn cpa(_track: &Track, _obs: Vec3, _atm: &Atmosphere) -> Option<Cpa> {
    todo!("physics agent")
}
