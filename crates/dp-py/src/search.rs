use std::path::PathBuf;

use dp_core::adsb::{self, TrackQuery};
use dp_core::audio::Audio;
use dp_core::config::Config;
use dp_core::dem::{Dem, geoid_undulation};
use dp_core::events::Event;
use dp_core::search::{EventSpec, Region, SearchResult, likelihood, locate};
use numpy::ndarray::{ArrayD, IxDyn};
use numpy::{IntoPyArray, PyArrayDyn, PyReadonlyArray1, PyReadonlyArrayDyn, PyUntypedArrayMethods};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

/// Coarse-to-fine search for one recording. `events` are `discover` dicts; tracks come from the ADS-B
/// `store` for [t0, t1] UNIX s in `bbox` (lat_min, lon_min, lat_max, lon_max). `region` = (lat, lon, radius_m);
/// `t_start_prior` = recording-start bounds (UNIX s). `config` is an experiment TOML string (spec + search).
/// Returns the SearchResult dict: `ranked` holds every scored cell at every level, best first.
#[pyfunction]
#[pyo3(signature = (samples, sr, events, store, t0, t1, region, t_start_prior, bbox=None, max_alt_m=Some(9_000.0), dem=None, config=None))]
#[allow(clippy::too_many_arguments)]
fn search<'py>(
    py: Python<'py>,
    samples: PyReadonlyArray1<'py, f32>,
    sr: u32,
    events: &Bound<'py, PyList>,
    store: PathBuf,
    t0: f64,
    t1: f64,
    region: (f64, f64, f64),
    t_start_prior: (f64, f64),
    bbox: Option<(f64, f64, f64, f64)>,
    max_alt_m: Option<f64>,
    dem: Option<PathBuf>,
    config: Option<&str>,
) -> PyResult<Bound<'py, PyAny>> {
    let json = py.import("json")?;
    let events: Vec<Event> = events_from_py(&json, events)?;
    let specs: Vec<EventSpec> = events.iter().map(EventSpec::from_event).collect();
    let cfg = config.map(Config::from_toml).transpose()?.unwrap_or_default();
    let audio = Audio { sample_rate: sr, samples: samples.as_slice()?.to_vec() };
    let (lat, lon, radius_m) = region;
    let mut p = cfg.search.clone();
    p.assoc.t_start_prior = t_start_prior;
    p.flat_h_ell_m = geoid_undulation(lat, lon);
    let q = TrackQuery { t_start: t0, t_end: t1, bbox, max_alt_m, exclude_engines: Vec::new() };
    let out = py.detach(|| -> anyhow::Result<String> {
        let dem = dem.map(|d| Dem::open(&d)).transpose()?;
        let tracks = adsb::load_tracks(&store, &q)?;
        let region = Region { lat_deg: lat, lon_deg: lon, radius_m };
        locate(&audio, &specs, tracks, region, dem.as_ref(), &cfg.spec, &p)?.to_json()
    })?;
    json.call_method1("loads", (out,))
}

/// Relative likelihood exp((s − s_max)/T) at each (lat, lon), from the finest scored cell of a `search`
/// result there (0 where unscored), plus the likelihood level enclosing each of `masses` (highest-density
/// regions over the given equal-area samples; nominal masses, not the conformal q̂). `temperature`
/// defaults to the fitted one (dp_core::search::calibration).
#[pyfunction]
#[pyo3(signature = (result, lat, lon, temperature=None, masses=vec![0.5, 0.9]))]
fn likelihood_grid<'py>(
    py: Python<'py>,
    result: &Bound<'py, PyDict>,
    lat: PyReadonlyArrayDyn<'py, f64>,
    lon: PyReadonlyArrayDyn<'py, f64>,
    temperature: Option<f64>,
    masses: Vec<f64>,
) -> PyResult<(Bound<'py, PyArrayDyn<f64>>, Vec<f64>)> {
    let temperature = temperature.unwrap_or_else(likelihood::default_temperature);
    if lat.shape() != lon.shape() {
        return Err(PyValueError::new_err("lat and lon must have the same shape"));
    }
    let s: String = py.import("json")?.call_method1("dumps", (result,))?.extract()?;
    let res = SearchResult::from_json(&s)?;
    let shape = lat.shape().to_vec();
    let pts: Vec<(f64, f64)> = lat.as_array().iter().zip(lon.as_array().iter()).map(|(a, b)| (*a, *b)).collect();
    let (lik, levels) = py.detach(|| {
        let lik = likelihood::relative_likelihood(&likelihood::finest_score_at(&res.ranked, &pts), temperature);
        let levels = masses.iter().map(|&m| likelihood::hpd_level(&lik, &[], m)).collect::<Vec<_>>();
        (lik, levels)
    });
    let arr = ArrayD::from_shape_vec(IxDyn(&shape), lik).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok((arr.into_pyarray(py), levels))
}

fn events_from_py(json: &Bound<'_, PyModule>, v: &Bound<'_, PyList>) -> PyResult<Vec<Event>> {
    let s: String = json.call_method1("dumps", (v,))?.extract()?;
    Ok(dp_core::events::events_from_json(&s)?)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(search, m)?)?;
    m.add_function(wrap_pyfunction!(likelihood_grid, m)?)
}
