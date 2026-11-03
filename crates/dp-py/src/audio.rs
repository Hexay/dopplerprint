use std::path::PathBuf;

use dp_core::audio::{self, Audio, SpecParams, WhitenParams};
use dp_core::events::{self, DiscoverConfig, Ridge};
use numpy::{PyArray1, PyArray2, PyArrayMethods, PyReadonlyArray1};
use pyo3::prelude::*;
use pyo3::types::PyDict;

type Spectrogram<'py> = (
    Bound<'py, PyArray2<f32>>,
    Bound<'py, PyArray1<f64>>,
    Bound<'py, PyArray1<f64>>,
);

/// Decode to mono f32. With `f_max`, decimate while decoding (exact integer factor) and return that rate.
#[pyfunction]
#[pyo3(signature = (path, f_max=None))]
fn decode(py: Python<'_>, path: PathBuf, f_max: Option<f64>) -> PyResult<(Bound<'_, PyArray1<f32>>, u32)> {
    let a = py.detach(|| match f_max {
        Some(f) => audio::decode_band(&path, f).map(|(a, _)| a),
        None => audio::decode(&path),
    })?;
    Ok((PyArray1::from_vec(py, a.samples), a.sample_rate))
}

/// Container sample rate, channels, frame count and codec, as declared.
#[pyfunction]
fn probe(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyAny>> {
    json_loads(py, audio::probe(&path)?.to_json()?)
}

fn to_audio(samples: PyReadonlyArray1<'_, f32>, sr: u32) -> PyResult<Audio> {
    Ok(Audio {
        sample_rate: sr,
        samples: samples.as_slice()?.to_vec(),
    })
}

/// Whitened log-f spectrogram `[t, f]` plus its time and frequency axes. `whiten=False` gives dB power.
#[pyfunction]
#[pyo3(signature = (samples, sr, win_s=1.0, hop_s=0.25, f_min=20.0, f_max=1000.0, bpo=96, mains_hz=None, whiten=true))]
#[allow(clippy::too_many_arguments)]
fn log_spectrogram<'py>(
    py: Python<'py>,
    samples: PyReadonlyArray1<'py, f32>,
    sr: u32,
    win_s: f64,
    hop_s: f64,
    f_min: f64,
    f_max: f64,
    bpo: u32,
    mains_hz: Option<f64>,
    whiten: bool,
) -> PyResult<Spectrogram<'py>> {
    let a = to_audio(samples, sr)?;
    let p = SpecParams {
        win_s,
        hop_s,
        f_min,
        f_max,
        bins_per_octave: bpo,
    };
    let w = WhitenParams {
        mains_hz,
        ..Default::default()
    };
    let spec = py.detach(|| {
        if whiten {
            audio::log_spectrogram_with(&a, &p, &w)
        } else {
            audio::log_power_spectrogram(&a, &p)
        }
    });
    let (times, freqs) = (spec.times(), spec.freqs());
    let img = PyArray1::from_vec(py, spec.data).reshape([spec.n_t, spec.n_f])?;
    Ok((img, PyArray1::from_vec(py, times), PyArray1::from_vec(py, freqs)))
}

fn json_loads<'py>(py: Python<'py>, s: String) -> PyResult<Bound<'py, PyAny>> {
    py.import("json")?.call_method1("loads", (s,))
}

/// Discover Doppler events. Keyword arguments override any FrontEnd / DiscoverParams field (see docs/AUDIO.md).
#[pyfunction]
#[pyo3(signature = (samples, sr, **params))]
fn discover<'py>(
    py: Python<'py>,
    samples: PyReadonlyArray1<'py, f32>,
    sr: u32,
    params: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let cfg = match params {
        Some(d) => DiscoverConfig::from_json(&py.import("json")?.call_method1("dumps", (d,))?.extract::<String>()?)?,
        None => DiscoverConfig::default(),
    };
    let a = to_audio(samples, sr)?;
    let json = py.detach(|| events::events_to_json(&events::discover_audio(&a, &cfg)))?;
    json_loads(py, json)
}

/// Robust constant-velocity fit of a ridge (audio s, Hz). Returns the CvFit as a dict.
#[pyfunction]
#[pyo3(signature = (t, f, c=343.0))]
fn fit_cv<'py>(
    py: Python<'py>,
    t: PyReadonlyArray1<'py, f64>,
    f: PyReadonlyArray1<'py, f64>,
    c: f64,
) -> PyResult<Bound<'py, PyAny>> {
    let ridge = Ridge {
        t: t.as_slice()?.to_vec(),
        f_hz: f.as_slice()?.to_vec(),
        snr: vec![1.0; t.len()?],
    };
    let fit = py.detach(|| events::fit_cv(&ridge, c));
    json_loads(py, events::fit_to_json(&fit)?)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(decode, m)?)?;
    m.add_function(wrap_pyfunction!(probe, m)?)?;
    m.add_function(wrap_pyfunction!(log_spectrogram, m)?)?;
    m.add_function(wrap_pyfunction!(discover, m)?)?;
    m.add_function(wrap_pyfunction!(fit_cv, m)?)?;
    Ok(())
}
