//! Ground elevation and geoid undulation lookups on an EPSG:5070-aligned, memory-mapped u16 grid
//! (format: docs/GEO.md). Rasters come from `ensure_dem` (downloads 1° tiles on demand) or from the bulk
//! Python prep, python/dopplerprint/prep/dem.py.

#[cfg(feature = "net")]
mod download;
mod fetch;
mod geoid;
mod geotiff;
mod grid;
mod raster;
mod resolve;
mod tiles;
mod warp;
mod write;

pub use fetch::{DemFetchOptions, DemProgress, ensure_dem};
pub use geoid::geoid_undulation;
pub use geotiff::SourceRaster;
pub use grid::{Grid, expand_bbox};
pub use raster::{Dem, DemHeader};
pub use resolve::{DownloadPlan, PlannedFile};
pub use tiles::{DEFAULT_SOURCES, DemSource, Tile, tiles_for_bbox};
pub use warp::{sample_tiles, warp_tile};
