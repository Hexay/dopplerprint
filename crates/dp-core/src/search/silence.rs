//! Silence term: what an audible flight that no event explains costs a hypothesis.
//!
//! `flat` (default): `AssocParams::silence_penalty` score units per flight whose CPA sound arrives during
//! the recording within `silence_range_m`. `likelihood`: per such flight, unless its CPA is a track end
//! (coverage began or ended, so the pass itself is unseen), the miss probability in nats:
//!
//! ```text
//! margin_dB  = −20·log10(r / r50) − hp_loss_dB                 r = CPA slant range
//! P_d        = cap · logistic(margin_dB · π / (√3 · σ_dB))     logistic with the s.d. of N(0, σ_dB²)
//! cost       = −ln(1 − P_d) nats, summed over flights           ≤ −ln(1 − cap) each
//! hp_loss_dB = 10·log10(Σ_h s_h² / Σ_{h : h·f_src ≥ f_hp} s_h²) (P_d = 0 when no harmonic survives)
//! ```
//!
//! r50 is the slant range at which the nominal source in a recording without high-pass is heard with
//! P_d = cap/2; σ_dB spreads source level and propagation between flights; s_h is the per-harmonic
//! per-pixel SNR prior (as `power_llr::LlrParams::snr`); f_src is one nominal fundamental for every
//! engine class (prop blade pass, 70–130 Hz); f_hp is the recording's measured high-pass corner
//! (`audio::highpass::corner_hz`). The absolute floor level is not used: phone gain and noise suppression
//! move level and floor together (corpus_signatures.md §6: NS costs level, not TNR), so the floor's shape
//! is the only gain-free input. Score units: z = nats · `SoftGates::z_per_nat` (the posterior temperature)
//! under zsum, nats under power_llr (`score_per_nat`).

use super::score::ScoreMode;
use crate::audio::{Audio, SpecParams};
use crate::physics::Cpa;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SilenceMode {
    #[default]
    Flat,
    Likelihood,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SilenceParams {
    pub mode: SilenceMode,
    pub r50_m: f64,
    pub sigma_db: f64,
    pub cap: f64,
    pub f_src_hz: f64,
    pub harmonic_snr: Vec<f64>,
    /// Measured per recording (`measure_recording`); 0 = no high-pass.
    pub hp_corner_hz: f64,
}

impl Default for SilenceParams {
    fn default() -> Self {
        Self {
            mode: SilenceMode::Flat,
            r50_m: 3_000.0,
            sigma_db: 6.0,
            cap: 0.9,
            f_src_hz: 90.0,
            harmonic_snr: vec![1.0, 1.0, 0.6, 0.4, 0.3, 0.2],
            hp_corner_hz: 0.0,
        }
    }
}

impl SilenceParams {
    /// In likelihood mode, sets `hp_corner_hz` from `audio`; a no-op in flat mode.
    pub fn measure_recording(&mut self, audio: &Audio, sp: &SpecParams) {
        if self.mode == SilenceMode::Likelihood {
            self.hp_corner_hz = crate::audio::highpass::corner_hz(audio, sp);
        }
    }

    /// Whether `c` (an unassigned flight's CPA) can count against a hypothesis at all.
    pub fn counts(&self, c: &Cpa) -> bool {
        self.mode == SilenceMode::Flat || !c.at_track_end
    }

    pub fn hp_loss_db(&self) -> f64 {
        let total: f64 = self.harmonic_snr.iter().map(|s| s * s).sum();
        let kept: f64 = self
            .harmonic_snr
            .iter()
            .enumerate()
            .filter(|(i, _)| (*i + 1) as f64 * self.f_src_hz >= self.hp_corner_hz)
            .map(|(_, s)| s * s)
            .sum();
        if kept > 0.0 { 10.0 * (total / kept).log10() } else { f64::INFINITY }
    }

    /// Detection probability of a flight at CPA slant `slant_m`.
    pub fn p_detect(&self, slant_m: f64) -> f64 {
        let margin = -20.0 * (slant_m.max(1.0) / self.r50_m).log10() - self.hp_loss_db();
        let k = std::f64::consts::PI / (3f64.sqrt() * self.sigma_db);
        self.cap / (1.0 + (-margin * k).exp())
    }

    /// −ln(1 − P_d) nats.
    pub fn miss_cost_nats(&self, slant_m: f64) -> f64 {
        -(1.0 - self.p_detect(slant_m)).ln()
    }
}

/// Score units per nat: `z_per_nat` for zsum, 1 for power_llr.
pub fn score_per_nat(mode: ScoreMode, z_per_nat: f64) -> f64 {
    match mode {
        ScoreMode::Zsum => z_per_nat,
        ScoreMode::PowerLlr => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn likelihood() -> SilenceParams {
        SilenceParams { mode: SilenceMode::Likelihood, ..Default::default() }
    }

    #[test]
    fn p_detect_falls_with_range_and_never_exceeds_the_cap() {
        let s = likelihood();
        let p: Vec<f64> = [10.0, 300.0, 1_000.0, 3_000.0, 6_000.0, 20_000.0].iter().map(|&r| s.p_detect(r)).collect();
        assert!(p.windows(2).all(|w| w[0] > w[1]), "{p:?}");
        assert!(p.iter().all(|&x| x > 0.0 && x <= s.cap));
        assert!((s.p_detect(s.r50_m) - 0.5 * s.cap).abs() < 1e-12);
        let max = -(1.0 - s.cap).ln();
        assert!(s.miss_cost_nats(10.0) <= max && s.miss_cost_nats(10.0) > 0.99 * max);
        assert!(s.miss_cost_nats(1_000.0) > s.miss_cost_nats(3_000.0));
    }

    #[test]
    fn high_pass_removes_harmonics_and_can_deafen() {
        let mut s = likelihood();
        assert_eq!(s.hp_loss_db(), 0.0);
        s.hp_corner_hz = 160.0;
        let (one, base) = (s.hp_loss_db(), SilenceParams::default().p_detect(2_000.0));
        assert!((one - 10.0 * (2.65f64 / 1.65).log10()).abs() < 1e-9, "f_src 90 Hz: h1 is cut");
        assert!(s.p_detect(2_000.0) < base);
        s.hp_corner_hz = 1_000.0;
        assert_eq!(s.p_detect(500.0), 0.0);
        assert_eq!(s.miss_cost_nats(500.0), 0.0);
    }

    #[test]
    fn units_follow_the_score_mode_and_track_ends_count_only_when_flat() {
        assert_eq!(score_per_nat(ScoreMode::Zsum, 1.58), 1.58);
        assert_eq!(score_per_nat(ScoreMode::PowerLlr, 1.58), 1.0);
        let c = Cpa { t_emit: 0.0, t_arrive: 1.0, slant_m: 500.0, ground_speed: 50.0, at_track_end: true };
        assert!(SilenceParams::default().counts(&c) && !likelihood().counts(&c));
    }
}
