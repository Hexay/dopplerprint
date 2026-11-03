//! Retarded-time Newton solve in the moving-air frame: |obs − p(τ) − w·(t−τ)| = c·(t−τ).
//! Derivation of the residual, its derivative and the Doppler factor: see docs/PHYSICS.md §1–2.
//! With a height column, c and w are those of the layer between the observer and p(τ) (§4a).

use super::{Atmosphere, Emission, Medium};
use crate::geo::Vec3;
use crate::track::{Cursor, Track};

const MAX_ITERS: u32 = 30;

/// Convergence tolerance on the residual (s). UNIX-second f64 has a ~2.4e-7 s ulp near 1.7e9, so a
/// fixed 1e-9 would never be met; scale with |t|.
fn tolerance(t_obs: f64) -> f64 {
    1e-9_f64.max(t_obs.abs() * 4.0 * f64::EPSILON)
}

/// Air-frame chord D(τ) = obs − p(τ) − w·(t−τ), residual g = τ + |D|/c − t and dg/dτ.
struct Eval {
    g: f64,
    dg: f64,
    pos: Vec3,
    vel: Vec3,
    k_hat: Vec3,
    medium: Medium,
}

fn eval(track: &Track, obs: Vec3, t_obs: f64, atm: &Atmosphere, tau: f64, cur: &mut Cursor) -> Option<Eval> {
    let s = track.state_at_from(tau, cur)?;
    let m = atm.medium(obs, s.pos);
    let d = obs - s.pos - m.wind * (t_obs - tau);
    let r = d.norm();
    let k_hat = d * (1.0 / r);
    Some(Eval {
        g: (tau - t_obs) + r / m.c,
        dg: 1.0 + k_hat.dot(m.wind - s.vel) / m.c,
        pos: s.pos,
        vel: s.vel,
        k_hat,
        medium: m,
    })
}

/// Received/emitted frequency ratio for ground velocity `vel`, air-frame wave normal `k_hat`.
pub fn factor(vel: Vec3, k_hat: Vec3, m: &Medium) -> f64 {
    let c_eff = m.c + m.wind.dot(k_hat);
    c_eff / (c_eff - vel.dot(k_hat))
}

/// Straight-line delay estimate from the position at `t_obs` (clamped into the track).
pub fn cold_guess(track: &Track, obs: Vec3, t_obs: f64, atm: &Atmosphere) -> Option<f64> {
    let p = track.state_at(t_obs.clamp(track.t_start(), track.t_end()))?.pos;
    Some(t_obs - (obs - p).norm() / atm.medium(obs, p).c)
}

/// Newton from `tau_guess`; returns the emission and the number of Newton updates taken.
/// `None` when the emission time falls outside the track (exact test: g is strictly increasing).
pub fn emission_iters(
    track: &Track,
    obs: Vec3,
    t_obs: f64,
    atm: &Atmosphere,
    tau_guess: f64,
) -> Option<(Emission, u32)> {
    emission_from(track, obs, t_obs, atm, tau_guess, &mut Cursor::default())
}

/// `emission_iters` with a knot-search hint carried across calls (identical result).
fn emission_from(
    track: &Track,
    obs: Vec3,
    t_obs: f64,
    atm: &Atmosphere,
    tau_guess: f64,
    cur: &mut Cursor,
) -> Option<(Emission, u32)> {
    let (lo, hi) = (track.t_start(), track.t_end());
    let tol = tolerance(t_obs);
    let mut tau = tau_guess.clamp(lo, hi);
    for it in 0..=MAX_ITERS {
        let e = eval(track, obs, t_obs, atm, tau, cur)?;
        if e.g.abs() < tol {
            let em = Emission {
                tau,
                pos: e.pos,
                vel: e.vel,
                range_m: (obs - e.pos).norm(),
                factor: factor(e.vel, e.k_hat, &e.medium),
            };
            return Some((em, it));
        }
        if (tau <= lo && e.g > 0.0) || (tau >= hi && e.g < 0.0) {
            return None;
        }
        tau = (tau - e.g / e.dg).clamp(lo, hi);
    }
    None
}

pub fn emission_warm(track: &Track, obs: Vec3, t_obs: f64, atm: &Atmosphere, tau_guess: f64) -> Option<Emission> {
    emission_iters(track, obs, t_obs, atm, tau_guess).map(|(e, _)| e)
}

/// Warm-started emissions for a sequence of observation times (best when `t_obs` is sorted).
/// The next guess extrapolates τ with dτ/dt = factor.
pub fn emissions<'a>(
    track: &'a Track,
    obs: Vec3,
    t_obs: &'a [f64],
    atm: &'a Atmosphere,
) -> impl Iterator<Item = Option<Emission>> + 'a {
    let mut prev: Option<(f64, Emission)> = None;
    let mut cur = Cursor::default();
    t_obs.iter().map(move |&t| {
        let guess = match prev {
            Some((tp, e)) => Some(e.tau + (t - tp) * e.factor),
            None => cold_guess(track, obs, t, atm),
        };
        let out = guess.and_then(|g| emission_from(track, obs, t, atm, g, &mut cur).map(|(e, _)| e));
        prev = out.map(|e| (t, e));
        out
    })
}
