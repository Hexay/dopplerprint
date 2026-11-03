//! Per-cell association: which flights could explain each event, with one shared recording clock,
//! plus the soft silence penalty for unexplained audible flights. Gate costs: see `soft`.

mod soft;

pub use soft::{Admit, SoftGates};

use crate::physics::Cpa;

/// Enumeration budget per cell, as a multiple of `max_hypotheses`, before keeping the cheapest.
const ENUM_FACTOR: usize = 8;

/// What the search needs to know about one audio event (from `events::Event` or oracle truth).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EventSpec {
    /// Audio window (s) to score.
    pub window: (f64, f64),
    /// Audio-relative arrival time of the CPA sound (s).
    pub t_cpa_audio: f64,
    pub d_range: (f64, f64),
    pub v_range: (f64, f64),
    pub f0_range: (f64, f64),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AssocParams {
    /// Max disagreement (s) between per-event implied clocks for one hypothesis.
    pub clock_tol_s: f64,
    /// Recording-start prior (UNIX s).
    pub t_start_prior: (f64, f64),
    /// Cap per cell to bound combinatorics in busy airspace; the cheapest (by gate cost) are kept.
    pub max_hypotheses: usize,
    /// Slant range (m) inside which an unassigned flight during the recording counts against a cell.
    pub silence_range_m: f64,
    /// Score penalty per unexplained audible flight (z units).
    pub silence_penalty: f64,
    pub soft: SoftGates,
}

impl Default for AssocParams {
    fn default() -> Self {
        Self {
            clock_tol_s: 8.0,
            t_start_prior: (f64::NEG_INFINITY, f64::INFINITY),
            max_hypotheses: 64,
            silence_range_m: 3_000.0,
            silence_penalty: 3.0,
            soft: SoftGates::default(),
        }
    }
}

/// One way the recording could be explained from a cell: track index per event and the clock.
#[derive(Clone, Debug)]
pub struct Hypothesis {
    pub tracks: Vec<usize>,
    /// UNIX = audio + clock.
    pub clock: f64,
    pub unexplained: usize,
    /// Summed soft-gate cost of the pairings (z units).
    pub gate_cost: f64,
}

impl Hypothesis {
    /// Everything subtracted from the joint score: gate cost plus the silence penalty.
    pub fn penalty(&self, p: &AssocParams) -> f64 {
        self.gate_cost + p.silence_penalty * self.unexplained as f64
    }
}

/// Enumerate hypotheses for one observer position from the CPA of each nearby track (index = track index).
/// `half_diag_m` is the scored cell's half-diagonal (0 for a point), used as gate slack (see `soft`).
pub fn associate(
    events: &[EventSpec],
    cpas: &[Option<Cpa>],
    duration_s: f64,
    p: &AssocParams,
    half_diag_m: f64,
) -> Vec<Hypothesis> {
    let slack = p.soft.slack_m(half_diag_m);
    let options: Vec<Vec<(usize, Admit)>> = events
        .iter()
        .map(|e| {
            let mut o: Vec<(usize, Admit)> = cpas
                .iter()
                .enumerate()
                .filter_map(|(i, c)| c.as_ref().and_then(|c| p.soft.admit(e, c, p.t_start_prior, slack)).map(|a| (i, a)))
                .collect();
            o.sort_by(|a, b| a.1.cost_z.total_cmp(&b.1.cost_z));
            o
        })
        .collect();
    let mut out = Vec::new();
    let mut stack = Vec::with_capacity(events.len());
    let budget = p.max_hypotheses.saturating_mul(ENUM_FACTOR);
    expand(&options, &mut stack, p, budget, &mut out);
    out.sort_by(|a, b| a.gate_cost.total_cmp(&b.gate_cost));
    out.truncate(p.max_hypotheses);
    for h in &mut out {
        h.unexplained = unexplained(cpas, h, duration_s, p);
    }
    out
}

/// Slant range beyond which a CPA cannot affect `associate` at this `half_diag_m`: the outer soft-gate
/// bound or the silence range. The 1e-9 relative margin keeps `sqrt` rounding at the boundary safe.
pub fn cpa_reach_m(events: &[EventSpec], p: &AssocParams, half_diag_m: f64) -> f64 {
    let slack = p.soft.slack_m(half_diag_m);
    events.iter().map(|e| p.soft.max_slant_m(e, slack)).fold(p.silence_range_m, f64::max) * (1.0 + 1e-9)
}

/// Recording-start window wide enough for every clock `associate` can admit at this `half_diag_m`.
pub fn clock_window(p: &AssocParams, half_diag_m: f64) -> (f64, f64) {
    let pad = p.soft.max_clock_pad_s(p.soft.slack_m(half_diag_m));
    (p.t_start_prior.0 - pad, p.t_start_prior.1 + pad)
}

fn expand(opts: &[Vec<(usize, Admit)>], stack: &mut Vec<(usize, Admit)>, p: &AssocParams, budget: usize, out: &mut Vec<Hypothesis>) {
    if out.len() >= budget {
        return;
    }
    if stack.len() == opts.len() {
        let clock = stack.iter().map(|s| s.1.clock).sum::<f64>() / stack.len() as f64;
        let gate_cost = stack.iter().map(|s| s.1.cost_z).sum();
        out.push(Hypothesis { tracks: stack.iter().map(|s| s.0).collect(), clock, unexplained: 0, gate_cost });
        return;
    }
    for &(i, a) in &opts[stack.len()] {
        let distinct = stack.iter().all(|s| s.0 != i);
        let agrees = stack.iter().all(|s| (s.1.clock - a.clock).abs() <= p.clock_tol_s + s.1.tau_s + a.tau_s);
        if distinct && agrees {
            stack.push((i, a));
            expand(opts, stack, p, budget, out);
            stack.pop();
        }
    }
}

fn unexplained(cpas: &[Option<Cpa>], h: &Hypothesis, duration_s: f64, p: &AssocParams) -> usize {
    let (a, b) = (h.clock, h.clock + duration_s);
    cpas.iter()
        .enumerate()
        .filter(|(i, _)| !h.tracks.contains(i))
        .filter_map(|(_, c)| *c)
        .filter(|c| c.t_arrive >= a && c.t_arrive <= b && c.slant_m <= p.silence_range_m)
        .count()
}
