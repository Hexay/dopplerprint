//! Ground elevation and geoid undulation lookups. Rasters are prepared offline by
//! python/doppler_print/prep/dem.py into an EPSG:5070-aligned, memory-mapped u16 grid (format: docs/GEO.md).

mod geoid;
mod raster;

pub use geoid::geoid_undulation;
pub use raster::{Dem, DemHeader};
