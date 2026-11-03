//! Ground elevation and geoid undulation lookups. Rasters are prepared offline by
//! python/doppler_print/prep/dem.py into an EPSG:5070-aligned, memory-mapped u16 grid.

use crate::geo::albers::Xy;

pub struct Dem {
    // geo agent: memmap2-backed raster + header (origin, step, nodata)
}

impl Dem {
    pub fn open(_path: &std::path::Path) -> anyhow::Result<Dem> {
        todo!("geo agent")
    }
    /// Orthometric (MSL) ground height in metres, bilinear.
    pub fn height_msl(&self, _xy: Xy) -> Option<f64> {
        todo!("geo agent")
    }
}

/// EGM2008 undulation N (m): h_ellipsoid = H_msl + N.
pub fn geoid_undulation(_lat_deg: f64, _lon_deg: f64) -> f64 {
    todo!("geo agent: embedded cropped CONUS grid")
}
