//! Soft association gates: the cost of letting a CPA explain an event, from how far its slant, ground
//! speed and implied recording clock fall outside the event's ranges.
//!
//! The event's own ranges (the CV fit's ±kσ band with floors) and the clock prior cost nothing; beyond them
//! the excess x is scored with the Student-t fit-error density, relative to its value at the range edge:
//!
//! ```text
//! x_d = ln-distance of CPA slant d from [d_lo − r, d_hi + r]       e.g. ln(d / (d_hi + r)) above it
//! x_v = ln-distance of CPA ground speed v from [v_lo, v_hi]
//! x_t = distance (s) of the implied clock t_arrive − t_cpa_audio from [t0 − τ, t1 + τ], τ = r/max(v, v_min) + r/c_min
//! w   = ½·ln(hi/lo) of the event's own d or v range (its centre-to-edge distance); 0 for the clock prior
//! σ_t = sigma_clock_ref_s · (clamp(h, h_ref, h_max)/h_ref)^clock_band_exp   per event; h = its t_cpa band half-width
//! ρ(u; σ) = (ν+1)/2 · ln(1 + (u/σ)²/ν)                          Student-t NLL (nats) up to a constant
//! tail(x; w, σ) = ρ(w + x; σ) − ρ(w; σ)                        0 at the edge, grows ~ (ν+1)·ln x
//! cost_z = z_per_nat · (tail(x_d; w_d, σ_d) + tail(x_v; w_v, σ_v) + tail(x_t; 0, σ_t))
//! ```
//!
//! σ_d, σ_v, ν and the σ_t law are Student-t MLEs of ln(d_fit/d_true), ln(v_fit/v_gs) and the t_cpa error over
//! discovered events with a true pass (even-indexed half of Livermore-40 + sim_chain phone_livermore_like);
//! the σ_t power law beats one global σ_t on the held-out half. See research_notes/…/fix_soft_assoc_gates.md
//! and fix_cpa_time_uncertainty.md. The d/v outer bounds are tight on purpose: wider ones let spurious
//! events pair with some flight. The clock outer bound is `outer_clock_sigmas`·σ_t, per event.
//!
//! Exact events (truth labels, `EventSpec::is_exact`) have exact CPA times and prior-box d/v ranges: every
//! gate is hard for them, with no cell slack — the legacy gates.
//!
//! `z_per_nat` converts nats to score (z) units, the units of `silence_penalty`. It is fixed at
//! `GATE_Z_PER_NAT` = 1.58, the posterior temperature T the gates were tuned under (calibration of
//! 2026-11-03), and deliberately not read from `calibration.toml`: refitting T must change only the posterior,
//! never which hypotheses are admitted or how they rank.
//! A CPA whose excess exceeds an outer bound is dropped; outer = 0 makes that dimension a hard gate.
//! Cell slack r (the cell's half-diagonal) widens only that pruning test: CPA slant changes by at most r
//! across the cell and CPA time by at most r/v + r/c, so no cell is dropped that some point inside it would
//! admit. The cost itself is taken at the cell centre (r = 0): a zero-cost slack let coarse cells, scored
//! at inadmissible centres, outrank their finer children (Livermore oracle 201 → 317 m).

use super::EventSpec;
use crate::physics::Cpa;

/// Lowest sound speed (m/s) assumed when bounding arrival-time change across a cell.
const C_MIN: f64 = 300.0;
/// Score units per nat for gate costs (see module doc).
pub const GATE_Z_PER_NAT: f64 = 1.58;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SoftGates {
    /// Student-t degrees of freedom shared by all three tails.
    pub nu: f64,
    /// Tail scales: log slant ratio, log speed ratio.
    pub sigma_log_d: f64,
    pub sigma_log_v: f64,
    /// Clock tail scale (s) of an event whose t_cpa band half-width is at most `clock_band_ref_s`.
    pub sigma_clock_ref_s: f64,
    pub clock_band_ref_s: f64,
    /// σ_t grows as (band half-width / `clock_band_ref_s`)^this, up to a half-width of `clock_band_max_s`.
    pub clock_band_exp: f64,
    /// Wider bands are ill-conditioned fits, not larger errors (N748SP_R36: ±644 s band, 0.8 s error).
    pub clock_band_max_s: f64,
    /// Hard bounds on the excess, for pruning only.
    pub outer_log_d: f64,
    pub outer_log_v: f64,
    /// Clock outer bound in units of the event's σ_t.
    pub outer_clock_sigmas: f64,
    /// Score units per nat (`GATE_Z_PER_NAT`).
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
            sigma_clock_ref_s: 0.8,
            clock_band_ref_s: 3.0,
            clock_band_exp: 0.4,
            clock_band_max_s: 30.0,
            outer_log_d: 0.4,
            outer_log_v: 0.15,
            outer_clock_sigmas: 8.0,
            z_per_nat: GATE_Z_PER_NAT,
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

/// Per-event gate shape: clock scale, outer bounds [d, v, clock] and the slack it gets.
struct EventGates {
    sigma_t: f64,
    outer: [f64; 3],
    slack_m: f64,
}

impl SoftGates {
    /// Legacy hard gates for every event: no tail, no cell slack.
    pub fn hard() -> Self {
        Self { outer_log_d: 0.0, outer_log_v: 0.0, outer_clock_sigmas: 0.0, cell_slack: false, ..Self::default() }
    }

    /// The slack radius actually used for a cell of half-diagonal `half_diag_m`.
    pub fn slack_m(&self, half_diag_m: f64) -> f64 {
        if self.cell_slack { half_diag_m } else { 0.0 }
    }

    /// σ_t (s) of `e`'s clock tail; 0 for an exact event.
    pub fn clock_sigma_s(&self, e: &EventSpec) -> f64 {
        if e.is_exact() {
            return 0.0;
        }
        let r = e.t_cpa_halfwidth_s.clamp(self.clock_band_ref_s, self.clock_band_max_s) / self.clock_band_ref_s;
        self.sigma_clock_ref_s * r.powf(self.clock_band_exp)
    }

    fn event(&self, e: &EventSpec, slack_m: f64) -> EventGates {
        if e.is_exact() {
            return EventGates { sigma_t: 0.0, outer: [0.0; 3], slack_m: 0.0 };
        }
        let sigma_t = self.clock_sigma_s(e);
        EventGates { sigma_t, outer: [self.outer_log_d, self.outer_log_v, self.outer_clock_sigmas * sigma_t], slack_m }
    }

    /// Cost of `c` explaining `e` under clock prior `prior`, or `None` beyond an outer bound.
    pub fn admit(&self, e: &EventSpec, c: &Cpa, prior: (f64, f64), slack_m: f64) -> Option<Admit> {
        let g = self.event(e, slack_m);
        let tau_s = g.slack_m / c.ground_speed.max(self.slack_min_speed_mps) + g.slack_m / C_MIN;
        let wide = excess(e, c, prior, g.slack_m, tau_s);
        let centre = if g.slack_m > 0.0 { excess(e, c, prior, 0.0, 0.0) } else { wide };
        // A hard dimension (outer = 0) is gated at the centre; slack would admit it with a tail cost.
        if (0..3).any(|k| wide[k] > g.outer[k] || (g.outer[k] == 0.0 && centre[k] > 0.0)) {
            return None;
        }
        let [x_d, x_v, x_t] = centre;
        let nats = self.tail(x_d, half_log_width(e.d_range), self.sigma_log_d)
            + self.tail(x_v, half_log_width(e.v_range), self.sigma_log_v)
            + self.tail(x_t, 0.0, g.sigma_t);
        Some(Admit { clock: c.t_arrive - e.t_cpa_audio, tau_s, cost_z: self.z_per_nat * nats })
    }

    /// Largest slant `admit` can accept for `e`.
    pub fn max_slant_m(&self, e: &EventSpec, slack_m: f64) -> f64 {
        let g = self.event(e, slack_m);
        (e.d_range.1 + g.slack_m) * g.outer[0].exp()
    }

    /// Largest |implied clock − prior| `admit` can accept for `e` and any CPA.
    pub fn max_clock_pad_s(&self, e: &EventSpec, slack_m: f64) -> f64 {
        let g = self.event(e, slack_m);
        g.outer[2] + g.slack_m / self.slack_min_speed_mps + g.slack_m / C_MIN
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
