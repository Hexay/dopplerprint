use dp_core::app::{self, ConsoleUi, LocateRequest};
use pyo3::prelude::*;

/// Run the `dopplerprint` command line with `argv` (argv[0] is the program name); returns the exit code.
#[pyfunction]
fn cli(py: Python<'_>, argv: Vec<String>) -> u8 {
    py.detach(|| dp_cli::run(argv))
}

/// `dopplerprint.locate`: `request` is a JSON object of `LocateRequest` fields; returns the report as JSON.
/// Never prompts: downloads need `yes=True`. Progress goes to stderr unless `quiet`.
#[pyfunction]
#[pyo3(signature = (request, quiet=false))]
fn locate_json(py: Python<'_>, request: &str, quiet: bool) -> PyResult<String> {
    let req: LocateRequest = serde_json::from_str(request).map_err(|e| pyo3::exceptions::PyTypeError::new_err(e.to_string()))?;
    let out = py.detach(|| -> anyhow::Result<String> {
        let report = app::locate(&req, &ConsoleUi::new(false, quiet))?;
        Ok(serde_json::to_string(&report)?)
    })?;
    Ok(out)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(cli, m)?)?;
    m.add_function(wrap_pyfunction!(locate_json, m)?)
}
