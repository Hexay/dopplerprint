//! Result files for one locate run: result.html (viewer), result.geojson, result.gpkg and result.json (the report).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::report::{Report, Status};
use crate::audio::LogSpectrogram;
use crate::events::Event;
use crate::geo::ecef_to_geodetic;
use crate::output::features::{CandidateRow, CellRow, EventRow, Results, RunInfo, TrackRow};
use crate::output::{geojson, gpkg, png, viewer};
use crate::search::pipeline::Candidate;
use crate::track::Track;

pub struct ResultInput<'a> {
    pub report: &'a Report,
    /// Every scored cell, best first.
    pub ranked: &'a [Candidate],
    pub coarse_step_m: u32,
    /// The events the search used, in search order.
    pub events: &'a [Event],
    pub tracks: &'a [Track],
    /// UNIX window of track points to draw.
    pub window: (f64, f64),
    pub spec: Option<&'a LogSpectrogram>,
}

pub const FILES: [&str; 4] = ["result.html", "result.geojson", "result.gpkg", "result.json"];

pub fn paths(dir: &Path) -> Vec<PathBuf> {
    FILES.iter().map(|f| dir.join(f)).collect()
}

/// Writes the four files; `report.files` should already list them.
pub fn write(dir: &Path, inp: &ResultInput<'_>) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let r = results(inp);
    let img = inp.spec.map(|s| png::spectrogram_image(s, 2400));
    let p = paths(dir);
    viewer::write(&p[0], &r, img.as_ref())?;
    geojson::write(&p[1], &r)?;
    gpkg::write(&p[2], &r)?;
    std::fs::write(&p[3], serde_json::to_vec_pretty(inp.report)?)?;
    Ok(())
}

pub fn results(inp: &ResultInput<'_>) -> Results {
    let rep = inp.report;
    let cells = inp
        .ranked
        .iter()
        .map(|c| CellRow {
            cell: c.cell,
            score: c.score,
            event_scores: c.events.iter().map(|e| e.z).collect(),
            level: (inp.coarse_step_m / c.cell.step_m.max(1)).max(1).ilog2() as u8,
            in_cr90: false,
        })
        .collect();
    let picked: Vec<&Candidate> = inp.ranked.first().into_iter().chain(rep.alternatives.iter().map(|a| &inp.ranked[a.rank])).collect();
    let weights = softmax(picked.iter().map(|c| c.score));
    let candidates = picked
        .iter()
        .zip(weights)
        .enumerate()
        .map(|(i, (c, w))| CandidateRow {
            rank: i as u32 + 1,
            lat_deg: c.lat_deg,
            lon_deg: c.lon_deg,
            score: c.score,
            event_scores: c.events.iter().map(|e| e.z).collect(),
            flight_ids: c.track_ids.clone(),
            clock_offset_s: c.clock,
            posterior: w,
            reliable: i == 0 && rep.status == Status::Found,
        })
        .collect();
    let best_ids = inp.ranked.first().map(|c| c.track_ids.clone()).unwrap_or_default();
    let events = inp
        .events
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let h = e.ridge_harmonic.max(1) as f64;
            let (lo, hi) = e.ridge.f_hz.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &f| (a.min(f), b.max(f)));
            EventRow {
                index: i as u32,
                name: format!("pass {}", i + 1),
                t_cpa_s: e.fit.best.t_cpa_arrive,
                f0_hz: e.fit.best.f0 / h,
                band_lo_hz: lo,
                band_hi_hz: hi,
                snr_z: e.snr,
                flight_id: best_ids.get(i).cloned(),
            }
        })
        .collect();
    let tracks = inp.tracks.iter().filter_map(|t| track_row(t, inp.window)).collect();
    let name = Path::new(&rep.audio).file_name().map_or(rep.audio.clone(), |n| n.to_string_lossy().into_owned());
    let note = match &rep.location {
        Some(l) => format!("{:?}: {:.5}, {:.5}", rep.status, l.lat, l.lon).to_lowercase(),
        None => format!("{:?}", rep.status).to_lowercase(),
    };
    Results {
        run: RunInfo {
            audio: name,
            date_utc: rep.clock.as_ref().map(|c| c.start_utc.clone()).unwrap_or_default(),
            version: env!("CARGO_PKG_VERSION").into(),
            note,
        },
        cells,
        candidates,
        tracks,
        events,
        regions: Vec::new(),
    }
}

fn track_row(t: &Track, (lo, hi): (f64, f64)) -> Option<TrackRow> {
    let points: Vec<[f64; 3]> = t
        .points
        .iter()
        .filter(|p| p.t >= lo && p.t <= hi)
        .map(|p| {
            let g = ecef_to_geodetic(p.pos);
            [g.lat_deg, g.lon_deg, g.h_ell_m]
        })
        .collect();
    (points.len() >= 2).then(|| TrackRow {
        id: t.id.clone(),
        icao24: super::report::icao_hex(t.icao24),
        type_code: t.type_code.clone().unwrap_or_default(),
        engine: format!("{:?}", t.engine).to_lowercase(),
        t_start: t.t_start().max(lo),
        t_end: t.t_end().min(hi),
        points,
    })
}

/// Relative weight of each listed spot (softmax of score); uncalibrated, for display only.
fn softmax(scores: impl Iterator<Item = f64> + Clone) -> Vec<f64> {
    let top = scores.clone().fold(f64::NEG_INFINITY, f64::max);
    let w: Vec<f64> = scores.map(|s| (s - top).exp()).collect();
    let sum: f64 = w.iter().sum();
    w.into_iter().map(|x| x / sum).collect()
}
