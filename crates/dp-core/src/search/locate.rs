//! One-call search for a recording: whitened scoring plane + in-memory catalogue + coarse-to-fine search.

use super::hypothesis::EventSpec;
use super::pipeline::{Region, SearchInput, SearchParams, SearchResult, search};
use super::score::ScoringPlane;
use super::MemCatalog;
use crate::audio::spectrogram::log_spectrogram;
use crate::audio::{Audio, SpecParams};
use crate::dem::Dem;
use crate::events::Event;
use crate::track::Track;

impl EventSpec {
    /// Search bounds from a discovered event. The fit is of harmonic `ridge_harmonic`, so f0 is rescaled
    /// to the fundamental that the scorer's harmonic comb expects.
    pub fn from_event(e: &Event) -> Self {
        let h = e.ridge_harmonic.max(1) as f64;
        let window = e.ridge.t.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &t| (a.min(t), b.max(t)));
        EventSpec {
            window,
            t_cpa_audio: e.fit.best.t_cpa_arrive,
            d_range: e.fit.d_range,
            v_range: e.fit.v_range,
            f0_range: (e.fit.f0_range.0 / h, e.fit.f0_range.1 / h),
        }
    }
}

impl SearchResult {
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }
    pub fn from_json(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }
}

/// `p.assoc.t_start_prior` and `p.flat_h_ell_m` must already be set by the caller.
pub fn locate(
    audio: &Audio,
    events: &[EventSpec],
    tracks: Vec<Track>,
    region: Region,
    dem: Option<&Dem>,
    spec: &SpecParams,
    p: &SearchParams,
) -> SearchResult {
    let plane = ScoringPlane::new(&log_spectrogram(audio, spec), &p.score);
    let catalog = MemCatalog::new(tracks);
    let inp = SearchInput { plane: &plane, events, catalog: &catalog, region, duration_s: audio.duration_s(), dem };
    search(&inp, p)
}
