//! Rust `ensure_dem` vs a Python-built raster of the same region. Needs data and (unless the tiles are
//! cached) network, so it is ignored by default:
//!
//!   DP_DEM_PARITY_REF=data/dem/5070/livermore_30m.u16 DOPPLERPRINT_CACHE=/some/cache \
//!     cargo nextest run -p dp-core --run-ignored all dem_fetch_parity --no-capture

use std::path::PathBuf;

use dp_core::dem::{Dem, DemFetchOptions, DemProgress, Grid, ensure_dem};

#[test]
#[ignore = "needs DP_DEM_PARITY_REF, DOPPLERPRINT_CACHE and tiles or network"]
fn dem_fetch_parity() {
    let reference = PathBuf::from(std::env::var("DP_DEM_PARITY_REF").expect("DP_DEM_PARITY_REF"));
    let cache = PathBuf::from(std::env::var("DOPPLERPRINT_CACHE").expect("DOPPLERPRINT_CACHE"));
    let py = Dem::open(&reference).unwrap();
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(reference.with_extension("json")).unwrap()).unwrap();
    let b: Vec<f64> = meta["bbox_lonlat"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let bbox = [b[0], b[1], b[2], b[3]];
    let py_grid = Grid::from_header(&py.header);
    assert_eq!(Grid::for_bbox(bbox, py_grid.step_m), py_grid, "grid alignment differs from the Python prep");

    let progress = |p: DemProgress<'_>| match p {
        DemProgress::Download { file, done, total } if total.is_some_and(|t| done == t) => eprintln!("downloaded {file} ({done} B)"),
        DemProgress::Resample { file, .. } => eprintln!("resampling {file}"),
        DemProgress::Reused { path } => eprintln!("reusing {}", path.display()),
        _ => {}
    };
    let opts = DemFetchOptions { step_m: py_grid.step_m, margin_m: 0.0, progress: Some(&progress), ..Default::default() };
    let t = std::time::Instant::now();
    let rs = ensure_dem(bbox, &cache, &opts).unwrap();
    eprintln!("ensure_dem: {:.1} s, sources {:?}", t.elapsed().as_secs_f64(), rs.header.sources);

    let (mut diffs, mut nodata_mismatch) = (Vec::with_capacity(py_grid.len()), 0usize);
    let mut worst = (0.0f64, 0, 0);
    // GDAL smears the Python prep's final partial 1024-row strip (docs/GEO.md "Rust path"); skip it.
    let full_rows = (py_grid.nrows / 1024).max(1) * 1024;
    for r in 0..full_rows.min(py_grid.nrows) {
        for c in 0..py_grid.ncols {
            let xy = py_grid.node(r, c);
            match (py.height_msl(xy), rs.height_msl(xy)) {
                (Some(a), Some(b)) => {
                    let d = (a - b).abs();
                    if d > worst.0 {
                        worst = (d, r, c);
                    }
                    diffs.push(d);
                }
                (None, None) => {}
                _ => nodata_mismatch += 1,
            }
        }
    }
    diffs.sort_by(f64::total_cmp);
    let q = |p: f64| diffs[((diffs.len() - 1) as f64 * p) as usize];
    let over_1m = diffs.iter().filter(|&&d| d > 1.0).count();
    eprintln!(
        "nodes {} | median {:.3} m | p99 {:.3} m | p99.99 {:.3} m | max {:.3} m at {:?} | >1 m: {} | nodata mismatch {}",
        diffs.len(),
        q(0.5),
        q(0.99),
        q(0.9999),
        worst.0,
        (worst.1, worst.2),
        over_1m,
        nodata_mismatch
    );
    assert!(q(0.5) <= 0.2 && q(0.99) < 3.0, "heights drift from the Python raster");
    assert!((over_1m as f64) < 0.1 * diffs.len() as f64);
}
