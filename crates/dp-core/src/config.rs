//! Tunable parameters. Experiments override any subset via TOML (experiments/*.toml).

use crate::audio::SpecParams;
use crate::search::SearchParams;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    pub spec: SpecParams,
    pub search: SearchParams,
    pub eval: EvalParams,
}

/// How a corpus case is posed to the search (how much prior knowledge we grant).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EvalParams {
    /// Search-region radius around a deterministically displaced centre (m).
    pub region_radius_m: f64,
    /// Displacement of the region centre from truth (m), so truth is never at the centre.
    pub region_offset_m: f64,
    /// Recording-start prior half-width around the true start (s).
    pub time_prior_s: f64,
    /// Use truth event windows/CPA times instead of automatic discovery (isolates the search stage).
    pub oracle_events: bool,
    /// ± fraction around oracle f0 when `oracle_events`.
    pub oracle_f0_frac: f64,
    /// Slant-range and speed bounds used for oracle events (truth has neither).
    pub oracle_d_range: (f64, f64),
    pub oracle_v_range: (f64, f64),
    /// Data roots.
    pub data_dir: std::path::PathBuf,
    pub store_dir: std::path::PathBuf,
    /// Only cases whose id contains one of these substrings (empty = all).
    pub cases: Vec<String>,
    /// Which corpus cases run: dev (default), the frozen test split, or all. See `eval::split`.
    pub split: crate::eval::split::Split,
    pub split_file: std::path::PathBuf,
    /// Posterior temperature β and uniform floor ε for log score / 90% region (see `eval::metrics`).
    pub posterior_beta: f64,
    pub posterior_floor: f64,
}

impl Default for EvalParams {
    fn default() -> Self {
        Self {
            region_radius_m: 4_000.0,
            region_offset_m: 1_500.0,
            time_prior_s: 120.0,
            oracle_events: true,
            oracle_f0_frac: 0.06,
            oracle_d_range: (300.0, 8_000.0),
            oracle_v_range: (25.0, 170.0),
            data_dir: "data".into(),
            store_dir: "data/adsb/store".into(),
            cases: Vec::new(),
            split: Default::default(),
            split_file: "experiments/split.toml".into(),
            posterior_beta: 1.0,
            posterior_floor: 0.01,
        }
    }
}

impl Config {
    pub fn from_toml(s: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(s)?)
    }
}
