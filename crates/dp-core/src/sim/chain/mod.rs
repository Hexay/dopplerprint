//! Recording-chain realism for `sim`, grounded in the parity corpus: ENF-wandering even-harmonic hum,
//! persistent local lines, gusty wind, source AM/drift/steps, ground-reflection coherence loss, and a
//! device stage (HP order, band limit, NS, low gain, quantisation, dual-mono/stereo). Each generator
//! draws from its own RNG stream, so enabling one never changes another. See docs/PHYSICS.md §6.

pub mod config;
pub mod device;
pub mod hum;
pub mod lines;
pub mod modulation;
pub mod ns;
pub mod source_mod;
#[cfg(test)]
mod testutil;
pub mod wind;

use serde::{Deserialize, Serialize};

pub use config::{Chain, Channels};

use super::noise::Pcg;

/// Amplitude of a line whose power is `tnr_db` over the 100 Hz background PSD in 1 Hz, given `[noise] snr_db`.
pub fn tnr_amplitude(tnr_db: f64, snr_db: f64) -> f64 {
    10f64.powf((tnr_db - snr_db) / 20.0)
}

#[derive(Clone, Copy)]
pub enum Stream {
    Hum = 1,
    Lines,
    Wind,
    Source,
    Path,
    SelfNoise,
}

pub fn rng(seed: u64, s: Stream) -> Pcg {
    Pcg::new(seed ^ (0xC4A1_0000 + s as u64).wrapping_mul(0x2545_F491_4F6C_DD1D))
}

/// Everything the chain applied, for truth.json.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChainTruth {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hum: Option<hum::HumTruth>,
    #[serde(default)]
    pub lines: Vec<lines::LineTruth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind: Option<wind::WindTruth>,
    /// Effective per-aircraft source parameters after `[chain.source]` draws.
    #[serde(default)]
    pub sources: Vec<source_mod::SourceTruth>,
    /// The `[chain]` table as rendered (device, NS, path settings).
    pub config: Chain,
}

/// Adds hum, persistent lines and wind to the physical mix.
pub fn add_environment(mix: &mut [f64], fs: f64, c: &Chain, snr_db: f64, seed: u64) -> ChainTruth {
    let hum = c.hum.as_ref().map(|h| hum::add(mix, fs, h, snr_db, &mut rng(seed, Stream::Hum)));
    let lines = lines::add(mix, fs, &c.lines, snr_db, &mut rng(seed, Stream::Lines));
    let wind = c.wind.as_ref().map(|w| wind::add(mix, fs, w, snr_db, &mut rng(seed, Stream::Wind)));
    ChainTruth { hum, lines, wind, sources: Vec::new(), config: c.clone() }
}
