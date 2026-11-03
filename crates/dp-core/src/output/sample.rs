//! Synthetic `Results` around Oyster Bay for writer tests and for eyeballing the QGIS/viewer styling.

use super::features::*;
use crate::geo::albers::{self, Cell};

pub fn demo_results() -> Results {
    let (lat, lon) = (40.876466, -73.524951);
    let centre = Cell::of(albers::forward(lat, lon), 200);
    let score = |dx: i32, dy: i32| 20.0 - ((dx * dx + dy * dy) as f64).sqrt() * 2.0;
    let cells = (-5..=5)
        .flat_map(|dx| (-5..=5).map(move |dy| (dx, dy)))
        .map(|(dx, dy)| {
            let s = score(dx, dy);
            CellRow {
                cell: Cell { ix: centre.ix + dx, iy: centre.iy + dy, step_m: 200 },
                score: s,
                event_scores: vec![s * 0.6, s * 0.4],
                level: 0,
                in_cr90: dx.abs() <= 1 && dy.abs() <= 1,
            }
        })
        .collect();
    let candidates = (0..3)
        .map(|i| CandidateRow {
            rank: i + 1,
            lat_deg: lat + 0.004 * i as f64,
            lon_deg: lon - 0.006 * i as f64,
            score: 20.48 - 3.0 * i as f64,
            event_scores: vec![12.0 - i as f64, 8.48 - 2.0 * i as f64],
            flight_ids: vec!["a1dfeb:1".into(), "ace391:0".into()],
            clock_offset_s: 1788898649.662,
            posterior: [0.7, 0.2, 0.1][i as usize],
            reliable: i == 0,
        })
        .collect();
    let track = |id: &str, ty: &str, engine: &str, dlat: f64, dlon: f64, t0: f64| TrackRow {
        id: format!("{id}:0"),
        icao24: id.into(),
        type_code: ty.into(),
        engine: engine.into(),
        t_start: t0,
        t_end: t0 + 600.0,
        points: (0..=20).map(|k| [lat - 0.1 + dlat * k as f64, lon - 0.15 + dlon * k as f64, 900.0 + 5.0 * k as f64]).collect(),
    };
    let events = vec![
        EventRow { index: 0, name: "event1_prop".into(), t_cpa_s: 2155.7, f0_hz: 112.4, band_lo_hz: 75.0, band_hi_hz: 190.0, snr_z: 14.0, flight_id: Some("a1dfeb:1".into()) },
        EventRow { index: 1, name: "event2_prop".into(), t_cpa_s: 3076.4, f0_hz: 96.0, band_lo_hz: 60.0, band_hi_hz: 160.0, snr_z: 9.5, flight_id: Some("ace391:0".into()) },
    ];
    let regions = vec![
        RegionRow { rank: 1, kind: RegionKind::Ellipse, mass: 0.68, area_km2: 0.38, ring: ellipse_ring(lat, lon, 450.0, 270.0, 35.0, 48) },
        RegionRow { rank: 1, kind: RegionKind::Credible90, mass: 0.9, area_km2: 0.36, ring: ellipse_ring(lat, lon, 400.0, 300.0, 0.0, 6) },
    ];
    Results {
        run: RunInfo { audio: "demo.wav".into(), date_utc: "2026-09-08".into(), version: env!("CARGO_PKG_VERSION").into(), note: "synthetic demo".into() },
        cells,
        candidates,
        tracks: vec![track("a1dfeb", "PC12", "turboprop", 0.01, 0.015, 1788900000.0), track("ace391", "B350", "turboprop", 0.008, 0.002, 1788900900.0)],
        events,
        regions,
    }
}
