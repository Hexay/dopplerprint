//! Result writers: GeoPackage (QGIS), GeoJSON and the single-file HTML viewer, all fed by `features::Results`.

pub mod features;
pub mod geojson;
pub mod gpkg;
pub mod png;
pub mod sample;
pub mod viewer;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub use features::Results;
pub use viewer::SpectrogramImage;

/// Writes `results.gpkg`, `results.geojson` and `viewer.html` into `dir` (created if missing).
pub fn write_all(dir: &Path, r: &Results, spec: Option<&SpectrogramImage>) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let (g, j, v) = (dir.join("results.gpkg"), dir.join("results.geojson"), dir.join("viewer.html"));
    gpkg::write(&g, r)?;
    geojson::write(&j, r)?;
    viewer::write(&v, r, spec)?;
    Ok(vec![g, j, v])
}

#[cfg(test)]
mod tests {
    /// `cargo test -p dp-core write_demo_outputs -- --ignored` writes target/demo_outputs/ for QGIS/ogrinfo checks.
    #[test]
    #[ignore]
    fn write_demo_outputs() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/demo_outputs");
        let files = super::write_all(&dir, &super::sample::demo_results(), None).unwrap();
        println!("{files:?}");
    }
}
