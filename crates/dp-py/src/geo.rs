use numpy::{IntoPyArray, PyArray1};
use pyo3::prelude::*;

use dp_core::geo::{Geodetic, geodetic_to_ecef};

/// ECEF (x, y, z) in metres for a geodetic position with ellipsoidal height.
#[pyfunction]
fn to_ecef(py: Python<'_>, lat_deg: f64, lon_deg: f64, h_ell_m: f64) -> Bound<'_, PyArray1<f64>> {
    let p = geodetic_to_ecef(Geodetic { lat_deg, lon_deg, h_ell_m });
    vec![p.x, p.y, p.z].into_pyarray(py)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(to_ecef, m)?)
}
