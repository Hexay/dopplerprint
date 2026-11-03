//! Scenario aircraft → ADS-B-like tracks, in audio-relative time (shift by `start_unix` for output).

use super::scenario::{Aircraft, Observer, Scenario, Straight, Waypoints};
use crate::geo::{Geodetic, Vec3, east_north, geodetic_to_ecef, up_vector};
use crate::track::{Track, TrackPoint};

/// Local tangent frame at the observer's ground point.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub ground: Vec3,
    pub east: Vec3,
    pub north: Vec3,
    pub up: Vec3,
    pub mic: Vec3,
    /// Microphone mirrored below the ground plane: the reflected path is the direct path to this point.
    pub mic_image: Vec3,
}

impl Frame {
    pub fn new(o: &Observer) -> Self {
        let ground = geodetic_to_ecef(Geodetic { lat_deg: o.lat_deg, lon_deg: o.lon_deg, h_ell_m: o.h_ell_m });
        let (east, north) = east_north(o.lat_deg, o.lon_deg);
        let up = up_vector(o.lat_deg, o.lon_deg);
        Self {
            ground,
            east,
            north,
            up,
            mic: ground + up * o.mic_height_m,
            mic_image: ground - up * o.mic_height_m,
        }
    }

    pub fn enu(&self, e: f64, n: f64, u: f64) -> Vec3 {
        self.east * e + self.north * n + self.up * u
    }
}

pub fn build_track(sc: &Scenario, frame: &Frame, a: &Aircraft) -> Track {
    let points = match (&a.straight, &a.waypoints) {
        (Some(s), _) => straight_points(frame, s, sc.adsb_dt_s),
        (_, Some(w)) => waypoint_points(frame, w, sc.adsb_dt_s),
        (None, None) => unreachable!("validated in Scenario::from_toml_str"),
    };
    Track { id: a.id.clone(), icao24: a.icao24, type_code: a.type_code.clone(), engine: a.engine, points }
}

fn straight_points(f: &Frame, s: &Straight, dt: f64) -> Vec<TrackPoint> {
    let (sh, ch) = s.heading_deg.to_radians().sin_cos();
    let dir = f.enu(sh, ch, 0.0);
    let right = f.enu(ch, -sh, 0.0);
    let at_cpa = f.ground + f.up * s.alt_agl_m - right * s.cpa_offset_m;
    let vel = dir * s.speed_mps;
    let n = (2.0 * s.half_span_s / dt).round() as i64;
    (0..=n)
        .map(|i| {
            let rel = -s.half_span_s + i as f64 * dt;
            TrackPoint { t: s.cpa_time_s + rel, pos: at_cpa + vel * rel, vel }
        })
        .collect()
}

/// Knots with three-point tangents (exact for quadratic motion), then resampled every `dt`.
fn waypoint_points(f: &Frame, w: &Waypoints, dt: f64) -> Vec<TrackPoint> {
    let t: Vec<f64> = w.points.iter().map(|p| p[0]).collect();
    let p: Vec<Vec3> = w.points.iter().map(|q| f.ground + f.enu(q[1], q[2], q[3])).collect();
    let last = p.len() - 1;
    let slope = |i: usize| (p[i + 1] - p[i]) * (1.0 / (t[i + 1] - t[i]));
    let knots: Vec<TrackPoint> = (0..=last)
        .map(|i| {
            let vel = if i == 0 {
                slope(0)
            } else if i == last {
                slope(last - 1)
            } else {
                let (h0, h1) = (t[i] - t[i - 1], t[i + 1] - t[i]);
                (slope(i - 1) * h1 + slope(i) * h0) * (1.0 / (h0 + h1))
            };
            TrackPoint { t: t[i], pos: p[i], vel }
        })
        .collect();
    let spline = Track {
        id: String::new(),
        icao24: 0,
        type_code: None,
        engine: crate::track::EngineClass::Unknown,
        points: knots,
    };
    let n = ((t[last] - t[0]) / dt).floor() as i64;
    (0..=n)
        .filter_map(|i| {
            let ti = t[0] + i as f64 * dt;
            spline.state_at(ti).map(|s| TrackPoint { t: ti, pos: s.pos, vel: s.vel })
        })
        .collect()
}
