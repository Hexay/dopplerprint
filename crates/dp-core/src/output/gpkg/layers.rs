//! `Results` → the five GeoPackage layers. Column names are the QGIS-facing contract (see docs/GEO.md).

use rusqlite::types::Value;

use super::layer::{GeomSpec, Layer};
use super::schema::{ALBERS, WGS84};
use super::wkb::Geom;
use crate::geo::albers::Cell;
use crate::output::features::{Results, normalise};

const CELLS_QML: &str = include_str!("../../../../../assets/qml/cells.qml");
const CANDIDATES_QML: &str = include_str!("../../../../../assets/qml/candidates.qml");
const TRACKS_QML: &str = include_str!("../../../../../assets/qml/tracks.qml");
const REGIONS_QML: &str = include_str!("../../../../../assets/qml/regions.qml");

fn int(v: impl Into<i64>) -> Value {
    Value::Integer(v.into())
}
fn real(v: f64) -> Value {
    if v.is_finite() { Value::Real(v) } else { Value::Null }
}
fn text(v: impl Into<String>) -> Value {
    Value::Text(v.into())
}

/// Closed counter-clockwise EPSG:5070 square of a lattice cell.
pub fn cell_ring(c: &Cell) -> Vec<[f64; 2]> {
    let s = c.step_m as f64;
    let (x0, y0) = (c.ix as f64 * s, c.iy as f64 * s);
    vec![[x0, y0], [x0 + s, y0], [x0 + s, y0 + s], [x0, y0 + s], [x0, y0]]
}

fn n_events(r: &Results) -> usize {
    let per_row = r.cells.iter().map(|c| c.event_scores.len()).chain(r.candidates.iter().map(|c| c.event_scores.len()));
    per_row.max().unwrap_or(0).max(r.events.len())
}

fn event_columns(l: &mut Layer, n: usize, prefix: &str, ty: &'static str) {
    for i in 1..=n {
        l.column(format!("{prefix}{i}"), ty);
    }
}

fn padded<T: Clone>(v: &[T], n: usize, f: impl Fn(&T) -> Value) -> impl Iterator<Item = Value> {
    (0..n).map(move |i| v.get(i).map_or(Value::Null, &f))
}

pub fn cells(r: &Results) -> Layer {
    let spec = GeomSpec { type_name: "POLYGON", srs_id: ALBERS, z: false };
    let mut l = Layer::new("cells", "Scored lattice cells (EPSG:5070 squares)", Some(spec), Some(CELLS_QML));
    l.column("ix", "INTEGER").column("iy", "INTEGER").column("step_m", "INTEGER").column("level", "INTEGER");
    l.column("score", "DOUBLE").column("score_norm", "DOUBLE").column("in_cr90", "BOOLEAN");
    let n = n_events(r);
    event_columns(&mut l, n, "score_e", "DOUBLE");
    let norm = normalise(r.cells.iter().map(|c| c.score));
    for (c, sn) in r.cells.iter().zip(norm) {
        let mut v = vec![int(c.cell.ix), int(c.cell.iy), int(c.cell.step_m), int(c.level), real(c.score), real(sn), int(c.in_cr90)];
        v.extend(padded(&c.event_scores, n, |s| real(*s)));
        l.rows.push((Some(Geom::Polygon(vec![cell_ring(&c.cell)])), v));
    }
    l
}

pub fn candidates(r: &Results) -> Layer {
    let spec = GeomSpec { type_name: "POINT", srs_id: WGS84, z: false };
    let mut l = Layer::new("candidates", "Ranked candidate recording sites", Some(spec), Some(CANDIDATES_QML));
    l.column("rank", "INTEGER").column("score", "DOUBLE").column("score_norm", "DOUBLE").column("posterior", "DOUBLE");
    l.column("clock_offset_s", "DOUBLE").column("reliable", "BOOLEAN").column("flight_ids", "TEXT");
    let n = n_events(r);
    event_columns(&mut l, n, "score_e", "DOUBLE");
    event_columns(&mut l, n, "flight_e", "TEXT");
    let norm = normalise(r.candidates.iter().map(|c| c.score));
    for (c, sn) in r.candidates.iter().zip(norm) {
        let mut v = vec![int(c.rank), real(c.score), real(sn), real(c.posterior), real(c.clock_offset_s), int(c.reliable)];
        v.push(text(c.flight_ids.join(",")));
        v.extend(padded(&c.event_scores, n, |s| real(*s)));
        v.extend(padded(&c.flight_ids, n, |s| text(s.clone())));
        l.rows.push((Some(Geom::Point([c.lon_deg, c.lat_deg])), v));
    }
    l
}

pub fn tracks(r: &Results) -> Layer {
    let spec = GeomSpec { type_name: "LINESTRING", srs_id: WGS84, z: true };
    let mut l = Layer::new("tracks", "ADS-B tracks; Z is height above the WGS84 ellipsoid (m)", Some(spec), Some(TRACKS_QML));
    l.column("track_id", "TEXT").column("icao24", "TEXT").column("type_code", "TEXT").column("engine", "TEXT");
    l.column("t_start", "DOUBLE").column("t_end", "DOUBLE").column("n_points", "INTEGER");
    for t in &r.tracks {
        let v = vec![text(&t.id), text(&t.icao24), text(&t.type_code), text(&t.engine), real(t.t_start), real(t.t_end)];
        let geom = (t.points.len() >= 2).then(|| Geom::LineStringZ(t.points.iter().map(|p| [p[1], p[0], p[2]]).collect()));
        l.rows.push((geom, [v, vec![int(t.points.len() as i64)]].concat()));
    }
    l
}

pub fn events(r: &Results) -> Layer {
    let mut l = Layer::new("events", "Detected Doppler events (audio-relative times)", None, None);
    l.column("idx", "INTEGER").column("name", "TEXT").column("t_cpa_s", "DOUBLE").column("f0_hz", "DOUBLE");
    l.column("band_lo_hz", "DOUBLE").column("band_hi_hz", "DOUBLE").column("snr_z", "DOUBLE").column("flight_id", "TEXT");
    for e in &r.events {
        let flight = e.flight_id.clone().map_or(Value::Null, text);
        let v = vec![int(e.index), text(&e.name), real(e.t_cpa_s), real(e.f0_hz), real(e.band_lo_hz), real(e.band_hi_hz)];
        l.rows.push((None, [v, vec![real(e.snr_z), flight]].concat()));
    }
    l
}

pub fn regions(r: &Results) -> Layer {
    let spec = GeomSpec { type_name: "POLYGON", srs_id: WGS84, z: false };
    let mut l = Layer::new("regions", "90% credible regions and mode ellipses", Some(spec), Some(REGIONS_QML));
    l.column("rank", "INTEGER").column("kind", "TEXT").column("mass", "DOUBLE").column("area_km2", "DOUBLE");
    for g in &r.regions {
        let kind = g.kind.as_str();
        let ring = g.ring.iter().map(|p| [p[1], p[0]]).collect();
        l.rows.push((Some(Geom::Polygon(vec![ring])), vec![int(g.rank), text(kind), real(g.mass), real(g.area_km2)]));
    }
    l
}
