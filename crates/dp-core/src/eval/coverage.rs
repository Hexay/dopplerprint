//! Truth-referenced scores of the area-weighted posterior over refined cells at one temperature: log score,
//! HPD-split score, region areas, and which mode (by peak or by mass) holds the truth.

use crate::search::calibration::Calibration;
use crate::search::modes::{Modes, answer, core_centroid, modes};
use crate::search::surface::Surface;

/// Nominal HPD masses reported everywhere.
pub const NOMINAL: [f64; 3] = [0.5, 0.8, 0.9];
/// Core depths whose centroid error is recorded (`Coverage::core_err_m`).
pub const CORE_DEPTHS_NATS: [f64; 4] = [0.5, 1.0, 2.0, 4.0];
/// Modes lighter than this are not counted or offered as alternatives.
pub const MIN_MODE_MASS: f64 = 0.01;
/// Same-flight modes closer than this are one solution, not a mirror pair.
const MIRROR_SEP_M: f64 = 300.0;

#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub temperature: f64,
    pub floor: f64,
    pub persistence_nats: f64,
}

impl Shape {
    pub fn of(c: &Calibration) -> Self {
        Shape { temperature: c.temperature, floor: c.floor, persistence_nats: c.persistence_nats }
    }
}

/// `c`'s q̂ for each NOMINAL level at this many passes (the nominal mass where uncalibrated).
pub fn masses(c: &Calibration, n_events: usize) -> Vec<f64> {
    NOMINAL.iter().map(|&l| c.q_hat(n_events, l).unwrap_or(l)).collect()
}

impl Coverage {
    /// Truth inside each measured region.
    pub fn covered(&self) -> Vec<bool> {
        self.masses.iter().map(|&q| self.hpd_score <= q).collect()
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Coverage {
    pub temperature: f64,
    /// ln posterior density at truth (ln km⁻²).
    pub log_score: f64,
    /// Posterior mass at density ≥ the truth's: truth is inside every HPD region of mass ≥ this.
    pub hpd_score: f64,
    /// HPD masses of the regions measured (q̂ per NOMINAL level, or NOMINAL itself) and their areas (km²).
    pub masses: Vec<f64>,
    pub area_km2: Vec<f64>,
    pub n_modes: usize,
    /// Mass rank (0 = heaviest) of the mode containing the truth; None when the truth cell was unscored.
    pub truth_mode: Option<usize>,
    /// Mass rank of the mode with the highest peak.
    pub peak_mode: usize,
    /// −ln P(truth's mode) with mode probabilities from basin mass, and from softmax(peak/T).
    pub mode_loss_mass: Option<f64>,
    pub mode_loss_peak: Option<f64>,
    pub mass_choice_err_m: Option<f64>,
    pub peak_choice_err_m: Option<f64>,
    /// Error of the probability-weighted centroid of the heaviest mode, and of the peak's mode.
    pub mass_centroid_err_m: Option<f64>,
    pub peak_centroid_err_m: Option<f64>,
    /// Error of the peak's core centroid (`modes::core_centroid`) at each of CORE_DEPTHS_NATS.
    pub core_err_m: Vec<f64>,
    /// Error of the reported answer (`modes::answer`).
    pub answer_err_m: Option<f64>,
    /// Heaviest mode and its heaviest same-flight rival ≥ 300 m away, when the truth is in one of them.
    pub mirror: Option<Mirror>,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Mirror {
    /// Mass share of the heavier side within the pair.
    pub split: f64,
    pub heavier_is_truth: bool,
}

pub fn measure(s: &Surface, lat: f64, lon: f64, sh: Shape, masses: &[f64]) -> Coverage {
    let post = s.posterior(sh.temperature, sh.floor);
    let leaf = s.leaf_at(lat, lon);
    let d = post.density_at(leaf);
    let m = modes(s, &post, sh.temperature, sh.persistence_nats, post.level(0.9));
    let truth_mode = leaf.map(|i| m.label[i]);
    let peak_mode = m.by_peak().unwrap_or(0);
    let total: f64 = m.modes.iter().map(|x| x.mass).sum();
    let top = m.modes.iter().map(|x| x.peak_score).fold(f64::NEG_INFINITY, f64::max);
    let zp: f64 = m.modes.iter().map(|x| ((x.peak_score - top) / sh.temperature).exp()).sum();
    let err = |k: usize| m.modes.get(k).map(|x| super::distance_m(x.lat, x.lon, lat, lon));
    let err_c = |k: usize| m.modes.get(k).map(|x| super::distance_m(x.centroid_lat, x.centroid_lon, lat, lon));
    let err_at = |p: (f64, f64)| super::distance_m(p.0, p.1, lat, lon);
    let peak = (0..s.leaves.len()).max_by(|&a, &b| s.leaves[a].score.total_cmp(&s.leaves[b].score));
    let core_err_m = peak.map_or_else(Vec::new, |pk| {
        CORE_DEPTHS_NATS.iter().map(|&d| err_at(core_centroid(s, &post, &m, sh.temperature, pk, d))).collect()
    });
    let answer_err_m = answer(s, &post, &m, sh.temperature).map(err_at);
    Coverage {
        temperature: sh.temperature,
        log_score: d.ln(),
        hpd_score: post.mass_above(d),
        masses: masses.to_vec(),
        area_km2: masses.iter().map(|&q| post.area_above(post.level(q))).collect(),
        n_modes: m.modes.iter().filter(|x| x.mass >= MIN_MODE_MASS * total).count(),
        truth_mode,
        peak_mode,
        mode_loss_mass: truth_mode.map(|k| -(m.modes[k].mass / total).ln()),
        mode_loss_peak: truth_mode.map(|k| -(((m.modes[k].peak_score - top) / sh.temperature).exp() / zp).ln()),
        mass_choice_err_m: err(0),
        peak_choice_err_m: err(peak_mode),
        mass_centroid_err_m: err_c(0),
        peak_centroid_err_m: err_c(peak_mode),
        core_err_m,
        answer_err_m,
        mirror: mirror(&m, truth_mode),
    }
}

/// Index of the heaviest mode on the same flights as mode 0 that is a distinct solution (≥ 300 m away).
pub fn mirror_of_top(m: &Modes) -> Option<usize> {
    let a = m.modes.first()?;
    let total: f64 = m.modes.iter().map(|x| x.mass).sum();
    (1..m.modes.len()).find(|&k| {
        let b = &m.modes[k];
        b.tuple == a.tuple && b.mass >= MIN_MODE_MASS * total && super::distance_m(a.lat, a.lon, b.lat, b.lon) >= MIRROR_SEP_M
    })
}

fn mirror(m: &Modes, truth_mode: Option<usize>) -> Option<Mirror> {
    let k = mirror_of_top(m)?;
    let t = truth_mode.filter(|&t| t == 0 || t == k)?;
    let (a, b) = (m.modes[0].mass, m.modes[k].mass);
    Some(Mirror { split: a / (a + b), heavier_is_truth: t == 0 })
}
