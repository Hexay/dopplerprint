//! Per-sample propagation: fixed-point retarded time, then the source phase evaluated at τ.
//! Deliberately independent of physics::emission (different solver, no Doppler factor anywhere).

use rayon::prelude::*;

use super::source::{Tone, absorption_db_per_km};
use crate::geo::Vec3;
use crate::track::Track;

const CHUNK: usize = 1 << 13;
const TAPER_S: f64 = 2.0;
const FP_TOL_S: f64 = 1e-11;
const FP_MAX_ITERS: usize = 200;

/// One acoustic path (direct, or ground-reflected via the mirrored microphone) from one aircraft.
pub struct Path<'a> {
    pub track: &'a Track,
    pub tone: &'a Tone,
    pub obs: Vec3,
    /// Source amplitude times reflection coefficient.
    pub gain: f64,
    pub c: f64,
    pub wind: Vec3,
}

/// Position with linear extrapolation past the ends, so the fixed point always has a solution.
fn position(track: &Track, tau: f64) -> Vec3 {
    let (first, last) = (&track.points[0], &track.points[track.points.len() - 1]);
    if tau < first.t {
        first.pos + first.vel * (tau - first.t)
    } else if tau > last.t {
        last.pos + last.vel * (tau - last.t)
    } else {
        track.state_at(tau).map_or(last.pos, |s| s.pos)
    }
}

/// Air-frame path length |obs − p(τ) − w·(t−τ)| at emission time τ.
fn path_len(p: &Path, t: f64, tau: f64) -> f64 {
    (p.obs - position(p.track, tau) - p.wind * (t - tau)).norm()
}

/// τ = t − L(τ)/c by plain fixed-point iteration (contraction factor ≤ Mach).
pub fn retarded_time(p: &Path, t: f64, guess: f64) -> (f64, f64) {
    let mut tau = guess;
    for _ in 0..FP_MAX_ITERS {
        let next = t - path_len(p, t, tau) / p.c;
        let done = (next - tau).abs() < FP_TOL_S;
        tau = next;
        if done {
            break;
        }
    }
    (tau, p.c * (t - tau))
}

/// Raised-cosine fade over the first/last `TAPER_S` of the track (avoids clicks at track ends).
fn envelope(track: &Track, tau: f64) -> f64 {
    let edge = (tau - track.t_start()).min(track.t_end() - tau);
    if edge <= 0.0 {
        0.0
    } else if edge >= TAPER_S {
        1.0
    } else {
        0.5 - 0.5 * (std::f64::consts::PI * edge / TAPER_S).cos()
    }
}

/// Adds this path's harmonic series to `out` (sample i at audio time i/fs).
pub fn render_path(p: &Path, fs: f64, out: &mut [f64]) {
    let nepers: Vec<f64> = (1..=p.tone.harmonics.len())
        .map(|k| absorption_db_per_km(k as f64 * p.tone.f0_at(0.0)) * std::f64::consts::LN_10 / 20_000.0)
        .collect();
    out.par_chunks_mut(CHUNK).enumerate().for_each(|(ci, chunk)| {
        let mut hist: Option<(f64, f64)> = None;
        for (j, y) in chunk.iter_mut().enumerate() {
            let t = (ci * CHUNK + j) as f64 / fs;
            let guess = match hist {
                Some((t1, t0)) => 2.0 * t1 - t0,
                None => t - (p.obs - position(p.track, t)).norm() / p.c,
            };
            let (tau, r) = retarded_time(p, t, guess);
            hist = Some((tau, hist.map_or(tau, |h| h.0)));
            let env = envelope(p.track, tau);
            if env == 0.0 {
                continue;
            }
            let phase = p.tone.phase(tau);
            let spread = p.gain * env * 1000.0 / r.max(1.0);
            *y += p
                .tone
                .harmonics
                .iter()
                .zip(&nepers)
                .enumerate()
                .map(|(k, (&(amp, th), &np))| amp * (-np * r).exp() * ((k + 1) as f64 * phase + th).sin())
                .sum::<f64>()
                * spread;
        }
    });
}
