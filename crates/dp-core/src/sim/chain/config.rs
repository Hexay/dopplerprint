//! `[chain]` scenario schema: recording-chain realism. Every table is optional and the defaults reproduce
//! the chain-free render bit for bit. Levels named `tnr_db` are line power over the background PSD at
//! 100 Hz in a 1 Hz band (the `[noise] snr_db` reference). Evidence: docs/PHYSICS.md §6 "Recording chain".

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Chain {
    pub hum: Option<Hum>,
    pub lines: Vec<Line>,
    pub wind: Option<Wind>,
    pub source: Option<SourceMod>,
    pub path: Option<PathMod>,
    pub device: ChainDevice,
}

/// Mains hum with grid-frequency (ENF) wander shared coherently by all harmonics.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Hum {
    pub mains_hz: f64,
    /// Relative amplitudes of k = 1, 2, ...; normalised so the largest sits at `tnr_db`.
    pub harmonics: Vec<f64>,
    pub tnr_db: f64,
    /// Mean grid offset from nominal.
    pub offset_mhz: f64,
    /// Ornstein–Uhlenbeck grid wander: stationary RMS and correlation time.
    pub wander_mhz: f64,
    pub wander_tau_s: f64,
    /// White per-second ENF jitter on top of the OU walk.
    pub jitter_mhz: f64,
    /// RMS of slow (minutes) per-harmonic level wander.
    pub level_wander_db: f64,
}

impl Default for Hum {
    fn default() -> Self {
        Self {
            mains_hz: 60.0,
            harmonics: vec![0.05, 0.8, 0.2, 0.7, 0.15, 1.0],
            tnr_db: 0.0,
            offset_mhz: 0.0,
            wander_mhz: 14.0,
            wander_tau_s: 60.0,
            jitter_mhz: 1.5,
            level_wander_db: 2.0,
        }
    }
}

/// A persistent local narrowband source (motor, fan, transformer) with small frequency wander.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    pub hz: f64,
    pub tnr_db: f64,
    /// Relative amplitudes of k·hz for k = 1, 2, ... (zeros allowed).
    #[serde(default = "unit_harmonic")]
    pub harmonics: Vec<f64>,
    /// RMS slow frequency wander of the fundamental.
    #[serde(default = "default_line_wander")]
    pub wander_hz: f64,
}

/// Gusty low-frequency wind noise: on/off gusts covering `fraction` of the clip.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Wind {
    pub fraction: f64,
    /// Gust PSD below the corner, over the 100 Hz background PSD, at median gust strength.
    pub level_db: f64,
    /// Third-order low-pass corner of the rumble.
    pub corner_hz: f64,
    /// Mean gust duration.
    pub gust_s: f64,
}

impl Default for Wind {
    fn default() -> Self {
        Self { fraction: 0.15, level_db: 35.0, corner_hz: 40.0, gust_s: 2.0 }
    }
}

/// Per-aircraft random source realism, drawn once per aircraft from the chain RNG and added to
/// whatever `[aircraft.source]` already specifies.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SourceMod {
    /// RMS fractional amplitude modulation of every harmonic (independent per harmonic).
    pub am_depth: f64,
    pub am_rate_hz: f64,
    /// SD of a per-aircraft linear f0 drift, in percent per minute.
    pub drift_pct_per_min_sd: f64,
    /// Probability of one rpm/pitch step during the audible pass, and SD of its size in percent.
    pub step_prob: f64,
    pub step_pct_sd: f64,
    pub step_ramp_s: f64,
    /// Amplitude of a half-order (0.5 f0) line relative to the fundamental.
    pub half_harmonic: f64,
}

impl Default for SourceMod {
    fn default() -> Self {
        Self {
            am_depth: 0.0,
            am_rate_hz: 1.0,
            drift_pct_per_min_sd: 0.0,
            step_prob: 0.0,
            step_pct_sd: 0.0,
            step_ramp_s: 3.0,
            half_harmonic: 0.0,
        }
    }
}

/// Turbulence-induced coherence loss on the ground-reflected path.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PathMod {
    /// Mean coherence E[cos ψ] of the reflection at 250 Hz (1 = rigid mirror, 0 = incoherent).
    pub ground_coherence: f64,
    /// Typical rate of the turbulent delay fluctuation.
    pub turbulence_hz: f64,
}

impl Default for PathMod {
    fn default() -> Self {
        Self { ground_coherence: 1.0, turbulence_hz: 0.5 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channels {
    #[default]
    Mono,
    /// Two bit-identical channels (the only "stereo" in the real corpus).
    DualMono,
    /// Two microphones `stereo_baseline_m` apart with independent self-noise.
    Stereo,
}

/// Additions to `[device]`, applied in order: high-pass (order), low-pass, NS, normalise, clip, quantise.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ChainDevice {
    /// Butterworth order of `[device] highpass_hz` (even).
    pub highpass_order: u32,
    /// Upstream band limit (4th-order Butterworth), e.g. a 9.5 kHz app/codec shelf.
    pub lowpass_hz: Option<f64>,
    pub ns: Option<Ns>,
    /// Quantise output samples to this many bits (the eval sees the quantised floor).
    pub quantise_bits: Option<u32>,
    pub channels: Channels,
    pub stereo_baseline_m: f64,
    /// Direction of the left→right microphone axis, degrees clockwise from north.
    pub stereo_axis_deg: f64,
    /// Independent per-channel self-noise (white) over the 100 Hz background PSD.
    pub self_noise_db: f64,
}

impl Default for ChainDevice {
    fn default() -> Self {
        Self {
            highpass_order: 2,
            lowpass_hz: None,
            ns: None,
            quantise_bits: None,
            channels: Channels::Mono,
            stereo_baseline_m: 0.12,
            stereo_axis_deg: 90.0,
            self_noise_db: -10.0,
        }
    }
}

/// WebRTC-like noise suppression: anything steady in a bin is pulled down to `depth_db` over ~`time_s`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Ns {
    pub depth_db: f64,
    pub time_s: f64,
}

impl Default for Ns {
    fn default() -> Self {
        Self { depth_db: -15.0, time_s: 2.0 }
    }
}

fn unit_harmonic() -> Vec<f64> {
    vec![1.0]
}
fn default_line_wander() -> f64 {
    0.01
}

impl Chain {
    pub fn validate(&self) -> anyhow::Result<()> {
        let d = &self.device;
        anyhow::ensure!(d.highpass_order >= 2 && d.highpass_order.is_multiple_of(2), "chain.device.highpass_order must be even ≥ 2");
        if let Some(b) = d.quantise_bits {
            anyhow::ensure!((2..=24).contains(&b), "chain.device.quantise_bits must be in 2..=24");
        }
        if let Some(ns) = &d.ns {
            anyhow::ensure!(ns.depth_db <= 0.0 && ns.time_s > 0.0, "chain.device.ns: depth_db ≤ 0, time_s > 0");
        }
        if let Some(w) = &self.wind {
            anyhow::ensure!((0.0..1.0).contains(&w.fraction) && w.gust_s > 0.0, "chain.wind: fraction in [0, 1), gust_s > 0");
        }
        if let Some(p) = &self.path {
            anyhow::ensure!((0.0..=1.0).contains(&p.ground_coherence), "chain.path.ground_coherence in [0, 1]");
        }
        if let Some(h) = &self.hum {
            anyhow::ensure!(h.harmonics.iter().any(|&a| a > 0.0), "chain.hum.harmonics needs a positive entry");
        }
        Ok(())
    }
}
