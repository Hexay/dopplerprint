//! Memory-mapped EPSG:5070 u16 ground raster written by `ensure_dem` or python/dopplerprint/prep/dem.py.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use memmap2::Mmap;

use super::geoid_undulation;
use crate::geo::albers::{self, Xy};

pub const FORMAT: &str = "dp-dem-u16-v1";

/// `<name>.json` sidecar. Node (row, col) sits at `(x0 + col·step, y0 − row·step)`, row 0 is north.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DemHeader {
    pub format: String,
    pub crs: String,
    pub x0: f64,
    pub y0: f64,
    pub step_m: f64,
    pub ncols: usize,
    pub nrows: usize,
    /// Height (m, MSL) = raw · scale_m + offset_m.
    pub scale_m: f64,
    pub offset_m: f64,
    pub nodata: u16,
    #[serde(default)]
    pub sources: Vec<String>,
}

impl DemHeader {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("DemHeader is always serialisable")
    }
}

pub struct Dem {
    pub header: DemHeader,
    map: Mmap,
}

impl std::fmt::Debug for Dem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dem").field("header", &self.header).finish_non_exhaustive()
    }
}

impl Dem {
    /// Opens `<name>.u16` (or any path whose sibling `<stem>.json` holds the header).
    pub fn open(path: &Path) -> Result<Dem> {
        let raw = path.with_extension("u16");
        let hdr_path = path.with_extension("json");
        let header: DemHeader = serde_json::from_slice(
            &std::fs::read(&hdr_path).with_context(|| format!("reading {}", hdr_path.display()))?,
        )
        .with_context(|| format!("parsing {}", hdr_path.display()))?;
        ensure!(header.format == FORMAT, "{}: format {:?}, expected {FORMAT}", hdr_path.display(), header.format);
        ensure!(header.crs == "EPSG:5070", "{}: crs {}", hdr_path.display(), header.crs);
        ensure!(header.nrows >= 2 && header.ncols >= 2 && header.step_m > 0.0, "degenerate DEM header");
        let file = std::fs::File::open(&raw).with_context(|| format!("opening {}", raw.display()))?;
        // SAFETY: the raster is written once by prep and treated as read-only; truncation is checked below.
        let map = unsafe { Mmap::map(&file)? };
        ensure!(map.len() == 2 * header.nrows * header.ncols, "{}: size mismatch with header", raw.display());
        Ok(Dem { header, map })
    }

    fn raw(&self, r: usize, c: usize) -> u16 {
        let o = 2 * (r * self.header.ncols + c);
        u16::from_le_bytes([self.map[o], self.map[o + 1]])
    }

    /// Orthometric (MSL) ground height in metres, bilinear. Nodata neighbours are dropped and the remaining
    /// weights renormalised, so coastlines still resolve; `None` outside the raster or over all-nodata.
    pub fn height_msl(&self, xy: Xy) -> Option<f64> {
        let h = &self.header;
        let fc = (xy.x - h.x0) / h.step_m;
        let fr = (h.y0 - xy.y) / h.step_m;
        if !(0.0..=(h.ncols - 1) as f64).contains(&fc) || !(0.0..=(h.nrows - 1) as f64).contains(&fr) {
            return None;
        }
        let (r, c) = ((fr as usize).min(h.nrows - 2), (fc as usize).min(h.ncols - 2));
        let (tr, tc) = (fr - r as f64, fc - c as f64);
        let (mut sum, mut wsum) = (0.0, 0.0);
        for (dr, dc, w) in [(0, 0, (1.0 - tr) * (1.0 - tc)), (0, 1, (1.0 - tr) * tc), (1, 0, tr * (1.0 - tc)), (1, 1, tr * tc)] {
            let v = self.raw(r + dr, c + dc);
            if v != h.nodata && w > 0.0 {
                sum += w * v as f64;
                wsum += w;
            }
        }
        (wsum > 0.0).then(|| sum / wsum * h.scale_m + h.offset_m)
    }

    pub fn height_msl_latlon(&self, lat_deg: f64, lon_deg: f64) -> Option<f64> {
        self.height_msl(albers::forward(lat_deg, lon_deg))
    }

    /// Ground height above the WGS84 ellipsoid: H_msl + N.
    pub fn height_ellipsoid_latlon(&self, lat_deg: f64, lon_deg: f64) -> Option<f64> {
        Some(self.height_msl_latlon(lat_deg, lon_deg)? + geoid_undulation(lat_deg, lon_deg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_dem(dir: &Path, vals: &[u16], ncols: usize) -> std::path::PathBuf {
        let header = DemHeader {
            format: FORMAT.into(),
            crs: "EPSG:5070".into(),
            x0: 1000.0,
            y0: 5000.0,
            step_m: 100.0,
            ncols,
            nrows: vals.len() / ncols,
            scale_m: 0.1,
            offset_m: -100.0,
            nodata: 0,
            sources: vec![],
        };
        let p = dir.join("t.u16");
        std::fs::write(&p, vals.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>()).unwrap();
        std::fs::write(p.with_extension("json"), serde_json::to_vec(&header).unwrap()).unwrap();
        p
    }

    #[test]
    fn bilinear_and_nodata() {
        let dir = std::env::temp_dir().join(format!("dp_dem_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 2x3 grid; row 0 (north, y=5000): 1000, 1100, 0(nodata); row 1 (y=4900): 1200, 1300, 1400.
        let dem = Dem::open(&write_dem(&dir, &[1000, 1100, 0, 1200, 1300, 1400], 3)).unwrap();
        let h = |x, y| dem.height_msl(Xy { x, y });
        assert!((h(1000.0, 5000.0).unwrap() - 0.0).abs() < 1e-9);
        assert!((h(1050.0, 4950.0).unwrap() - 15.0).abs() < 1e-9);
        assert!((h(1200.0, 4900.0).unwrap() - 40.0).abs() < 1e-9);
        assert!((h(1200.0, 4950.0).unwrap() - 40.0).abs() < 1e-9, "nodata neighbour renormalised");
        assert_eq!(h(999.0, 5000.0), None);
        assert_eq!(h(1000.0, 4899.0), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
