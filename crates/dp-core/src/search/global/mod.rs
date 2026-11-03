//! Region-free (CONUS) search: invert each event from the aircraft side to propose small regions,
//! then score the top proposals with the regional search. Design and evaluation:
//! docs/research/global_search_2026-11-03.md.

mod cluster;
mod footprint;
mod rerank;
mod run;
mod source;

use rayon::prelude::*;

use super::hypothesis::EventSpec;
use super::pipeline::Region;
use crate::track::Track;

pub use run::{GlobalInput, GlobalResult, RegionOutcome, locate_global};
pub use source::{StoreSource, TrackSource};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GlobalParams {
    /// Vote-lattice step (m); raised automatically for wide time priors (see `vote_cell_m`).
    pub cell_m: f64,
    /// Target lattice cells per (event, flight) footprint; bounds memory for wide priors.
    pub cells_per_pair: f64,
    /// Clock-offset bin (s) for event agreement.
    pub clock_bin_s: f64,
    /// Max clock disagreement (s) between events of one proposal.
    pub clock_tol_s: f64,
    /// Ground MSL height range assumed when converting slant range to horizontal offset (no national DEM).
    pub ground_msl_m: (f64, f64),
    pub mic_height_m: f64,
    pub c: f64,
    pub max_dt_s: f64,
    /// Cells explaining `n_max − event_slack` events are kept when that is still ≥ 2.
    pub event_slack: usize,
    pub min_radius_m: f64,
    pub max_radius_m: f64,
    pub margin_m: f64,
    /// Proposals handed to the regional search. The rerank is weak (Oyster Bay truth #17–19); the regional
    /// score is what separates truth, so K is generous (global_search_2026-11-03.md).
    pub top_k: usize,
    /// Proposals kept (and reranked by spectrogram score) before the top_k are searched.
    pub keep_proposals: usize,
    /// Track bbox (lat_min, lon_min, lat_max, lon_max); the store is worldwide.
    pub bbox: Option<(f64, f64, f64, f64)>,
    /// Half-width (m) of the 3×3 offset grid tried around each proposal centre by `rerank`.
    pub rerank_offset_m: f64,
    /// Proposals (geometry order) that get the spectrogram rerank; ~0.2 s each on an hour of audio.
    pub rerank_max: usize,
}

impl Default for GlobalParams {
    fn default() -> Self {
        Self {
            cell_m: 500.0,
            cells_per_pair: 2_000.0,
            clock_bin_s: 4.0,
            clock_tol_s: 10.0,
            ground_msl_m: (0.0, 800.0),
            mic_height_m: 1.5,
            c: 343.0,
            max_dt_s: 2.0,
            event_slack: 1,
            min_radius_m: 1_500.0,
            max_radius_m: 5_000.0,
            margin_m: 750.0,
            top_k: 30,
            keep_proposals: 5_000,
            bbox: Some((24.0, -125.5, 50.0, -66.0)),
            rerank_offset_m: 300.0,
            rerank_max: 1_000,
        }
    }
}

/// A candidate region for the regional search, from agreeing (event, flight) footprints.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RegionProposal {
    pub region: Region,
    /// Sum over explained events of the best footprint weight (d/v plausibility, ≤ 1 each).
    pub score: f64,
    pub n_events: usize,
    /// Recording-start (UNIX = audio + clock) range implied by the supporting footprints.
    pub clock_range: (f64, f64),
    /// Supporting track ids per event (empty = event unexplained here).
    pub flights: Vec<Vec<String>>,
    /// Indices of `flights` into the proposal-stage window tracks.
    #[serde(skip)]
    pub tracks: Vec<Vec<u32>>,
    /// Spectrogram score (z) of the supporting flights from the region centre (`rerank`).
    pub quick: Option<f64>,
    pub n_cells: usize,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ProposalStats {
    pub tracks: usize,
    pub votes: usize,
    pub cells: usize,
    pub n_max: usize,
    pub vote_cell_m: f64,
    pub n_proposals: usize,
}

/// Lattice step: `cell_m`, coarsened so one footprint (along-track window × both perpendicular sides)
/// covers about `cells_per_pair` cells.
pub fn vote_cell_m(events: &[EventSpec], prior: (f64, f64), p: &GlobalParams) -> f64 {
    let area = events
        .iter()
        .map(|e| {
            let (lo, hi) = footprint::emission_window(e, prior, p.c);
            (hi - lo).max(0.0) * e.v_range.1 * 4.0 * e.d_range.1
        })
        .fold(0.0, f64::max);
    p.cell_m.max((area / p.cells_per_pair).sqrt()).ceil()
}

/// Rank regions where the observer could be, given only the events, the window's tracks and the
/// recording-start prior. `tracks` must cover every event's emission window (`emission_windows`).
pub fn propose_regions(
    events: &[EventSpec],
    tracks: &[Track],
    prior: (f64, f64),
    p: &GlobalParams,
) -> (Vec<RegionProposal>, ProposalStats) {
    let cell = vote_cell_m(events, prior, p);
    let mut votes: Vec<footprint::Vote> = tracks
        .par_iter()
        .enumerate()
        .flat_map_iter(|(ti, tr)| {
            let mut out = Vec::new();
            for (ei, e) in events.iter().enumerate() {
                footprint::track_votes(e, ei as u16, tr, ti as u32, prior, cell, p, &mut out);
            }
            out
        })
        .collect();
    let n_votes = votes.len();
    let cells = cluster::best_per_cell(&mut votes, events.len(), p);
    let (props, n_max, n_proposals) = cluster::proposals(&cells, &votes, tracks, events.len(), prior, cell, p);
    let stats = ProposalStats {
        tracks: tracks.len(),
        votes: n_votes,
        cells: cells.len(),
        n_max,
        vote_cell_m: cell,
        n_proposals,
    };
    (props, stats)
}

/// Emission-time windows (UNIX s) in which each event's aircraft must have been at CPA.
pub fn emission_windows(events: &[EventSpec], prior: (f64, f64), p: &GlobalParams) -> Vec<(f64, f64)> {
    events.iter().map(|e| footprint::emission_window(e, prior, p.c)).collect()
}

#[cfg(test)]
mod tests;
