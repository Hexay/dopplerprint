//! Drop non-ICAO (`~`, TIS-B / anonymous) tracks that shadow an ICAO track: FAA radar rebroadcasts of
//! aircraft already seen on ADS-B (docs/research/adsb_data.md §2.4). Unmatched `~` tracks stay as audible traffic.

use std::collections::HashMap;

use super::index::SegmentIndex;
use super::model::NON_ICAO_BIT;
use crate::track::Track;

#[derive(Clone, Copy, Debug)]
pub struct DedupParams {
    /// TIS-B positions are radar-derived (coarse, 5–12 s), so the match radius is generous.
    pub max_dist_m: f64,
    /// Fraction of the `~` track's points that must sit on one ICAO track.
    pub min_match_frac: f64,
}

impl Default for DedupParams {
    fn default() -> Self {
        Self { max_dist_m: 1500.0, min_match_frac: 0.8 }
    }
}

fn is_non_icao(t: &Track) -> bool {
    t.icao24 & NON_ICAO_BIT != 0
}

/// Returns the kept tracks (order preserved) and the number dropped.
pub fn drop_shadow_tracks(tracks: Vec<Track>, p: &DedupParams) -> (Vec<Track>, usize) {
    let icao: Vec<usize> = (0..tracks.len()).filter(|&i| !is_non_icao(&tracks[i])).collect();
    if icao.len() == tracks.len() || icao.is_empty() {
        return (tracks, 0);
    }
    let icao_tracks: Vec<Track> = icao.iter().map(|&i| tracks[i].clone()).collect();
    let idx = SegmentIndex::from_tracks(&icao_tracks);
    let shadow: Vec<bool> = tracks
        .iter()
        .map(|t| is_non_icao(t) && shadows(t, &icao_tracks, &idx, p))
        .collect();
    let dropped = shadow.iter().filter(|s| **s).count();
    let kept = tracks.into_iter().zip(shadow).filter_map(|(t, s)| (!s).then_some(t)).collect();
    (kept, dropped)
}

fn shadows(t: &Track, icao_tracks: &[Track], idx: &SegmentIndex, p: &DedupParams) -> bool {
    let mut matches: HashMap<usize, usize> = HashMap::new();
    for pt in &t.points {
        let mut seen = Vec::new();
        for h in idx.within(pt.pos, p.max_dist_m, pt.t, pt.t) {
            let leg = &icao_tracks[h.track].points[h.seg..=h.seg + 1];
            let dt = leg[1].t - leg[0].t;
            let s = if dt > 0.0 { ((pt.t - leg[0].t) / dt).clamp(0.0, 1.0) } else { 0.0 };
            let at_t = leg[0].pos + (leg[1].pos - leg[0].pos) * s;
            if (at_t - pt.pos).norm() <= p.max_dist_m && !seen.contains(&h.track) {
                seen.push(h.track);
                *matches.entry(h.track).or_default() += 1;
            }
        }
    }
    let best = matches.values().max().copied().unwrap_or(0);
    best as f64 >= p.min_match_frac * t.points.len() as f64
}
