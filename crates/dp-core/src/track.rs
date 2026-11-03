use crate::geo::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EngineClass {
    Piston,
    Turboprop,
    Jet,
    Helicopter,
    Unknown,
}

/// One cleaned ADS-B sample in ECEF metres / m/s, `t` in UNIX seconds (UTC).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackPoint {
    pub t: f64,
    pub pos: Vec3,
    pub vel: Vec3,
}

/// A gap-free flight segment. Tracks are split at ADS-B gaps, so `state_at` never bridges one.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub id: String,
    pub icao24: u32,
    pub type_code: Option<String>,
    pub engine: EngineClass,
    pub points: Vec<TrackPoint>,
}

/// Position and velocity at an instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub pos: Vec3,
    pub vel: Vec3,
}

impl Track {
    pub fn t_start(&self) -> f64 {
        self.points.first().map_or(f64::NAN, |p| p.t)
    }
    pub fn t_end(&self) -> f64 {
        self.points.last().map_or(f64::NAN, |p| p.t)
    }

    /// Cubic Hermite interpolation using reported velocities; `None` outside the track.
    pub fn state_at(&self, _t: f64) -> Option<State> {
        todo!("physics agent")
    }
}
