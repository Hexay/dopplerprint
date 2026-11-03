//! Trace → gap-split, jump-free, airborne `CleanTrack`s with ENU velocity.

use super::altitude::select_altitudes;
use super::model::{CleanPoint, CleanTrack, ground_dist_m};
use super::trace::{FLAG_NEW_LEG, RawTrace};
use crate::track::EngineClass;

const KT_TO_MPS: f64 = 0.514_444;
const FPM_TO_MPS: f64 = 0.005_08;

#[derive(Clone, Debug, serde::Serialize)]
pub struct CleanParams {
    /// Split a track where consecutive accepted points are further apart in time than this.
    pub max_gap_s: f64,
    /// Ground-speed ceiling for the implied speed between points (≈ 680 kt).
    pub max_speed_mps: f64,
    /// Fixed position slack added to `max_speed_mps · dt` (MLAT/TIS-B noise).
    pub pos_tol_m: f64,
    pub max_vrate_mps: f64,
    pub alt_tol_m: f64,
    /// After this many consecutive rejections the anchor, not the newcomers, is assumed wrong.
    pub max_consecutive_rejects: usize,
}

impl Default for CleanParams {
    fn default() -> Self {
        Self {
            max_gap_s: 60.0,
            max_speed_mps: 350.0,
            pos_tol_m: 150.0,
            max_vrate_mps: 60.0,
            alt_tol_m: 60.0,
            max_consecutive_rejects: 3,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct CleanStats {
    pub points_in: u64,
    pub ground: u64,
    pub no_alt: u64,
    pub non_monotonic: u64,
    pub jumps: u64,
    pub short_tracks: u64,
    pub points_out: u64,
    pub tracks: u64,
}

impl CleanStats {
    pub fn add(&mut self, o: &CleanStats) {
        self.points_in += o.points_in;
        self.ground += o.ground;
        self.no_alt += o.no_alt;
        self.non_monotonic += o.non_monotonic;
        self.jumps += o.jumps;
        self.short_tracks += o.short_tracks;
        self.points_out += o.points_out;
        self.tracks += o.tracks;
    }
}

struct Splitter<'a> {
    p: &'a CleanParams,
    cur: Vec<CleanPoint>,
    pending: Vec<CleanPoint>,
    done: Vec<Vec<CleanPoint>>,
    stats: CleanStats,
}

impl Splitter<'_> {
    fn close(&mut self) {
        self.pending.clear();
        let run = std::mem::take(&mut self.cur);
        if run.len() >= 2 {
            self.done.push(run);
        } else if !run.is_empty() {
            self.stats.short_tracks += 1;
        }
    }

    fn is_jump(&self, a: &CleanPoint, b: &CleanPoint) -> bool {
        let dt = b.t - a.t;
        let horiz = ground_dist_m(a.lat, a.lon, b.lat, b.lon);
        horiz > self.p.max_speed_mps * dt + self.p.pos_tol_m
            || (b.alt_m - a.alt_m).abs() > self.p.max_vrate_mps * dt + self.p.alt_tol_m
    }

    fn push(&mut self, pt: CleanPoint) {
        let Some(last) = self.cur.last().copied() else {
            self.cur.push(pt);
            return;
        };
        if pt.t <= last.t {
            self.stats.non_monotonic += 1;
            return;
        }
        if pt.t - last.t > self.p.max_gap_s {
            self.close();
            self.cur.push(pt);
            return;
        }
        if !self.is_jump(&last, &pt) {
            self.pending.clear();
            self.cur.push(pt);
            return;
        }
        self.stats.jumps += 1;
        self.pending.push(pt);
        if self.pending.len() >= self.p.max_consecutive_rejects {
            let replay = std::mem::take(&mut self.pending);
            self.close();
            for q in replay {
                self.push(q);
            }
        }
    }
}

/// Clean one aircraft's trace. Returned tracks are numbered from 0 in time order.
pub fn clean_trace(
    raw: &RawTrace,
    icao: u32,
    type_code: Option<String>,
    engine: EngineClass,
    p: &CleanParams,
) -> (Vec<CleanTrack>, CleanStats) {
    let alts = select_altitudes(&raw.trace);
    let mut s = Splitter { p, cur: Vec::new(), pending: Vec::new(), done: Vec::new(), stats: CleanStats::default() };
    s.stats.points_in = raw.trace.len() as u64;
    for (rp, alt) in raw.trace.iter().zip(alts) {
        if matches!(rp.alt, super::trace::Alt::Ground) {
            s.stats.ground += 1;
            s.close();
            continue;
        }
        let Some((alt_m, alt_source)) = alt else {
            s.stats.no_alt += 1;
            continue;
        };
        if rp.flags & FLAG_NEW_LEG != 0 {
            s.close();
        }
        let (ve, vn) = match (rp.gs_kt, rp.track_deg) {
            (Some(gs), Some(trk)) => {
                let (st, ct) = trk.to_radians().sin_cos();
                (gs * KT_TO_MPS * st, gs * KT_TO_MPS * ct)
            }
            _ => (f64::NAN, f64::NAN),
        };
        let vu = rp.geom_rate_fpm.or(rp.baro_rate_fpm).map_or(f64::NAN, |r| r * FPM_TO_MPS);
        s.push(CleanPoint {
            t: raw.timestamp + rp.dt,
            lat: rp.lat,
            lon: rp.lon,
            alt_m,
            alt_source,
            ve,
            vn,
            vu,
            src: rp.src,
            flags: rp.flags,
        });
    }
    s.close();
    let mut stats = s.stats;
    let tracks: Vec<CleanTrack> = s
        .done
        .into_iter()
        .enumerate()
        .map(|(i, mut pts)| {
            fill_missing_velocity(&mut pts);
            CleanTrack { icao, track_no: i as u32, type_code: type_code.clone(), engine, points: pts }
        })
        .collect();
    stats.tracks = tracks.len() as u64;
    stats.points_out = tracks.iter().map(|t| t.points.len() as u64).sum();
    (tracks, stats)
}

/// Replace NaN velocity components by position finite differences inside the track (never across gaps).
fn fill_missing_velocity(pts: &mut [CleanPoint]) {
    let n = pts.len();
    for i in 0..n {
        let p = pts[i];
        if !(p.ve.is_nan() || p.vn.is_nan() || p.vu.is_nan()) {
            continue;
        }
        let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(n - 1)]);
        let dt = b.t - a.t;
        let m_per_deg = 111_320.0;
        let fd = [
            (b.lon - a.lon) * m_per_deg * p.lat.to_radians().cos() / dt,
            (b.lat - a.lat) * m_per_deg / dt,
            (b.alt_m - a.alt_m) / dt,
        ];
        let q = &mut pts[i];
        if q.ve.is_nan() || q.vn.is_nan() {
            q.ve = fd[0];
            q.vn = fd[1];
        }
        if q.vu.is_nan() {
            q.vu = fd[2];
        }
    }
}
