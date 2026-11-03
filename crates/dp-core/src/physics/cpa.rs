//! Closest point of approach over a Hermite track: range-rate root per bracketing segment.

use super::{Atmosphere, Cpa, Medium};
use crate::geo::{Vec3, ecef_to_geodetic, up_vector};
use crate::track::{Track, TrackPoint, hermite};

const ROOT_TOL_S: f64 = 1e-9;

pub fn closest_approach(track: &Track, obs: Vec3, atm: &Atmosphere) -> Option<Cpa> {
    let range2 = |p: &TrackPoint| (p.pos - obs).dot(p.pos - obs);
    let knot = track
        .points
        .iter()
        .min_by(|a, b| range2(a).total_cmp(&range2(b)))?;
    let mut best = (knot.t, range2(knot));
    for (a, b) in track.segments() {
        let closing_at = |p: &TrackPoint| (p.pos - obs).dot(p.vel);
        if closing_at(a) < 0.0 && closing_at(b) >= 0.0 {
            let (t, r2) = segment_min(a, b, obs);
            if r2 <= best.1 {
                best = (t, r2);
            }
        }
    }
    let s = track.state_at(best.0)?;
    let g = ecef_to_geodetic(s.pos);
    let up = up_vector(g.lat_deg, g.lon_deg);
    let horiz = s.vel - up * s.vel.dot(up);
    Some(Cpa {
        t_emit: best.0,
        t_arrive: best.0 + travel_time(obs - s.pos, &atm.medium(obs, s.pos)),
        slant_m: best.1.sqrt(),
        ground_speed: horiz.norm(),
    })
}

/// Delay s solving |q − w·s| = c·s for the chord q = obs − source (positive root).
pub fn travel_time(q: Vec3, m: &Medium) -> f64 {
    let (c2, w2, qw, q2) = (m.c * m.c, m.wind.dot(m.wind), q.dot(m.wind), q.dot(q));
    (-qw + (qw * qw + (c2 - w2) * q2).sqrt()) / (c2 - w2)
}

/// Minimum of |p − obs|² on one segment, as the root of the range rate (p − obs)·v bracketed by the
/// caller. Golden section on R² itself stalls at ~1e-5 s: R² is flat to f64 noise near its minimum.
/// Runs in segment-relative time because absolute UNIX time is too coarse.
fn segment_min(a: &TrackPoint, b: &TrackPoint, obs: Vec3) -> (f64, f64) {
    let rate = |s: f64| {
        let st = hermite(a, b, a.t + s);
        (st.pos - obs).dot(st.vel)
    };
    let (mut lo, mut hi) = (0.0, b.t - a.t);
    while hi - lo > ROOT_TOL_S {
        let mid = 0.5 * (lo + hi);
        if rate(mid) < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let s = 0.5 * (lo + hi);
    let p = hermite(a, b, a.t + s).pos - obs;
    (a.t + s, p.dot(p))
}
