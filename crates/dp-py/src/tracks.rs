use std::path::PathBuf;

use dp_core::adsb::{self, TrackQuery, model::engine_from_name, model::format_icao};
use dp_core::geo::{Geodetic, ecef_to_geodetic};
use dp_core::track::Track;
use numpy::{PyArray1, PyArrayMethods};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

// TODO: return the physics agent's Track pyclass once it lands; dicts of numpy arrays until then.
fn track_dict<'py>(py: Python<'py>, tr: &Track) -> PyResult<Bound<'py, PyDict>> {
    let n = tr.points.len();
    let t: Vec<f64> = tr.points.iter().map(|p| p.t).collect();
    let pos: Vec<f64> = tr.points.iter().flat_map(|p| [p.pos.x, p.pos.y, p.pos.z]).collect();
    let vel: Vec<f64> = tr.points.iter().flat_map(|p| [p.vel.x, p.vel.y, p.vel.z]).collect();
    let geo: Vec<Geodetic> = tr.points.iter().map(|p| ecef_to_geodetic(p.pos)).collect();
    let d = PyDict::new(py);
    d.set_item("id", &tr.id)?;
    d.set_item("icao24", format_icao(tr.icao24))?;
    d.set_item("type_code", tr.type_code.as_deref())?;
    d.set_item("engine", format!("{:?}", tr.engine).to_lowercase())?;
    d.set_item("t", PyArray1::from_vec(py, t))?;
    d.set_item("pos", PyArray1::from_vec(py, pos).reshape([n, 3])?)?;
    d.set_item("vel", PyArray1::from_vec(py, vel).reshape([n, 3])?)?;
    d.set_item("lat", PyArray1::from_iter(py, geo.iter().map(|g| g.lat_deg)))?;
    d.set_item("lon", PyArray1::from_iter(py, geo.iter().map(|g| g.lon_deg)))?;
    d.set_item("h_ell", PyArray1::from_iter(py, geo.iter().map(|g| g.h_ell_m)))?;
    Ok(d)
}

/// Tracks touching [t0, t1] (UNIX s) and bbox (lat_min, lon_min, lat_max, lon_max).
/// Each is a dict: id, icao24 (hex str), type_code, engine, t (N,), pos (N,3) and vel (N,3) ECEF m, m/s,
/// and lat, lon (deg), h_ell (m) per point.
/// `dedup` drops TIS-B/anonymous `~` tracks that shadow an ADS-B track.
#[pyfunction]
#[pyo3(signature = (store, t0, t1, bbox=None, exclude=Vec::new(), max_alt_m=None, dedup=true))]
#[allow(clippy::too_many_arguments)]
fn load_tracks<'py>(
    py: Python<'py>,
    store: PathBuf,
    t0: f64,
    t1: f64,
    bbox: Option<(f64, f64, f64, f64)>,
    exclude: Vec<String>,
    max_alt_m: Option<f64>,
    dedup: bool,
) -> PyResult<Vec<Bound<'py, PyDict>>> {
    let exclude_engines = exclude
        .iter()
        .map(|e| engine_from_name(e).ok_or_else(|| PyValueError::new_err(format!("unknown engine class {e:?}"))))
        .collect::<PyResult<_>>()?;
    let q = TrackQuery { t_start: t0, t_end: t1, bbox, max_alt_m, exclude_engines };
    let opts = adsb::LoadOptions { dedup: dedup.then(Default::default) };
    let tracks = py.detach(|| adsb::load_tracks_with(&store, &q, &opts))?;
    tracks.iter().map(|t| track_dict(py, t)).collect()
}

/// Ingest one adsb.lol day archive into `out`; returns the manifest as a JSON string.
#[pyfunction]
#[pyo3(signature = (archive, out, typedb=None))]
fn ingest_adsblol_day(py: Python<'_>, archive: PathBuf, out: PathBuf, typedb: Option<PathBuf>) -> PyResult<String> {
    let mut opts = adsb::IngestOptions::default();
    if typedb.is_some() {
        opts.typedb_dir = typedb;
    }
    let mf = py.detach(|| adsb::ingest_day(&archive, &out, &opts))?;
    Ok(mf.to_json())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(load_tracks, m)?)?;
    m.add_function(wrap_pyfunction!(ingest_adsblol_day, m)?)?;
    Ok(())
}
