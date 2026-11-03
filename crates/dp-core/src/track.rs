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
    /// The returned velocity is the derivative of the interpolant, so position and velocity are consistent.
    pub fn state_at(&self, t: f64) -> Option<State> {
        let pts = &self.points;
        if pts.is_empty() || !(t >= self.t_start() && t <= self.t_end()) {
            return None;
        }
        if pts.len() == 1 {
            return Some(State { pos: pts[0].pos, vel: pts[0].vel });
        }
        let i = pts.partition_point(|p| p.t <= t).clamp(1, pts.len() - 1);
        Some(hermite(&pts[i - 1], &pts[i], t))
    }

    /// Consecutive knot pairs, one per Hermite segment.
    pub fn segments(&self) -> impl Iterator<Item = (&TrackPoint, &TrackPoint)> {
        self.points.windows(2).map(|w| (&w[0], &w[1]))
    }
}

/// Cubic Hermite between two knots, evaluated at `t` (not clamped to the interval).
pub fn hermite(a: &TrackPoint, b: &TrackPoint, t: f64) -> State {
    let h = b.t - a.t;
    let s = (t - a.t) / h;
    let (s2, s3) = (s * s, s * s * s);
    let pos = a.pos * (2.0 * s3 - 3.0 * s2 + 1.0)
        + a.vel * ((s3 - 2.0 * s2 + s) * h)
        + b.pos * (3.0 * s2 - 2.0 * s3)
        + b.vel * ((s3 - s2) * h);
    let dp = (b.pos - a.pos) * ((6.0 * s - 6.0 * s2) / h);
    let vel = dp + a.vel * (3.0 * s2 - 4.0 * s + 1.0) + b.vel * (3.0 * s2 - 2.0 * s);
    State { pos, vel }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_track() -> Track {
        let p0 = Vec3::new(-1.2e6, -4.7e6, 4.1e6);
        let v = Vec3::new(55.0, -31.0, 12.5);
        let points = [0.0, 1.0, 3.5, 10.0, 11.0]
            .iter()
            .map(|&t| TrackPoint { t: 1.7e9 + t, pos: p0 + v * t, vel: v })
            .collect();
        Track { id: "lin".into(), icao24: 0, type_code: None, engine: EngineClass::Unknown, points }
    }

    #[test]
    fn hermite_reproduces_linear_motion() {
        let tr = linear_track();
        let (p0, v) = (tr.points[0].pos, tr.points[0].vel);
        for k in 0..=1100 {
            let t = 1.7e9 + k as f64 * 0.01;
            // UNIX-second f64 has a 2.4e-7 s ulp: compare at the time actually represented.
            let dt = t - 1.7e9;
            let s = tr.state_at(t).unwrap();
            assert!((s.pos - (p0 + v * dt)).norm() < 1e-6, "pos at {dt}");
            assert!((s.vel - v).norm() < 1e-9, "vel at {dt}");
        }
    }

    #[test]
    fn outside_is_none() {
        let tr = linear_track();
        assert!(tr.state_at(1.7e9 - 1e-6).is_none());
        assert!(tr.state_at(1.7e9 + 11.0 + 1e-6).is_none());
        assert!(tr.state_at(1.7e9 + 11.0).is_some());
        assert!(tr.state_at(f64::NAN).is_none());
    }

    #[test]
    fn velocity_is_derivative_of_position() {
        let mut tr = linear_track();
        tr.points[2].vel = Vec3::new(80.0, 10.0, -5.0);
        let (t, h) = (1.7e9 + 2.2, 1e-4);
        let (t1, t0) = (t + h, t - h);
        let num = (tr.state_at(t1).unwrap().pos - tr.state_at(t0).unwrap().pos) * (1.0 / (t1 - t0));
        assert!((num - tr.state_at(t).unwrap().vel).norm() < 1e-4);
    }
}
