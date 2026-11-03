//! One-call search for a recording: whitened scoring plane + in-memory catalogue + coarse-to-fine search.

use super::hypothesis::EventSpec;
use super::pipeline::{Region, SearchInput, SearchParams, SearchResult, search};
use super::score::ScoringPlane;
use super::{MemCatalog, Scorer};
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
            t_cpa_halfwidth_s: 0.5 * (e.fit.t_cpa_range.1 - e.fit.t_cpa_range.0),
            kappa_range: e.fit.kappa_range,
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

    /// No cell beats the all-unexplained null, so no location should be reported.
    pub fn abstains(&self) -> bool {
        self.null_score.is_some_and(|n| self.ranked.first().is_none_or(|b| b.score <= n))
    }
}

/// `p.assoc.t_start_prior` and `p.flat_h_ell_m` must already be set by the caller.
/// Fails only when `p.backend` is `Cuda` and no usable GPU is present.
pub fn locate(
    audio: &Audio,
    events: &[EventSpec],
    tracks: Vec<Track>,
    region: Region,
    dem: Option<&Dem>,
    spec: &SpecParams,
    p: &SearchParams,
) -> anyhow::Result<SearchResult> {
    let plane = ScoringPlane::build(audio, spec, &p.score);
    let scorer = Scorer::new(&plane, p.backend)?;
    let catalog = MemCatalog::new(tracks);
    let inp = SearchInput { scorer: &scorer, events, catalog: &catalog, region, duration_s: audio.duration_s(), dem };
    let mut p = p.clone();
    p.assoc.silence.measure_recording(audio, spec);
    Ok(search(&inp, &p))
}
