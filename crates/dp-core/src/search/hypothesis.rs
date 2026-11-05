//! Per-cell association: which flights could explain each event, with one shared recording clock,
//! plus the silence term for unexplained audible flights (`silence`). Gate costs: see `soft`.
//!
//! With `unexplained_cost` = Some(λ_u) an event may also stay unexplained (an unknown source or clutter):
//! it is not scored and adds λ_u to the penalty. A hypothesis needs ≥ 1 explained event and takes its
//! clock from the explained events only. The all-unexplained null scores −N·λ_u (silence 0: some cell in
//! the region hears no flight), and the search abstains when no cell beats it (`SearchResult::abstains`).
//! Partial hypotheses are enumerated and capped separately, after the full ones, which stay as without λ_u.

mod soft;

pub use soft::{Admit, GATE_Z_PER_NAT, SoftGates};

use super::score::EventScore;
use super::silence::{SilenceMode, SilenceParams};
use crate::physics::Cpa;

/// Enumeration budget per cell, as a multiple of `max_hypotheses`, before keeping the cheapest.
const ENUM_FACTOR: usize = 8;

/// `Candidate::track_ids` entry of an unexplained event.
pub const UNEXPLAINED_ID: &str = "";

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
    /// Half-width (s) of the CPA-time band (`CvFit::t_cpa_range`); 0 = exact truth label (see `soft`).
    #[serde(default)]
    pub t_cpa_halfwidth_s: f64,
    /// CPA curvature bounds κ = v²/d from the fit (`CvFit::kappa_range`); None = no constraint.
    #[serde(default)]
    pub kappa_range: Option<(f64, f64)>,
}

impl EventSpec {
    /// `d_range` narrowed to the slant ranges consistent with κ for a source moving at `v`.
    /// Empty (lo > hi) when the speed cannot produce this curve.
    pub fn d_range_at(&self, v: f64) -> (f64, f64) {
        match self.kappa_range {
            Some((k_lo, k_hi)) => (self.d_range.0.max(v * v / k_hi), self.d_range.1.min(v * v / k_lo)),
            None => self.d_range,
        }
    }

    /// A truth label: exact CPA time and a prior-box d/v, so every association gate is hard.
    pub fn is_exact(&self) -> bool {
        self.t_cpa_halfwidth_s <= 0.0
    }
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
    /// Flat silence penalty per unexplained audible flight (score units).
    pub silence_penalty: f64,
    pub silence: SilenceParams,
    /// λ_u (score units) per unexplained event; None = every event takes a flight. Negative = a credit.
    pub unexplained_cost: Option<f64>,
    pub soft: SoftGates,
}

impl Default for AssocParams {
    fn default() -> Self {
        Self {
            clock_tol_s: 8.0,
            t_start_prior: (f64::NEG_INFINITY, f64::INFINITY),
            max_hypotheses: 64,
            silence_range_m: 6_000.0,
            silence_penalty: 3.0,
            silence: SilenceParams::default(),
            unexplained_cost: None,
            soft: SoftGates::default(),
        }
    }
}

impl AssocParams {
    /// Score of the all-unexplained null for `n_events`, when events may stay unexplained.
    pub fn null_score(&self, n_events: usize) -> Option<f64> {
        self.unexplained_cost.map(|l| -(n_events as f64) * l)
    }
}

/// One way the recording could be explained from a cell: track index per event and the clock.
#[derive(Clone, Debug)]
pub struct Hypothesis {
    /// Track index per event; None = the event is unexplained.
    pub tracks: Vec<Option<usize>>,
    /// UNIX = audio + clock.
    pub clock: f64,
    /// Audible flights no event is assigned to.
    pub unexplained: usize,
    /// Summed soft-gate cost of the pairings (z units).
    pub gate_cost: f64,
    /// Likelihood-mode silence cost (nats).
    pub silence_nats: f64,
}

impl Hypothesis {
    /// Everything subtracted from the joint score; `per_nat` = score units per nat (`silence::score_per_nat`).
    pub fn penalty(&self, p: &AssocParams, per_nat: f64) -> f64 {
        let silence = match p.silence.mode {
            SilenceMode::Flat => p.silence_penalty * self.unexplained as f64,
            SilenceMode::Likelihood => per_nat * self.silence_nats,
        };
        let base = self.gate_cost + silence;
        match p.unexplained_cost {
            Some(l) => base + l * self.n_unexplained_events() as f64,
            None => base,
        }
    }

    pub fn n_unexplained_events(&self) -> usize {
        self.tracks.iter().filter(|t| t.is_none()).count()
    }

    /// Per-event ids, `UNEXPLAINED_ID` for unexplained events.
    pub fn track_ids(&self, id: impl Fn(usize) -> String) -> Vec<String> {
        self.tracks.iter().map(|t| t.map_or_else(|| UNEXPLAINED_ID.to_string(), &id)).collect()
    }

    /// Per-event scores from the explained events' scores (in event order); unexplained events get z = 0.
    pub fn per_event_scores(&self, scored: Vec<EventScore>) -> Vec<EventScore> {
        let mut it = scored.into_iter();
        self.tracks
            .iter()
            .map(|t| match t {
                Some(_) => it.next().expect("one score per explained event"),
                None => EventScore { z: 0.0, f0: 0.0, coverage: 0.0 },
            })
            .collect()
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
    let options: Vec<Vec<Choice>> = events
        .iter()
        .map(|e| {
            let mut o: Vec<(usize, Admit)> = cpas
                .iter()
                .enumerate()
                .filter_map(|(i, c)| c.as_ref().and_then(|c| p.soft.admit(e, c, p.t_start_prior, slack)).map(|a| (i, a)))
                .collect();
            o.sort_by(|a, b| a.1.cost_z.total_cmp(&b.1.cost_z));
            o.into_iter().map(Some).collect()
        })
        .collect();
    let mut out = cheapest(&options, false, p);
    // Partial hypotheses get their own budget and cap, so full ones are exactly those of the default.
    if p.unexplained_cost.is_some() {
        let with_none: Vec<Vec<Choice>> = options.iter().map(|o| o.iter().copied().chain([None]).collect()).collect();
        out.extend(cheapest(&with_none, true, p));
    }
    for h in &mut out {
        silence(cpas, h, duration_s, p);
    }
    out
}

/// Slant range beyond which a CPA cannot affect `associate` at this `half_diag_m`: the outer soft-gate
/// bound or the silence range. The 1e-9 relative margin keeps `sqrt` rounding at the boundary safe.
pub fn cpa_reach_m(events: &[EventSpec], p: &AssocParams, half_diag_m: f64) -> f64 {
    let slack = p.soft.slack_m(half_diag_m);
    events.iter().map(|e| p.soft.max_slant_m(e, slack)).fold(p.silence_range_m, f64::max) * (1.0 + 1e-9)
}

/// Recording-start window wide enough for every clock `associate` can admit for `events` at this `half_diag_m`.
pub fn clock_window(events: &[EventSpec], p: &AssocParams, half_diag_m: f64) -> (f64, f64) {
    let slack = p.soft.slack_m(half_diag_m);
    let pad = events.iter().map(|e| p.soft.max_clock_pad_s(e, slack)).fold(0.0, f64::max);
    (p.t_start_prior.0 - pad, p.t_start_prior.1 + pad)
}

/// One event's pairing; None = unexplained.
type Choice = Option<(usize, Admit)>;

/// The `max_hypotheses` cheapest (by gate cost) full (`partial` false) or partial hypotheses.
fn cheapest(opts: &[Vec<Choice>], partial: bool, p: &AssocParams) -> Vec<Hypothesis> {
    let mut out = Vec::new();
    let mut stack = Vec::with_capacity(opts.len());
    let budget = p.max_hypotheses.saturating_mul(ENUM_FACTOR);
    expand(opts, &mut stack, partial, p, budget, &mut out);
    out.sort_by(|a, b| a.gate_cost.total_cmp(&b.gate_cost));
    out.truncate(p.max_hypotheses);
    out
}

fn expand(opts: &[Vec<Choice>], stack: &mut Vec<Choice>, partial: bool, p: &AssocParams, budget: usize, out: &mut Vec<Hypothesis>) {
    if out.len() >= budget {
        return;
    }
    if stack.len() == opts.len() {
        let n = stack.iter().flatten().count();
        if n == 0 || (n < stack.len()) != partial {
            return;
        }
        // Plain mean on purpose: an inverse-σ_t² mean moved N788BM_R03 41 → 364 m (band width ≠ error).
        let clock = stack.iter().flatten().map(|s| s.1.clock).sum::<f64>() / n as f64;
        let gate_cost = stack.iter().flatten().map(|s| s.1.cost_z).sum();
        let tracks = stack.iter().map(|s| s.map(|s| s.0)).collect();
        out.push(Hypothesis { tracks, clock, unexplained: 0, gate_cost, silence_nats: 0.0 });
        return;
    }
    // A partial pass never walks into a full leaf.
    let only_none = partial && stack.len() + 1 == opts.len() && stack.iter().all(Option::is_some);
    for &c in &opts[stack.len()] {
        let fits = c.is_none_or(|(i, a)| {
            !only_none
                && stack.iter().flatten().all(|s| s.0 != i && (s.1.clock - a.clock).abs() <= p.clock_tol_s + s.1.tau_s + a.tau_s)
        });
        if fits {
            stack.push(c);
            expand(opts, stack, partial, p, budget, out);
            stack.pop();
        }
    }
}

/// Sets the silence count and likelihood cost of `h`: audible flights whose CPA sound arrives during
/// the recording and that no event is assigned to.
fn silence(cpas: &[Option<Cpa>], h: &mut Hypothesis, duration_s: f64, p: &AssocParams) {
    let (a, b) = (h.clock, h.clock + duration_s);
    let s = &p.silence;
    let silent = cpas
        .iter()
        .enumerate()
        .filter(|(i, _)| !h.tracks.contains(&Some(*i)))
        .filter_map(|(_, c)| c.as_ref())
        .filter(|c| c.t_arrive >= a && c.t_arrive <= b && c.slant_m <= p.silence_range_m && s.counts(c));
    let (mut n, mut nats) = (0, 0.0);
    for c in silent {
        n += 1;
        if s.mode == SilenceMode::Likelihood {
            nats += s.miss_cost_nats(c.slant_m);
        }
    }
    (h.unexplained, h.silence_nats) = (n, nats);
}
