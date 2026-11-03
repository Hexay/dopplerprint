use std::path::PathBuf;

use numpy::ndarray::{ArrayD, Zip};
use numpy::{IntoPyArray, PyArray1, PyArrayDyn, PyReadonlyArrayDyn, PyUntypedArrayMethods};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use dp_core::dem::{self, Dem};
use dp_core::geo::albers::{self, Xy};
use dp_core::geo::{Geodetic, geodetic_to_ecef};

type In<'py> = PyReadonlyArrayDyn<'py, f64>;
type Out<'py> = Bound<'py, PyArrayDyn<f64>>;

/// ECEF (x, y, z) in metres for a geodetic position with ellipsoidal height.
#[pyfunction]
fn to_ecef(py: Python<'_>, lat_deg: f64, lon_deg: f64, h_ell_m: f64) -> Bound<'_, PyArray1<f64>> {
    let p = geodetic_to_ecef(Geodetic { lat_deg, lon_deg, h_ell_m });
    vec![p.x, p.y, p.z].into_pyarray(py)
}

fn owned_pair(a: &In<'_>, b: &In<'_>) -> PyResult<(ArrayD<f64>, ArrayD<f64>)> {
    if a.shape() != b.shape() {
        return Err(PyValueError::new_err(format!("shape mismatch: {:?} vs {:?}", a.shape(), b.shape())));
    }
    Ok((a.as_array().to_owned(), b.as_array().to_owned()))
}

/// Element-wise `f(a, b)` over two same-shape arrays, with the GIL released.
fn map1<'py>(py: Python<'py>, a: In<'py>, b: In<'py>, f: impl Fn(f64, f64) -> f64 + Sync) -> PyResult<Out<'py>> {
    let (a, b) = owned_pair(&a, &b)?;
    let u = py.detach(|| Zip::from(&a).and(&b).map_collect(|&a, &b| f(a, b)));
    Ok(u.into_pyarray(py))
}

/// Element-wise `f(a, b) -> (u, v)` over two same-shape arrays, with the GIL released.
fn map2<'py>(
    py: Python<'py>,
    a: In<'py>,
    b: In<'py>,
    f: impl Fn(f64, f64) -> (f64, f64) + Sync,
) -> PyResult<(Out<'py>, Out<'py>)> {
    let (a, b) = owned_pair(&a, &b)?;
    let (u, v) = py.detach(|| {
        let mut u = ArrayD::<f64>::zeros(a.raw_dim());
        let mut v = ArrayD::<f64>::zeros(a.raw_dim());
        Zip::from(&mut u).and(&mut v).and(&a).and(&b).for_each(|u, v, &a, &b| (*u, *v) = f(a, b));
        (u, v)
    });
    Ok((u.into_pyarray(py), v.into_pyarray(py)))
}

/// EPSG:5070 (x, y) metres from WGS84/NAD83 latitude and longitude in degrees.
#[pyfunction]
fn albers_forward<'py>(py: Python<'py>, lat: In<'py>, lon: In<'py>) -> PyResult<(Out<'py>, Out<'py>)> {
    map2(py, lat, lon, |la, lo| {
        let p = albers::forward(la, lo);
        (p.x, p.y)
    })
}

/// (lat, lon) degrees from EPSG:5070 (x, y) metres.
#[pyfunction]
fn albers_inverse<'py>(py: Python<'py>, x: In<'py>, y: In<'py>) -> PyResult<(Out<'py>, Out<'py>)> {
    map2(py, x, y, |x, y| albers::inverse(Xy { x, y }))
}

/// EGM2008 undulation N (m), so h_ellipsoid = H_msl + N.
#[pyfunction]
fn geoid_undulation<'py>(py: Python<'py>, lat: In<'py>, lon: In<'py>) -> PyResult<Out<'py>> {
    map1(py, lat, lon, dem::geoid_undulation)
}

/// Memory-mapped EPSG:5070 ground raster from `doppler_print.prep.dem`. Heights are NaN off-raster.
#[pyclass(name = "Dem", frozen)]
struct PyDem(Dem);

#[pymethods]
impl PyDem {
    #[new]
    fn new(path: PathBuf) -> anyhow::Result<Self> {
        Ok(PyDem(Dem::open(&path)?))
    }

    /// MSL ground height (m) at EPSG:5070 (x, y), bilinear.
    fn height_msl<'py>(&self, py: Python<'py>, x: In<'py>, y: In<'py>) -> PyResult<Out<'py>> {
        map1(py, x, y, |x, y| self.0.height_msl(Xy { x, y }).unwrap_or(f64::NAN))
    }

    /// MSL ground height (m) at latitude/longitude (degrees).
    fn height_msl_latlon<'py>(&self, py: Python<'py>, lat: In<'py>, lon: In<'py>) -> PyResult<Out<'py>> {
        map1(py, lat, lon, |la, lo| self.0.height_msl_latlon(la, lo).unwrap_or(f64::NAN))
    }

    /// The JSON sidecar header (as parsed) as a string.
    fn header_json(&self) -> String {
        self.0.header.to_json()
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(to_ecef, m)?)?;
    m.add_function(wrap_pyfunction!(albers_forward, m)?)?;
    m.add_function(wrap_pyfunction!(albers_inverse, m)?)?;
    m.add_function(wrap_pyfunction!(geoid_undulation, m)?)?;
    m.add_class::<PyDem>()
}
