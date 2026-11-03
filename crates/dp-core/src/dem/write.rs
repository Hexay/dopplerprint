//! Writes a `dp-dem-u16-v1` raster (`<key>.u16` + `<key>.json`), the same layout and header fields as
//! python/dopplerprint/prep/dem.py.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::grid::Grid;
use super::raster::FORMAT;

pub const SCALE_M: f64 = 0.1;
pub const OFFSET_M: f64 = -100.0;
pub const NODATA: u16 = 0;

/// Quantises a height (m MSL); NaN becomes nodata and the range clips to −99.9 … 6453.5 m.
pub fn encode(h: f32) -> u16 {
    let q = ((h as f64 - OFFSET_M) / SCALE_M).round();
    if q.is_finite() { q.clamp(1.0, 65535.0) as u16 } else { NODATA }
}

/// Writes `<dir>/<grid.key()>.u16` and its `.json`, each via a `.part` rename; the header lands last, so a
/// raster without one is never picked up as complete. Returns the `.u16` path.
pub fn write_raster(dir: &Path, grid: &Grid, heights: &[f32], sources: &[String], bbox: [f64; 4]) -> Result<PathBuf> {
    assert_eq!(heights.len(), grid.len());
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let raw = dir.join(format!("{}.u16", grid.key()));
    let codes: Vec<u16> = heights.iter().map(|&h| encode(h)).collect();
    let mut bytes = Vec::with_capacity(2 * codes.len());
    codes.iter().for_each(|v| bytes.extend_from_slice(&v.to_le_bytes()));
    write_atomic(&raw, &bytes)?;
    let header = serde_json::json!({
        "format": FORMAT, "crs": "EPSG:5070",
        "x0": grid.x0, "y0": grid.y0, "step_m": grid.step_m, "ncols": grid.ncols, "nrows": grid.nrows,
        "scale_m": SCALE_M, "offset_m": OFFSET_M, "nodata": NODATA, "sources": sources,
        "resampling": "bilinear", "vertical_datum": "MSL: NAVD88 (3DEP) / EGM2008 (GLO-30)",
        "bbox_lonlat": bbox,
        "layout": "row-major little-endian u16, row 0 north; node = (x0 + col*step, y0 - row*step)",
        "created_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        "builder": "dp_core::dem::ensure_dem",
        "stats": stats(&codes),
    });
    write_atomic(&raw.with_extension("json"), serde_json::to_string_pretty(&header)?.as_bytes())?;
    Ok(raw)
}

fn stats(codes: &[u16]) -> serde_json::Value {
    let valid: Vec<f64> = codes.iter().filter(|&&v| v != NODATA).map(|&v| v as f64 * SCALE_M + OFFSET_M).collect();
    let mut s = serde_json::json!({ "valid_fraction": valid.len() as f64 / codes.len().max(1) as f64 });
    if !valid.is_empty() {
        s["min_m"] = valid.iter().copied().fold(f64::INFINITY, f64::min).into();
        s["max_m"] = valid.iter().copied().fold(f64::NEG_INFINITY, f64::max).into();
        s["mean_m"] = (valid.iter().sum::<f64>() / valid.len() as f64).into();
    }
    s
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.part", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    let mut f = std::fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming {}", tmp.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dem::Dem;
    use crate::geo::albers::Xy;

    #[test]
    fn encode_matches_python() {
        assert_eq!(encode(0.0), 1000);
        assert_eq!(encode(-5.6), 944);
        assert_eq!(encode(f32::NAN), NODATA);
        assert_eq!(encode(-500.0), 1);
        assert_eq!(encode(9000.0), 65535);
    }

    #[test]
    fn round_trips_through_dem() {
        let dir = tempfile::tempdir().unwrap();
        let grid = Grid { x0: 300.0, y0: 600.0, step_m: 30.0, ncols: 3, nrows: 2 };
        let h = [10.0, 20.0, f32::NAN, 40.0, 50.0, 60.0];
        let p = write_raster(dir.path(), &grid, &h, &["3dep:x.tif".into()], [0.0; 4]).unwrap();
        let dem = Dem::open(&p).unwrap();
        assert_eq!(Grid::from_header(&dem.header), grid);
        assert_eq!(dem.header.sources, vec!["3dep:x.tif".to_string()]);
        assert!((dem.height_msl(Xy { x: 330.0, y: 570.0 }).unwrap() - 50.0).abs() < 1e-9);
        assert!((dem.height_msl(Xy { x: 345.0, y: 600.0 }).unwrap() - 20.0).abs() < 1e-9, "nodata dropped");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2, "no .part left behind");
    }
}
