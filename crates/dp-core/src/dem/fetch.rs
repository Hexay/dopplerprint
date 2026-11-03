//! `ensure_dem`: a ready `Dem` for a bbox, from the cache or built from downloaded 1° tiles
//! (layout: docs/GEO.md "DEM raster").

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use super::geotiff::SourceRaster;
use super::grid::{Grid, expand_bbox};
use super::raster::{Dem, DemHeader, FORMAT};
use super::resolve::{DownloadPlan, PlannedFile, Resolved, resolve_tile};
use super::tiles::{DEFAULT_SOURCES, DemSource, Tile, tiles_for_bbox};
use super::{warp, write};

/// Largest grid built in memory (f32 per node); bigger regions belong to the Python prep's strip builder.
const MAX_NODES: usize = 150_000_000;

/// Progress reported by `ensure_dem`.
#[derive(Clone, Copy, Debug)]
pub enum DemProgress<'p> {
    Reused { path: &'p Path },
    Download { file: &'p str, done: u64, total: Option<u64> },
    Resample { file: &'p str, index: usize, count: usize },
}

pub struct DemFetchOptions<'a> {
    /// Node spacing (m). The Rust path always resamples bilinearly, so keep this near the 1″ source (≤ 45 m).
    pub step_m: f64,
    /// Added on every side of the requested bbox.
    pub margin_m: f64,
    pub sources: Vec<DemSource>,
    /// Never touch the network; error listing the tiles that would be needed.
    pub offline: bool,
    /// Refuse a plan whose known total exceeds this.
    pub max_download_bytes: u64,
    /// Asked before any download; `false` aborts.
    pub confirm: Option<&'a dyn Fn(&DownloadPlan) -> bool>,
    pub progress: Option<&'a dyn Fn(DemProgress<'_>)>,
}

impl Default for DemFetchOptions<'_> {
    fn default() -> Self {
        DemFetchOptions {
            step_m: 30.0,
            margin_m: 1000.0,
            sources: DEFAULT_SOURCES.to_vec(),
            offline: false,
            max_download_bytes: 2_000_000_000,
            confirm: None,
            progress: None,
        }
    }
}

impl DemFetchOptions<'_> {
    fn report(&self, p: DemProgress<'_>) {
        if let Some(f) = self.progress {
            f(p);
        }
    }
}

/// Memory-mapped MSL ground raster covering `bbox_wgs84 = [lon_w, lat_s, lon_e, lat_n]` plus the margin.
/// Reuses any `<cache_dir>/dem/*.u16` on the same lattice that covers it; otherwise fetches the missing
/// tiles into `<cache_dir>/dem/tiles/` and writes `<cache_dir>/dem/<key>.u16` + `.json`.
pub fn ensure_dem(bbox_wgs84: [f64; 4], cache_dir: &Path, opts: &DemFetchOptions) -> Result<Dem> {
    let [w, s, e, n] = bbox_wgs84;
    ensure!(bbox_wgs84.iter().all(|v| v.is_finite()) && w < e && s < n, "bad bbox {bbox_wgs84:?}");
    ensure!((-85.0..=85.0).contains(&s) && (-85.0..=85.0).contains(&n), "bbox {bbox_wgs84:?} outside ±85° lat");
    ensure!(opts.step_m >= 1.0 && opts.margin_m >= 0.0, "bad step/margin");
    let bbox = expand_bbox(bbox_wgs84, opts.margin_m);
    let grid = Grid::for_bbox(bbox, opts.step_m);
    ensure!(
        grid.len() <= MAX_NODES,
        "{}x{} nodes is too large to build here; use python -m dopplerprint.prep.dem",
        grid.ncols,
        grid.nrows
    );
    let dem_dir = cache_dir.join("dem");
    if let Some(path) = find_cached(&dem_dir, &grid) {
        opts.report(DemProgress::Reused { path: &path });
        return Dem::open(&path);
    }
    let inputs = fetch_tiles(&tiles_for_bbox(grid.lonlat_bounds()), &dem_dir.join("tiles"), opts)?;
    let mut heights = vec![f32::NAN; grid.len()];
    let mut used = Vec::new();
    for (i, (tile, source, path)) in inputs.iter().enumerate() {
        let name = path.file_name().and_then(|f| f.to_str()).unwrap_or("?");
        opts.report(DemProgress::Resample { file: name, index: i, count: inputs.len() });
        let raster = SourceRaster::read(path, *tile)
            .with_context(|| format!("bad cached tile; delete {} to re-download", path.display()))?;
        warp::warp_tile(&grid, &raster, &mut heights);
        used.push(format!("{}:{name}", source.label()));
    }
    used.sort();
    Dem::open(&write::write_raster(&dem_dir, &grid, &heights, &used, bbox)?)
}

/// Smallest cached raster on the same lattice that covers `grid`.
fn find_cached(dem_dir: &Path, grid: &Grid) -> Option<PathBuf> {
    std::fs::read_dir(dem_dir)
        .ok()?
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| {
            let h: DemHeader = serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?;
            let g = Grid::from_header(&h);
            let ok = h.format == FORMAT && h.crs == "EPSG:5070" && g.covers(grid);
            let raw = p.with_extension("u16");
            (ok && raw.metadata().is_ok_and(|m| m.len() == 2 * g.len() as u64)).then_some((g.len(), raw))
        })
        .min_by_key(|(n, _)| *n)
        .map(|(_, p)| p)
}

/// Source file per tile in warp order (ocean tiles dropped), downloading what is missing.
fn fetch_tiles(tiles: &[Tile], dir: &Path, opts: &DemFetchOptions) -> Result<Vec<(Tile, DemSource, PathBuf)>> {
    let mut inputs = Vec::new();
    let (mut plan, mut unknown) = (DownloadPlan::default(), Vec::new());
    for &tile in tiles {
        match resolve_tile(tile, dir, &opts.sources, !opts.offline)? {
            Resolved::Cached(src, path) => inputs.push((tile, src, path)),
            Resolved::Ocean => {}
            Resolved::Download(f) => {
                inputs.push((tile, f.source, f.path.clone()));
                plan.files.push(f);
            }
            Resolved::Unknown(f) => unknown.push(f),
        }
    }
    if !unknown.is_empty() {
        let list: Vec<String> = unknown.iter().map(|f| format!("  {} -> {}", f.url, f.path.display())).collect();
        bail!("offline and {} DEM tile(s) are not cached:\n{}", unknown.len(), list.join("\n"));
    }
    if !plan.files.is_empty() {
        let total = plan.total_bytes();
        ensure!(
            total <= opts.max_download_bytes,
            "DEM download of {:.0} MB exceeds the {:.0} MB limit",
            total as f64 / 1e6,
            opts.max_download_bytes as f64 / 1e6
        );
        if opts.confirm.is_some_and(|c| !c(&plan)) {
            bail!("DEM download of {} tile(s) declined", plan.files.len());
        }
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        for f in &plan.files {
            download(f, opts)?;
        }
    }
    Ok(inputs)
}

#[cfg(feature = "net")]
fn download(f: &PlannedFile, opts: &DemFetchOptions) -> Result<()> {
    use super::download::{Remote, download};
    let name = f.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    let max = opts.max_download_bytes.min(f.bytes.map_or(u64::MAX, |b| b + 1));
    let mut cb = |done, total| opts.report(DemProgress::Download { file: name, done, total });
    match download(&f.url, &f.path, max, &mut cb)? {
        Remote::Available(_) => Ok(()),
        Remote::Missing => bail!("{} disappeared between probe and download", f.url),
    }
}

#[cfg(not(feature = "net"))]
fn download(f: &PlannedFile, _: &DemFetchOptions) -> Result<()> {
    bail!("{}: built without the `net` feature, cannot download", f.url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dem::geotiff::tests::fixture_with;
    use std::cell::Cell;
    use tiff::encoder::Compression;

    const ARCSEC: f64 = 1.0 / 3600.0;

    /// Linear field over the 3DEP-style fixture (2-pixel overlap, pixel-is-area).
    fn plane(lat: f64, lon: f64) -> f64 {
        500.0 + 2000.0 * (41.0 - lat) + 1000.0 * (lon + 100.0)
    }

    #[test]
    fn builds_from_cached_tile_then_reuses_then_reports_missing_offline() {
        let cache = tempfile::tempdir().unwrap();
        let tiles = cache.path().join("dem/tiles");
        std::fs::create_dir_all(&tiles).unwrap();
        let (lat0, lon0) = (41.0 + 1.5 * ARCSEC, -100.0 - 1.5 * ARCSEC);
        let tif = fixture_with(3604, 3604, [-100.0 - 2.0 * ARCSEC, 41.0 + 2.0 * ARCSEC], ARCSEC, false, Compression::Uncompressed, |r, c| {
            plane(lat0 - r as f64 * ARCSEC, lon0 + c as f64 * ARCSEC) as f32
        });
        std::fs::write(tiles.join("USGS_1_n41w100.tif"), tif).unwrap();

        let events = Cell::new((0, 0));
        let progress = |p: DemProgress<'_>| {
            let (re, rs) = events.get();
            match p {
                DemProgress::Reused { .. } => events.set((re + 1, rs)),
                DemProgress::Resample { .. } => events.set((re, rs + 1)),
                DemProgress::Download { .. } => panic!("offline"),
            }
        };
        let opts = DemFetchOptions { offline: true, margin_m: 0.0, progress: Some(&progress), ..Default::default() };
        let bbox = [-99.6, 40.4, -99.5, 40.5];
        let dem = ensure_dem(bbox, cache.path(), &opts).unwrap();
        assert_eq!(events.get(), (0, 1));
        assert_eq!(dem.header.sources, vec!["3dep:USGS_1_n41w100.tif".to_string()]);
        for (lat, lon) in [(40.4, -99.6), (40.45, -99.55), (40.5, -99.5)] {
            let h = dem.height_msl_latlon(lat, lon).unwrap();
            assert!((h - plane(lat, lon)).abs() < 0.06, "{lat},{lon}: {h} vs {}", plane(lat, lon));
        }

        let again = ensure_dem([-99.58, 40.42, -99.52, 40.48], cache.path(), &opts).unwrap();
        assert_eq!(events.get(), (1, 1), "covered by the first raster");
        assert_eq!(again.header, dem.header);

        let err = ensure_dem([-99.6, 41.4, -99.5, 41.5], cache.path(), &opts).unwrap_err().to_string();
        assert!(err.contains("not cached") && err.contains("USGS_1_n42w100.tif"), "{err}");
    }

    #[test]
    fn refuses_bad_bbox_and_huge_grids() {
        let cache = tempfile::tempdir().unwrap();
        let o = DemFetchOptions { offline: true, ..Default::default() };
        assert!(ensure_dem([-99.0, 40.0, -100.0, 41.0], cache.path(), &o).is_err());
        assert!(ensure_dem([-99.0, 40.0, f64::NAN, 41.0], cache.path(), &o).is_err());
        let err = ensure_dem([-125.0, 24.0, -66.0, 50.0], cache.path(), &o).unwrap_err().to_string();
        assert!(err.contains("too large"), "{err}");
    }
}
