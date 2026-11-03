//! OGC GeoPackage 1.4 writer for QGIS. Approach ported from thunderprint's `output/gpkg.rs` (MIT, (c) 2026 Hexay):
//! hand-written core tables + GP/WKB blobs, extended with EPSG:5070, LineStringZ, R*Tree and `layer_styles`.

mod layer;
pub mod layers;
mod schema;
pub mod wkb;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

pub use layer::{GeomSpec, Layer, write_layer};
pub use schema::{ALBERS, WGS84};

use crate::output::features::Results;

/// Writes `layers` to a fresh GeoPackage at `path` (an existing file is replaced).
pub fn write_layers(path: &Path, layers: &[Layer]) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path).with_context(|| format!("replacing {}", path.display()))?;
    }
    let mut db = Connection::open(path)?;
    db.execute_batch(schema::CORE)?;
    for (name, id, wkt, desc) in schema::SRS {
        db.execute("INSERT INTO gpkg_spatial_ref_sys VALUES (?1, ?2, 'EPSG', ?2, ?3, ?4)", (name, id, wkt, desc))?;
    }
    let tx = db.transaction()?;
    for l in layers {
        write_layer(&tx, l).with_context(|| format!("layer {}", l.name))?;
    }
    tx.commit()?;
    Ok(())
}

/// The standard result GeoPackage: cells, candidates, tracks, events, regions.
pub fn write(path: &Path, r: &Results) -> Result<()> {
    write_layers(path, &[layers::cells(r), layers::candidates(r), layers::tracks(r), layers::events(r), layers::regions(r)])
}

#[cfg(test)]
mod tests;
