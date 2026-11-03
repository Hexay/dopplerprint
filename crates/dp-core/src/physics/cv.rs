//! Closed-form constant-velocity (straight, level, constant-speed) flyover curve.

use crate::geo::Vec3;
use crate::track::{EngineClass, Track, TrackPoint};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CvParams {
    pub f0: f64,
    pub v: f64,
    /// Slant range at CPA (m).
    pub d_cpa: f64,
    /// Arrival time of the CPA sound, in the time base of the caller (audio s or UNIX s).
    pub t_cpa_arrive: f64,
    pub c: f64,
}

/// Received frequency at time `t` for the exact retarded-time constant-velocity model (still air).
/// f = f0·c²/(c²−v²)·[1 − v²(t−τc)/√(d²(c²−v²) + c²v²(t−τc)²)], τc = t_cpa_arrive − d/c.
pub fn frequency(p: &CvParams, t: f64) -> f64 {
    let (c2, v2) = (p.c * p.c, p.v * p.v);
    let dt = (t - p.t_cpa_arrive) + p.d_cpa / p.c;
    let root = (p.d_cpa * p.d_cpa * (c2 - v2) + c2 * v2 * dt * dt).sqrt();
    p.f0 * c2 / (c2 - v2) * (1.0 - v2 * dt / root)
}

/// Straight constant-velocity path in ECEF, for tests: p(t) = obs + offset + dir·speed·(t − t_cpa_emit).
/// `offset` must be perpendicular to `dir` for `|offset|` to be the CPA slant range.
#[derive(Clone, Copy, Debug)]
pub struct StraightLine {
    pub obs: Vec3,
    pub dir: Vec3,
    pub offset: Vec3,
    pub speed: f64,
    pub t_cpa_emit: f64,
    pub half_span_s: f64,
    pub dt: f64,
}

impl StraightLine {
    pub fn track(&self, id: &str) -> Track {
        let vel = self.dir.normalized() * self.speed;
        let n = (2.0 * self.half_span_s / self.dt).round() as i64;
        let points = (0..=n)
            .map(|i| {
                let s = -self.half_span_s + i as f64 * self.dt;
                TrackPoint { t: self.t_cpa_emit + s, pos: self.obs + self.offset + vel * s, vel }
            })
            .collect();
        Track { id: id.into(), icao24: 0, type_code: None, engine: EngineClass::Unknown, points }
    }
}
