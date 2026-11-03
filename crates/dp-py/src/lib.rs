//! Python bindings. Each submodule registers its own functions; arrays cross as numpy without copies
//! where possible and long-running calls release the GIL.

mod app;
mod audio;
mod geo;
mod hum;
mod physics;
mod search;
mod tracks;

use pyo3::prelude::*;

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    app::register(m)?;
    audio::register(m)?;
    geo::register(m)?;
    hum::register(m)?;
    physics::register(m)?;
    search::register(m)?;
    tracks::register(m)?;
    Ok(())
}
