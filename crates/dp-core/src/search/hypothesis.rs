//! Per-cell association: which flights could explain each event, with one shared recording clock,
//! plus the soft silence penalty for unexplained audible flights.

use crate::physics::Cpa;

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
    /// Cap per cell to bound combinatorics in busy airspace.
    pub max_hypotheses: usize,
    /// Slant range (m) inside which an unassigned flight during the recording counts against a cell.
    pub silence_range_m: f64,
    /// Score penalty per unexplained audible flight (z units).
    pub silence_penalty: f64,
}

impl Default for AssocParams {
    fn default() -> Self {
        Self {
            clock_tol_s: 8.0,
            t_start_prior: (f64::NEG_INFINITY, f64::INFINITY),
            max_hypotheses: 64,
            silence_range_m: 3_000.0,
            silence_penalty: 3.0,
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
}

/// Enumerate hypotheses for one observer position from the CPA of each nearby track (index = track index).
pub fn associate(events: &[EventSpec], cpas: &[Option<Cpa>], duration_s: f64, p: &AssocParams) -> Vec<Hypothesis> {
    let options: Vec<Vec<(usize, f64)>> = events
        .iter()
        .map(|e| {
            cpas.iter()
                .enumerate()
                .filter_map(|(i, c)| c.map(|c| (i, c)))
                .filter(|(_, c)| in_range(c.slant_m, e.d_range) && in_range(c.ground_speed, e.v_range))
                .map(|(i, c)| (i, c.t_arrive - e.t_cpa_audio))
                .filter(|(_, clk)| in_range(*clk, p.t_start_prior))
                .collect()
        })
        .collect();
    let mut out = Vec::new();
    let mut stack = Vec::with_capacity(events.len());
    expand(&options, 0, &mut stack, p, &mut out);
    for h in &mut out {
        h.unexplained = unexplained(cpas, h, duration_s, p);
    }
    out
}

/// Slant range beyond which a CPA cannot affect `associate` (event d_range or silence range).
/// The 1e-9 relative margin keeps `sqrt` rounding at the boundary on the safe side.
pub fn cpa_reach_m(events: &[EventSpec], p: &AssocParams) -> f64 {
    events.iter().map(|e| e.d_range.1).fold(p.silence_range_m, f64::max) * (1.0 + 1e-9)
}

fn in_range(x: f64, r: (f64, f64)) -> bool {
    x >= r.0 && x <= r.1
}

fn expand(opts: &[Vec<(usize, f64)>], e: usize, stack: &mut Vec<(usize, f64)>, p: &AssocParams, out: &mut Vec<Hypothesis>) {
    if out.len() >= p.max_hypotheses {
        return;
    }
    if e == opts.len() {
        let clock = stack.iter().map(|s| s.1).sum::<f64>() / stack.len() as f64;
        out.push(Hypothesis { tracks: stack.iter().map(|s| s.0).collect(), clock, unexplained: 0 });
        return;
    }
    for &(i, clk) in &opts[e] {
        let distinct = stack.iter().all(|s| s.0 != i);
        let agrees = stack.iter().all(|s| (s.1 - clk).abs() <= p.clock_tol_s);
        if distinct && agrees {
            stack.push((i, clk));
            expand(opts, e + 1, stack, p, out);
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
