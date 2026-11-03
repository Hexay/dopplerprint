//! One (event, flight) pair → lattice cells where the observer could stand, each with the implied clock.
//! At CPA the observer lies perpendicular to the ground track at horizontal range √(d² − H²), on either
//! side (the mirror); sliding the CPA along the track trades against the clock offset.

use super::GlobalParams;
use crate::dem::geoid_undulation;
use crate::geo::{albers, east_north, ecef_to_geodetic, radii_of_curvature};
use crate::search::hypothesis::EventSpec;
use crate::track::Track;

#[derive(Clone, Copy, Debug)]
pub(super) struct Vote {
    pub ix: i32,
    pub iy: i32,
    /// floor((clock − prior.0) / clock_bin_s).
    pub bin: i32,
    pub event: u16,
    pub track: u32,
    pub w: f32,
}

impl Vote {
    pub fn key(&self) -> (i32, i32, u16, i32) {
        (self.ix, self.iy, self.event, self.bin)
    }
}

/// CPA emission times compatible with the clock prior: t_emit = t_cpa_audio + clock − d/c.
pub(super) fn emission_window(e: &EventSpec, prior: (f64, f64), c: f64) -> (f64, f64) {
    (e.t_cpa_audio + prior.0 - e.d_range.1 / c, e.t_cpa_audio + prior.1 - e.d_range.0 / c)
}

/// Log-normal plausibility of `x` within a fit range whose geometric centre is the best estimate.
fn range_weight(x: f64, r: (f64, f64)) -> f64 {
    let sigma = ((r.1 / r.0.max(1e-9)).ln() / 4.0).max(0.05);
    let z = (x / (r.0 * r.1).sqrt()).ln() / sigma;
    (-0.5 * z * z).exp()
}

/// Ground state of the aircraft at one emission time.
struct Sample {
    t: f64,
    lat: f64,
    lon: f64,
    /// Height above the ground (min, max) over the assumed ground-height range.
    agl: (f64, f64),
    /// Unit left-perpendicular of the ground track, (east, north).
    perp: (f64, f64),
    gs: f64,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn track_votes(
    e: &EventSpec,
    ei: u16,
    tr: &Track,
    ti: u32,
    prior: (f64, f64),
    cell: f64,
    p: &GlobalParams,
    out: &mut Vec<Vote>,
) {
    let (lo, hi) = emission_window(e, prior, p.c);
    let (t0, t1) = (lo.max(tr.t_start()), hi.min(tr.t_end()));
    if t1.is_nan() || t0.is_nan() || t1 < t0 {
        return;
    }
    let start = out.len();
    let mut t = t0;
    while t <= t1 {
        let Some(s) = sample(tr, t, p) else { break };
        if s.gs >= e.v_range.0 && s.gs <= e.v_range.1 {
            ground_votes(e, ei, ti, &s, prior, cell, p, out);
        }
        t += (0.7 * cell / s.gs.max(1.0)).min(p.max_dt_s);
    }
    let mine = &mut out[start..];
    mine.sort_unstable_by(|a, b| a.key().cmp(&b.key()).then(b.w.total_cmp(&a.w)));
    let mut keep = start;
    for i in start..out.len() {
        if i == start || out[i].key() != out[keep - 1].key() {
            out[keep] = out[i];
            keep += 1;
        }
    }
    out.truncate(keep);
}

fn sample(tr: &Track, t: f64, p: &GlobalParams) -> Option<Sample> {
    let s = tr.state_at(t)?;
    let g = ecef_to_geodetic(s.pos);
    let (east, north) = east_north(g.lat_deg, g.lon_deg);
    let (ve, vn) = (s.vel.dot(east), s.vel.dot(north));
    let gs = ve.hypot(vn);
    let ground = geoid_undulation(g.lat_deg, g.lon_deg) + p.mic_height_m;
    let agl = ((g.h_ell_m - ground - p.ground_msl_m.1).max(0.0), (g.h_ell_m - ground - p.ground_msl_m.0).max(0.0));
    let perp = if gs > 0.0 { (-vn / gs, ve / gs) } else { (0.0, 0.0) };
    Some(Sample { t, lat: g.lat_deg, lon: g.lon_deg, agl, perp, gs })
}

#[allow(clippy::too_many_arguments)]
fn ground_votes(e: &EventSpec, ei: u16, ti: u32, s: &Sample, prior: (f64, f64), cell: f64, p: &GlobalParams, out: &mut Vec<Vote>) {
    let (d_min, d_max) = e.d_range;
    if d_max <= s.agl.0 {
        return;
    }
    let l_max = (d_max * d_max - s.agl.0 * s.agl.0).sqrt();
    let l_min = (d_min * d_min - s.agl.1 * s.agl.1).max(0.0).sqrt();
    let (m_rad, n_rad) = radii_of_curvature(s.lat);
    let cos_lat = s.lat.to_radians().cos();
    let wv = range_weight(s.gs, e.v_range);
    let step = 0.7 * cell;
    let n = ((l_max - l_min) / step).ceil().max(0.0) as usize;
    for k in 0..=n {
        let l = (l_min + k as f64 * step).min(l_max);
        let d_lo = l.hypot(s.agl.0).max(d_min);
        let d_hi = l.hypot(s.agl.1).min(d_max);
        if d_lo > d_hi {
            continue;
        }
        let clk_lo = (s.t + d_lo / p.c - e.t_cpa_audio).max(prior.0);
        let clk_hi = (s.t + d_hi / p.c - e.t_cpa_audio).min(prior.1);
        if clk_lo > clk_hi {
            continue;
        }
        let w = (range_weight(0.5 * (d_lo + d_hi), e.d_range) * wv) as f32;
        let (b0, b1) = (bin(clk_lo, prior, p), bin(clk_hi, prior, p));
        for side in [-1.0, 1.0] {
            let (de, dn) = (side * l * s.perp.0, side * l * s.perp.1);
            let lat = s.lat + (dn / m_rad).to_degrees();
            let lon = s.lon + (de / (n_rad * cos_lat)).to_degrees();
            let xy = albers::forward(lat, lon);
            let (ix, iy) = ((xy.x / cell).floor() as i32, (xy.y / cell).floor() as i32);
            for b in b0..=b1 {
                out.push(Vote { ix, iy, bin: b, event: ei, track: ti, w });
            }
        }
    }
}

pub(super) fn bin(clock: f64, prior: (f64, f64), p: &GlobalParams) -> i32 {
    ((clock - prior.0) / p.clock_bin_s).floor() as i32
}

pub(super) fn bin_start(b: i32, prior: (f64, f64), p: &GlobalParams) -> f64 {
    prior.0 + b as f64 * p.clock_bin_s
}
