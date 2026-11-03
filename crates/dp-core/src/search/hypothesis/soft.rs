//! Soft association gates: the cost of letting a CPA explain an event, from how far its slant, ground
//! speed and implied recording clock fall outside the event's ranges.
//!
//! The event's own ranges (the CV fit's ±kσ band with floors, or the oracle box) and the clock prior cost
//! nothing; beyond them the excess x is scored with the Student-t fit-error density, relative to its value
//! at the range edge:
//!
//! ```text
//! x_d = ln-distance of CPA slant d from [d_lo − r, d_hi + r]       e.g. ln(d / (d_hi + r)) above it
//! x_v = ln-distance of CPA ground speed v from [v_lo, v_hi]
//! x_t = distance (s) of the implied clock t_arrive − t_cpa_audio from [t0 − τ, t1 + τ], τ = r/max(v, v_min) + r/c_min
//! w   = ½·ln(hi/lo) of the event's own d or v range (its centre-to-edge distance); 0 for the clock prior
//! ρ(u; σ) = (ν+1)/2 · ln(1 + (u/σ)²/ν)                          Student-t NLL (nats) up to a constant
//! tail(x; w, σ) = ρ(w + x; σ) − ρ(w; σ)                        0 at the edge, grows ~ (ν+1)·ln x
//! cost_z = z_per_nat · (tail(x_d; w_d, σ_d) + tail(x_v; w_v, σ_v) + tail(x_t; 0, σ_t))
//! ```
//!
//! σ, ν are Student-t MLEs of ln(d_fit/d_true), ln(v_fit/v_gs), t_cpa error over discovered events that
//! have a true pass (even-indexed half of Livermore-40 + sim_chain phone_livermore_like, ν = 2); see
//! research_notes/…/fix_soft_assoc_gates.md. The d/v outer bounds are tight on purpose: wider ones let
//! spurious events pair with some flight, which defeats the drop-weakest-event retry of the front end.
//! Oracle (truth-label) events have exact CPA times; eval searches them with `hard()`.
//!
//! `z_per_nat` is the posterior temperature T (`calibration.toml`): the posterior is exp((score − top)/T),
//! so a log-likelihood of −n nats is −n·T in score (z) units, the same units as `silence_penalty`.
//! A CPA whose excess exceeds an `outer_*` bound is dropped; all `outer_*` = 0 gives the legacy hard gates.
//! Cell slack r (the cell's half-diagonal) widens only that pruning test: CPA slant changes by at most r
//! across the cell and CPA time by at most r/v + r/c, so no cell is dropped that some point inside it would
//! admit. The cost itself is taken at the cell centre (r = 0): a zero-cost slack let coarse cells, scored
//! at inadmissible centres, outrank their finer children (Livermore oracle 201 → 317 m).

use super::EventSpec;
use crate::physics::Cpa;

/// Lowest sound speed (m/s) assumed when bounding arrival-time change across a cell.
const C_MIN: f64 = 300.0;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SoftGates {
    /// Student-t degrees of freedom shared by all three tails.
    pub nu: f64,
    /// Tail scales: log slant ratio, log speed ratio, clock (s).
    pub sigma_log_d: f64,
    pub sigma_log_v: f64,
    pub sigma_clock_s: f64,
    /// Hard bounds on the excess, for pruning only.
    pub outer_log_d: f64,
    pub outer_log_v: f64,
    pub outer_clock_s: f64,
    /// Score units per nat (posterior temperature).
    pub z_per_nat: f64,
    /// Widen ranges by what can change across the scored cell (see module doc).
    pub cell_slack: bool,
    /// Ground-speed floor (m/s) when bounding CPA-time change across a cell.
    pub slack_min_speed_mps: f64,
}

impl Default for SoftGates {
    fn default() -> Self {
        Self {
            nu: 2.0,
            sigma_log_d: 0.29,
            sigma_log_v: 0.15,
            sigma_clock_s: 1.15,
            outer_log_d: 0.4,
            outer_log_v: 0.15,
            outer_clock_s: 8.0,
            z_per_nat: crate::search::calibration::get().temperature,
            cell_slack: true,
            slack_min_speed_mps: 20.0,
        }
    }
}

/// One admissible (event, CPA) pairing.
#[derive(Clone, Copy, Debug)]
pub struct Admit {
    /// Implied recording clock: UNIX = audio + clock.
    pub clock: f64,
    /// Clock tolerance (s) this CPA gets from the cell extent.
    pub tau_s: f64,
    /// Soft-gate cost in score (z) units.
    pub cost_z: f64,
}

impl SoftGates {
    /// Legacy hard gates: no tail, no cell slack.
    pub fn hard() -> Self {
        Self { outer_log_d: 0.0, outer_log_v: 0.0, outer_clock_s: 0.0, cell_slack: false, ..Self::default() }
    }

    /// The slack radius actually used for a cell of half-diagonal `half_diag_m`.
    pub fn slack_m(&self, half_diag_m: f64) -> f64 {
        if self.cell_slack { half_diag_m } else { 0.0 }
    }

    /// Cost of `c` explaining `e` under clock prior `prior`, or `None` beyond an outer bound.
    pub fn admit(&self, e: &EventSpec, c: &Cpa, prior: (f64, f64), slack_m: f64) -> Option<Admit> {
        let tau_s = slack_m / c.ground_speed.max(self.slack_min_speed_mps) + slack_m / C_MIN;
        let wide = excess(e, c, prior, slack_m, tau_s);
        let centre = if slack_m > 0.0 { excess(e, c, prior, 0.0, 0.0) } else { wide };
        let outer = [self.outer_log_d, self.outer_log_v, self.outer_clock_s];
        // A hard dimension (outer = 0) is gated at the centre; slack would admit it with a tail cost.
        if (0..3).any(|k| wide[k] > outer[k] || (outer[k] == 0.0 && centre[k] > 0.0)) {
            return None;
        }
        let [x_d, x_v, x_t] = centre;
        let nats = self.tail(x_d, half_log_width(e.d_range), self.sigma_log_d)
            + self.tail(x_v, half_log_width(e.v_range), self.sigma_log_v)
            + self.tail(x_t, 0.0, self.sigma_clock_s);
        Some(Admit { clock: c.t_arrive - e.t_cpa_audio, tau_s, cost_z: self.z_per_nat * nats })
    }

    /// Largest slant `admit` can accept for `e`.
    pub fn max_slant_m(&self, e: &EventSpec, slack_m: f64) -> f64 {
        (e.d_range.1 + slack_m) * self.outer_log_d.exp()
    }

    /// Largest |implied clock − prior| `admit` can accept for any CPA.
    pub fn max_clock_pad_s(&self, slack_m: f64) -> f64 {
        self.outer_clock_s + slack_m / self.slack_min_speed_mps + slack_m / C_MIN
    }

    /// ρ((w + x)/σ) − ρ(w/σ): Student-t NLL at excess x past a range edge w from its centre.
    fn tail(&self, x: f64, w: f64, sigma: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        let rho = |u: f64| 0.5 * (self.nu + 1.0) * (1.0 + (u / sigma).powi(2) / self.nu).ln();
        rho(w + x) - rho(w)
    }
}

/// [x_d, x_v, x_t] of `c` for `e` with ranges widened by `slack_m` (slant) and `tau_s` (clock).
fn excess(e: &EventSpec, c: &Cpa, prior: (f64, f64), slack_m: f64, tau_s: f64) -> [f64; 3] {
    let clock = c.t_arrive - e.t_cpa_audio;
    [
        log_excess(c.slant_m, ((e.d_range.0 - slack_m).max(0.0), e.d_range.1 + slack_m)),
        log_excess(c.ground_speed, e.v_range),
        (prior.0 - tau_s - clock).max(clock - prior.1 - tau_s).max(0.0),
    ]
}

fn half_log_width((lo, hi): (f64, f64)) -> f64 {
    if lo > 0.0 { 0.5 * (hi / lo).ln() } else { 0.0 }
}

/// How far (in log units) `x` lies outside `[lo, hi]`; 0 inside. A non-positive `lo` admits everything below.
fn log_excess(x: f64, (lo, hi): (f64, f64)) -> f64 {
    if x > hi {
        (x / hi).ln()
    } else if x < lo && lo > 0.0 {
        if x <= 0.0 { f64::INFINITY } else { (lo / x).ln() }
    } else {
        0.0
    }
}
