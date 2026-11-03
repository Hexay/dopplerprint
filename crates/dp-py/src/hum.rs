use dp_core::audio::{self, Audio, HumParams};
use numpy::PyReadonlyArray1;
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// Mains-hum report as a dict. Keyword arguments override any HumParams field (see docs/AUDIO.md);
/// pass `labelled_sr` if `samples` were decimated from the container rate.
#[pyfunction]
#[pyo3(signature = (samples, sr, **params))]
fn analyse_hum<'py>(
    py: Python<'py>,
    samples: PyReadonlyArray1<'py, f32>,
    sr: u32,
    params: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let json = py.import("json")?;
    let p: HumParams = match params {
        Some(d) => HumParams::from_json(&json.call_method1("dumps", (d,))?.extract::<String>()?)?,
        None => HumParams::default(),
    };
    let a = Audio { sample_rate: sr, samples: samples.as_slice()?.to_vec() };
    let out = py.detach(|| audio::analyse_hum(&a, &p).to_json())?;
    json.call_method1("loads", (out,))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(analyse_hum, m)?)
}
