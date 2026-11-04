//! Which scored cells of one lattice level are refined at the next. Rule and measurements:
//! docs/research/refine_keep_mode_centre_2026-11-03.md.

use super::calibration;
use super::modes::modes;
use super::pipeline::Candidate;
use super::surface::{Leaf, Surface};

/// All windows are in nats, i.e. score / T at the shipped temperature (`search::calibration`).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct KeepParams {
    /// Keep every cell within this many nats of the level's best; 0 = off.
    pub delta_nats: f64,
    /// Best cells guaranteed to every basin whose peak is within `basin_delta_nats` of the best; 0 = off.
    pub per_basin: usize,
    pub basin_delta_nats: f64,
    /// Most cells kept at one level.
    pub cap: usize,
}

impl Default for KeepParams {
    fn default() -> Self {
        KeepParams { delta_nats: 0.0, per_basin: 0, basin_delta_nats: 12.0, cap: 1000 }
    }
}

impl KeepParams {
    pub fn is_off(&self) -> bool {
        self.delta_nats <= 0.0 && self.per_basin == 0
    }
}

/// Indices into `found` (one level's scored cells, best first) to refine, ascending. With `kp` off this is
/// the top `keep_top`; otherwise the top `keep_top`, the Δ window and the basin quotas, at most `kp.cap`,
/// quota cells taking precedence over the rest under the cap.
pub fn kept(found: &[Candidate], keep_top: usize, kp: &KeepParams) -> Vec<usize> {
    if kp.is_off() {
        return (0..found.len().min(keep_top)).collect();
    }
    let Some(best) = found.first().map(|c| c.score) else { return Vec::new() };
    let t = calibration::get().temperature;
    let quota = basin_quota(found, kp, t);
    let window = |i: usize| i < keep_top || (best - found[i].score) / t <= kp.delta_nats;
    let mut out: Vec<usize> = (0..found.len()).filter(|&i| quota[i]).collect();
    out.extend((0..found.len()).filter(|&i| !quota[i] && window(i)));
    out.truncate(kp.cap);
    out.sort_unstable();
    out
}

/// Marks the best `per_basin` cells of each qualifying ToMATo basin of this level's surface.
fn basin_quota(found: &[Candidate], kp: &KeepParams, t: f64) -> Vec<bool> {
    let mut quota = vec![false; found.len()];
    if kp.per_basin == 0 {
        return quota;
    }
    let cal = calibration::get();
    let leaves = found.iter().map(|c| Leaf { cell: c.cell, score: c.score, tuple: 0 }).collect();
    let s = Surface { leaves, tuples: vec![Vec::new()], region_km2: 0.0 };
    let post = s.posterior(t, cal.floor);
    let m = modes(&s, &post, t, cal.persistence_nats, f64::INFINITY);
    let best = found[0].score;
    let open: Vec<bool> = m.modes.iter().map(|x| (best - x.peak_score) / t <= kp.basin_delta_nats).collect();
    let mut taken = vec![0usize; m.modes.len()];
    for (i, q) in quota.iter_mut().enumerate() {
        let k = m.label[i];
        if open[k] && taken[k] < kp.per_basin {
            taken[k] += 1;
            *q = true;
        }
    }
    quota
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::albers::{self, Cell};

    /// A row of cells, best first as `search` sorts them.
    fn level(scores: &[f64]) -> Vec<Candidate> {
        let mut v: Vec<Candidate> = scores
            .iter()
            .enumerate()
            .map(|(i, &score)| {
                let cell = Cell { ix: i as i32, iy: 0, step_m: 200 };
                let (lat_deg, lon_deg) = albers::inverse(cell.center());
                Candidate { cell, lat_deg, lon_deg, score, raw: score, clock: 0.0, track_ids: vec!["a".into()], events: Vec::new(), unexplained: 0, capped: false }
            })
            .collect();
        v.sort_by(|a, b| b.score.total_cmp(&a.score));
        v
    }

    fn ix(found: &[Candidate], kept: &[usize]) -> Vec<i32> {
        let mut v: Vec<i32> = kept.iter().map(|&i| found[i].cell.ix).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn off_is_the_top_k() {
        let f = level(&[1.0, 5.0, 3.0, 4.0]);
        assert_eq!(kept(&f, 2, &KeepParams::default()), [0, 1]);
        assert_eq!(kept(&f, 9, &KeepParams::default()), [0, 1, 2, 3]);
    }

    #[test]
    fn window_quota_and_cap() {
        let t = calibration::get().temperature;
        // Basin A peaks at x=1 (a ridge of near-equal cells), a deep valley, basin B peaks at x=8, 4 nats down.
        let mut row = vec![20.0, 30.0, 29.9, 29.8, 29.7, 0.0, 0.0, 0.0, 0.0, 0.0];
        row[8] = 30.0 - 4.0 * t;
        row[9] = 30.0 - 5.0 * t;
        let f = level(&row);
        let window = KeepParams { delta_nats: 1.0, ..Default::default() };
        assert_eq!(ix(&f, &kept(&f, 2, &window)), [1, 2, 3, 4]);
        let quota = KeepParams { per_basin: 1, basin_delta_nats: 6.0, ..Default::default() };
        assert_eq!(ix(&f, &kept(&f, 2, &quota)), [1, 2, 8]);
        let capped = KeepParams { delta_nats: 1.0, per_basin: 1, basin_delta_nats: 6.0, cap: 3 };
        assert_eq!(ix(&f, &kept(&f, 2, &capped)), [1, 2, 8]);
        let closed = KeepParams { per_basin: 1, basin_delta_nats: 3.0, ..Default::default() };
        assert_eq!(ix(&f, &kept(&f, 2, &closed)), [1, 2]);
    }
}
