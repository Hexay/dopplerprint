use numpy::{IntoPyArray, PyArray1, PyReadonlyArray1, PyReadonlyArray2};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use dp_core::geo::Vec3;
use dp_core::physics::{self, Atmosphere, cv};
use dp_core::sim;
use dp_core::track::{EngineClass, Track, TrackPoint};

/// Immutable handle to a dp-core `Track` (ECEF metres, UNIX seconds).
#[pyclass(name = "Track", frozen, module = "doppler_print._core")]
pub struct PyTrack {
    pub inner: Track,
}

#[pymethods]
impl PyTrack {
    #[getter]
    fn id(&self) -> &str {
        &self.inner.id
    }
    #[getter]
    fn t_start(&self) -> f64 {
        self.inner.t_start()
    }
    #[getter]
    fn t_end(&self) -> f64 {
        self.inner.t_end()
    }
    fn __len__(&self) -> usize {
        self.inner.points.len()
    }
    /// ((x, y, z), (vx, vy, vz)) at `t`, or None outside the track.
    fn state_at(&self, t: f64) -> Option<([f64; 3], [f64; 3])> {
        self.inner.state_at(t).map(|s| (xyz(s.pos), xyz(s.vel)))
    }
}

fn xyz(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn vec3(a: &PyReadonlyArray1<'_, f64>) -> PyResult<Vec3> {
    match a.as_slice()? {
        [x, y, z] => Ok(Vec3::new(*x, *y, *z)),
        _ => Err(PyValueError::new_err("expected a length-3 xyz array")),
    }
}

fn atmosphere(c: f64, wind: Option<PyReadonlyArray1<'_, f64>>) -> PyResult<Atmosphere> {
    Ok(Atmosphere { c, wind: wind.as_ref().map(vec3).transpose()?.unwrap_or(Vec3::ZERO) })
}

fn engine_class(s: &str) -> PyResult<EngineClass> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "piston" => EngineClass::Piston,
        "turboprop" => EngineClass::Turboprop,
        "jet" => EngineClass::Jet,
        "helicopter" | "heli" => EngineClass::Helicopter,
        "unknown" => EngineClass::Unknown,
        _ => return Err(PyValueError::new_err(format!("unknown engine class {s:?}"))),
    })
}

/// Build a track from t (N,), pos (N, 3) and vel (N, 3) ECEF arrays; t must strictly increase.
#[pyfunction]
#[pyo3(signature = (t, pos, vel, id = String::from("track"), engine = "Unknown", icao24 = 0))]
fn make_track(
    t: PyReadonlyArray1<'_, f64>,
    pos: PyReadonlyArray2<'_, f64>,
    vel: PyReadonlyArray2<'_, f64>,
    id: String,
    engine: &str,
    icao24: u32,
) -> PyResult<PyTrack> {
    let (t, pos, vel) = (t.as_array(), pos.as_array(), vel.as_array());
    let n = t.len();
    if pos.shape() != [n, 3] || vel.shape() != [n, 3] {
        return Err(PyValueError::new_err("pos and vel must have shape (len(t), 3)"));
    }
    if !t.windows(2).into_iter().all(|w| w[1] > w[0]) {
        return Err(PyValueError::new_err("t must be strictly increasing"));
    }
    let row = |a: &numpy::ndarray::ArrayView2<'_, f64>, i| Vec3::new(a[[i, 0]], a[[i, 1]], a[[i, 2]]);
    let points = (0..n).map(|i| TrackPoint { t: t[i], pos: row(&pos, i), vel: row(&vel, i) }).collect();
    let engine = engine_class(engine)?;
    Ok(PyTrack { inner: Track { id, icao24, type_code: None, engine, points } })
}

/// Received/emitted frequency ratio at each observation time (NaN outside the track).
#[pyfunction]
#[pyo3(signature = (track, obs_xyz, t_obs, c = 343.0, wind = None))]
fn doppler_factors<'py>(
    py: Python<'py>,
    track: &Bound<'py, PyTrack>,
    obs_xyz: PyReadonlyArray1<'py, f64>,
    t_obs: PyReadonlyArray1<'py, f64>,
    c: f64,
    wind: Option<PyReadonlyArray1<'py, f64>>,
) -> PyResult<Bound<'py, PyArray1<f64>>> {
    let (obs, atm) = (vec3(&obs_xyz)?, atmosphere(c, wind)?);
    let (tr, ts) = (&track.get().inner, t_obs.as_slice()?);
    Ok(py.detach(|| physics::doppler_factors(tr, obs, ts, &atm)).into_pyarray(py))
}

/// Closed-form constant-velocity received frequency at each `t`.
#[pyfunction]
#[pyo3(signature = (f0, v, d_cpa, t_cpa_arrive, t, c = 343.0))]
fn cv_frequency<'py>(
    py: Python<'py>,
    f0: f64,
    v: f64,
    d_cpa: f64,
    t_cpa_arrive: f64,
    t: PyReadonlyArray1<'py, f64>,
    c: f64,
) -> PyResult<Bound<'py, PyArray1<f64>>> {
    let p = cv::CvParams { f0, v, d_cpa, t_cpa_arrive, c };
    Ok(t.as_array().mapv(|ti| cv::frequency(&p, ti)).into_pyarray(py))
}

/// dict(t_emit, t_arrive, slant_m, ground_speed), or None for an empty track.
#[pyfunction]
#[pyo3(signature = (track, obs_xyz, c = 343.0, wind = None))]
fn cpa<'py>(
    py: Python<'py>,
    track: &Bound<'py, PyTrack>,
    obs_xyz: PyReadonlyArray1<'py, f64>,
    c: f64,
    wind: Option<PyReadonlyArray1<'py, f64>>,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let (obs, atm) = (vec3(&obs_xyz)?, atmosphere(c, wind)?);
    let Some(r) = physics::cpa(&track.get().inner, obs, &atm) else { return Ok(None) };
    let d = PyDict::new(py);
    d.set_item("t_emit", r.t_emit)?;
    d.set_item("t_arrive", r.t_arrive)?;
    d.set_item("slant_m", r.slant_m)?;
    d.set_item("ground_speed", r.ground_speed)?;
    Ok(Some(d))
}

/// Render a scenario TOML string: (samples float32 in [-1, 1], sample_rate, truth_json).
#[pyfunction]
fn simulate<'py>(py: Python<'py>, scenario_toml: &str) -> PyResult<(Bound<'py, PyArray1<f32>>, u32, String)> {
    let r = py.detach(|| sim::render_toml(scenario_toml))?;
    let truth = r.truth.to_json().map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok((r.samples.into_pyarray(py), r.sample_rate, truth))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyTrack>()?;
    m.add_function(wrap_pyfunction!(make_track, m)?)?;
    m.add_function(wrap_pyfunction!(doppler_factors, m)?)?;
    m.add_function(wrap_pyfunction!(cv_frequency, m)?)?;
    m.add_function(wrap_pyfunction!(cpa, m)?)?;
    m.add_function(wrap_pyfunction!(simulate, m)?)
}
