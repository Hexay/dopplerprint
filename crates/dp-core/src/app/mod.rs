//! `dopplerprint locate`: one call from an audio file to a location, a short report and result files.
//! The CLI and the Python package both call `locate`; flags and defaults are documented in docs/USAGE.md.

pub mod cache;
pub mod fetch;
pub mod filetime;
pub mod geocode;
pub mod http;
mod inputs;
pub mod place;
pub mod posterior;
pub mod report;
pub mod results;
mod run;
pub mod text;
pub mod ui;
pub mod weather;
pub mod when;

use std::path::PathBuf;

pub use inputs::missing_fields;
pub use report::{Report, Status};
pub use ui::{ConsoleUi, QuietUi, Ui};

/// Something only the user can supply (a time, a place, consent to download). The message is one sentence
/// saying what to add, with an example.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct NeedsInput(pub String);

/// Default cache cap; one adsb.lol day is ~3.5 GB while downloading and ~1.5 GB once processed.
pub const DEFAULT_MAX_CACHE_GB: f64 = 20.0;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocateRequest {
    pub audio: PathBuf,
    /// Recording start, e.g. "2026-05-14 10:02 -07:00", optionally with "±10m".
    pub start: Option<String>,
    /// Overrides the tolerance in `start` (default ±2 min).
    pub start_tolerance: Option<String>,
    /// "lat,lon", "lat,lon,radius_km", "west,south,east,north" or a place name.
    pub near: Option<String>,
    pub radius_km: Option<f64>,
    pub cache: Option<PathBuf>,
    /// Result directory (default ./dopplerprint-results/<audio stem>).
    pub out: Option<PathBuf>,
    pub dem: Option<PathBuf>,
    /// Mains hum to notch (50 or 60 Hz).
    pub mains_hz: Option<f64>,
    /// Experiment TOML (`config::Config`) overriding spectrogram and search defaults.
    pub config: Option<PathBuf>,
    pub offline: bool,
    /// Search in still air (the config's atmosphere) instead of archived winds aloft and temperature.
    pub no_weather: bool,
    /// Download without asking.
    pub yes: bool,
    pub keep_raw: bool,
    pub max_cache_gb: f64,
    /// Strongest passes used in the search.
    pub max_events: usize,
}

impl Default for LocateRequest {
    fn default() -> Self {
        Self {
            audio: PathBuf::new(),
            start: None,
            start_tolerance: None,
            near: None,
            radius_km: None,
            cache: None,
            out: None,
            dem: None,
            mains_hz: None,
            config: None,
            offline: false,
            no_weather: false,
            yes: false,
            keep_raw: false,
            max_cache_gb: DEFAULT_MAX_CACHE_GB,
            max_events: 4,
        }
    }
}

/// Run the whole pipeline. Missing input becomes a `NeedsInput` report, a search that can't decide an
/// `Inconclusive` one; `Err` is reserved for real failures (unreadable file, network, disk).
pub fn locate(req: &LocateRequest, ui: &dyn Ui) -> anyhow::Result<Report> {
    run::run(req, ui)
}
