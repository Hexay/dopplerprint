//! Where national-search tracks come from: the ADS-B store, optionally with injected tracks (sim eval).

use std::path::PathBuf;

use super::GlobalParams;
use crate::adsb::{self, TrackQuery};
use crate::geo::ecef_to_geodetic;
use crate::search::hypothesis::EventSpec;
use crate::track::Track;

pub trait TrackSource: Sync {
    fn load(&self, q: &TrackQuery) -> anyhow::Result<Vec<Track>>;
}

/// The v2 store plus `extra` tracks, which are returned whenever they overlap the query.
pub struct StoreSource {
    pub store: PathBuf,
    pub extra: Vec<Track>,
}

impl TrackSource for StoreSource {
    fn load(&self, q: &TrackQuery) -> anyhow::Result<Vec<Track>> {
        let mut out = adsb::load_tracks(&self.store, q)?;
        out.extend(self.extra.iter().filter(|t| matches(t, q)).cloned());
        Ok(out)
    }
}

fn matches(t: &Track, q: &TrackQuery) -> bool {
    let in_time = t.t_end() >= q.t_start && t.t_start() <= q.t_end;
    in_time
        && q.bbox.is_none_or(|(la0, lo0, la1, lo1)| {
            t.points.iter().any(|p| {
                let g = ecef_to_geodetic(p.pos);
                (la0..=la1).contains(&g.lat_deg) && (lo0..=lo1).contains(&g.lon_deg)
            })
        })
}

/// Every track in `p.bbox` that could be an event's source, over its emission window and its scored
/// audio window: one store query per merged span, capped at the highest altitude any slant range allows.
pub fn window_tracks(src: &dyn TrackSource, events: &[EventSpec], prior: (f64, f64), p: &GlobalParams) -> anyhow::Result<Vec<Track>> {
    let mut wins: Vec<(f64, f64)> = super::emission_windows(events, prior, p)
        .into_iter()
        .zip(events)
        .map(|(w, e)| (w.0.min(e.window.0 + prior.0 - e.d_range.1 / p.c), w.1.max(e.window.1 + prior.1)))
        .collect();
    wins.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for w in wins {
        match merged.last_mut() {
            Some(m) if w.0 <= m.1 => m.1 = m.1.max(w.1),
            _ => merged.push(w),
        }
    }
    let d_max = events.iter().map(|e| e.d_range.1).fold(0.0, f64::max);
    let mut out = Vec::new();
    for (t_start, t_end) in merged {
        let q = TrackQuery {
            t_start,
            t_end,
            bbox: p.bbox,
            max_alt_m: Some(d_max + p.ground_msl_m.1),
            exclude_engines: Vec::new(),
        };
        out.extend(src.load(&q)?);
    }
    Ok(out)
}
