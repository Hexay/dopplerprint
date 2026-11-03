//! Closest point of approach over a Hermite track: range-rate root per bracketing segment.

use super::{Atmosphere, Cpa, Medium};
use crate::geo::{Vec3, ecef_to_geodetic, up_vector};
use crate::track::bounds::Chunk;
use crate::track::{Track, TrackPoint, hermite};

const ROOT_TOL_S: f64 = 1e-9;
/// Illinois converges superlinearly; the cap only guards degenerate segments.
const MAX_ROOT_ITERS: usize = 100;

/// Reference implementation; `closest_approach_bounded` returns the same result faster.
pub fn closest_approach(track: &Track, obs: Vec3, atm: &Atmosphere) -> Option<Cpa> {
    let knot = track.points.iter().min_by(|a, b| range2(a, obs).total_cmp(&range2(b, obs)))?;
    let mut best = (knot.t, range2(knot, obs));
    for (a, b) in track.segments() {
        refine(&mut best, a, b, obs);
    }
    finish(track, best, obs, atm)
}

/// `closest_approach` when its range is ≤ `reach2` (squared m), else `None`. `chunks` (in track order) may
/// omit any chunk whose `curve_lb2` exceeds `reach2`. Chunks are skipped when their lower bound cannot beat
/// the running best or `reach2`. Knots: best chunk first, then (lb2, index) order is preserved by a
/// lowest-index tie-break. Segments: original order with a strict skip, so every accepted update is the one
/// the reference makes; a skipped or omitted chunk's values all exceed the final minimum.
pub fn closest_approach_bounded<'b>(
    track: &Track,
    chunks: impl Iterator<Item = &'b Chunk> + Clone,
    obs: Vec3,
    atm: &Atmosphere,
    reach2: f64,
) -> Option<Cpa> {
    if chunks.clone().all(|c| c.curve_lb2(obs) > reach2) {
        return None;
    }
    let pts = &track.points;
    let seed = chunks.clone().min_by(|a, b| a.point_lb2(obs).total_cmp(&b.point_lb2(obs)))?;
    let mut knot: Option<(f64, usize)> = None;
    nearest_knot(pts, seed, obs, &mut knot);
    for c in chunks.clone() {
        if !std::ptr::eq(c, seed) && knot.is_none_or(|(b, _)| c.point_lb2(obs) <= b) {
            nearest_knot(pts, c, obs, &mut knot);
        }
    }
    let (r2, i) = knot?;
    let mut best = (pts[i].t, r2);
    for c in chunks {
        if c.curve_lb2(obs) > best.1.min(reach2) {
            continue;
        }
        for s in c.first..c.last {
            refine(&mut best, &pts[s], &pts[s + 1], obs);
        }
    }
    if best.1 > reach2 {
        return None;
    }
    finish(track, best, obs, atm)
}

/// Chunk knots `first..last` (the final chunk also owns `last`), so chunks partition the points.
fn nearest_knot(pts: &[TrackPoint], c: &Chunk, obs: Vec3, best: &mut Option<(f64, usize)>) {
    let end = if c.last + 1 == pts.len() { pts.len() } else { c.last };
    for (i, p) in pts.iter().enumerate().take(end).skip(c.first) {
        let r2 = range2(p, obs);
        if best.is_none_or(|(b, bi)| r2 < b || (r2 == b && i < bi)) {
            *best = Some((r2, i));
        }
    }
}

fn range2(p: &TrackPoint, obs: Vec3) -> f64 {
    (p.pos - obs).dot(p.pos - obs)
}

/// A segment whose range rate turns from closing to opening holds a local minimum; keep it if ≤ best.
fn refine(best: &mut (f64, f64), a: &TrackPoint, b: &TrackPoint, obs: Vec3) {
    let closing_at = |p: &TrackPoint| (p.pos - obs).dot(p.vel);
    if closing_at(a) < 0.0 && closing_at(b) >= 0.0 {
        let (t, r2) = segment_min(a, b, obs);
        if r2 <= best.1 {
            *best = (t, r2);
        }
    }
}

fn finish(track: &Track, best: (f64, f64), obs: Vec3, atm: &Atmosphere) -> Option<Cpa> {
    let s = track.state_at(best.0)?;
    let g = ecef_to_geodetic(s.pos);
    let up = up_vector(g.lat_deg, g.lon_deg);
    let horiz = s.vel - up * s.vel.dot(up);
    Some(Cpa {
        t_emit: best.0,
        t_arrive: best.0 + travel_time(obs - s.pos, &atm.medium(obs, s.pos)),
        slant_m: best.1.sqrt(),
        ground_speed: horiz.norm(),
        at_track_end: track.points.first().is_some_and(|p| best.0 <= p.t) || track.points.last().is_some_and(|p| best.0 >= p.t),
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
    // Illinois (bracketed false position): a handful of evaluations where bisection needed ~33. Near the
    // root the rate's slope is ≈ |v|², so |rate| ≤ |v|²·tol pins s to about tol, like the bracket test.
    let (mut lo, mut hi) = (0.0, b.t - a.t);
    let (mut f_lo, mut f_hi) = (rate(lo), rate(hi));
    let f_tol = a.vel.dot(a.vel).max(b.vel.dot(b.vel)) * ROOT_TOL_S;
    let mut s = 0.5 * (lo + hi);
    let mut side = 0i8;
    for _ in 0..MAX_ROOT_ITERS {
        if hi - lo <= ROOT_TOL_S {
            s = 0.5 * (lo + hi);
            break;
        }
        s = (lo * f_hi - hi * f_lo) / (f_hi - f_lo);
        if !(s > lo && s < hi) {
            s = 0.5 * (lo + hi);
        }
        let f = rate(s);
        if f.abs() <= f_tol {
            break;
        }
        // Illinois: halve the stale end's value when the same end moves twice, so both ends converge.
        if f < 0.0 {
            (lo, f_lo) = (s, f);
            if side == -1 {
                f_hi *= 0.5;
            }
            side = -1;
        } else {
            (hi, f_hi) = (s, f);
            if side == 1 {
                f_lo *= 0.5;
            }
            side = 1;
        }
    }
    let p = hermite(a, b, a.t + s).pos - obs;
    (a.t + s, p.dot(p))
}
