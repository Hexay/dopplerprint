//! Resamples lon/lat source tiles onto the EPSG:5070 node grid.

use rayon::prelude::*;

use super::geotiff::SourceRaster;
use super::grid::{Grid, project_bounds};
use crate::geo::albers::{self, Xy};

/// Writes `src` into every node of `out` (row-major over `grid`, NaN = unset) that falls inside the tile's
/// pixel extent and has a valid bilinear sample. Calling this per tile in order lets later tiles overwrite
/// overlaps, as GDAL's `reproject` into a shared destination does.
pub fn warp_tile(grid: &Grid, src: &SourceRaster, out: &mut [f32]) {
    assert_eq!(out.len(), grid.len());
    let xy_bounds = project_bounds(src.extent(), |lon, lat| {
        let p = albers::forward(lat, lon);
        (p.x, p.y)
    });
    let Some(((r0, r1), (c0, c1))) = grid.node_range(xy_bounds) else { return };
    out.par_chunks_mut(grid.ncols).enumerate().skip(r0).take(r1 + 1 - r0).for_each(|(r, row)| {
        for (c, cell) in row.iter_mut().enumerate().take(c1 + 1).skip(c0) {
            let (lat, lon) = albers::inverse(grid.node(r, c));
            if let Some(h) = src.sample(lat, lon) {
                *cell = h as f32;
            }
        }
    });
}

/// MSL height at an EPSG:5070 point from a set of tiles, last tile wins (for tests and spot checks).
pub fn sample_tiles(tiles: &[SourceRaster], xy: Xy) -> Option<f64> {
    let (lat, lon) = albers::inverse(xy);
    tiles.iter().rev().find_map(|t| t.sample(lat, lon))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(lon0: f64, lat0: f64, n: usize, step: f64) -> SourceRaster {
        let data = (0..n * n).map(|i| (100.0 + 1000.0 * (lat0 - (i / n) as f64 * step) + 500.0 * (lon0 + (i % n) as f64 * step)) as f32);
        SourceRaster { width: n, height: n, lon0, lat0, dlon: step, dlat: step, nodata: None, data: data.collect() }
    }

    #[test]
    fn plane_survives_reprojection_and_tile_seams() {
        // Two abutting 0.1° tiles; a linear field comes through bilinear exactly except within half a pixel of
        // the seam, where (as in GDAL's per-tile warp) each tile only has one column to offer.
        let step = 0.1 / 36.0;
        let west = plane(-100.1 + step / 2.0, 40.1 - step / 2.0, 36, step);
        let east = plane(-100.0 + step / 2.0, 40.1 - step / 2.0, 36, step);
        let grid = Grid::for_bbox([-100.09, 40.01, -99.91, 40.09], 30.0);
        let mut out = vec![f32::NAN; grid.len()];
        warp_tile(&grid, &west, &mut out);
        warp_tile(&grid, &east, &mut out);
        let mut checked = 0;
        for r in 0..grid.nrows {
            for c in 0..grid.ncols {
                let (lat, lon) = albers::inverse(grid.node(r, c));
                if !(40.0 + step..40.1 - step).contains(&lat) || !(-100.1 + step..-99.9 - step).contains(&lon) {
                    continue;
                }
                if (lon + 100.0).abs() < step / 2.0 {
                    let got = out[r * grid.ncols + c] as f64;
                    assert!((got - (100.0 + 1000.0 * lat + 500.0 * lon)).abs() < 500.0 * step, "seam ({r},{c})");
                    continue;
                }
                let want = 100.0 + 1000.0 * lat + 500.0 * lon;
                let got = out[r * grid.ncols + c] as f64;
                assert!((got - want).abs() < 0.02, "node ({r},{c}) {got} vs {want}");
                checked += 1;
            }
        }
        assert!(checked > 100_000, "{checked}");
        let mid = grid.node(grid.nrows / 2, grid.ncols / 2);
        assert!(sample_tiles(&[west, east], mid).is_some());
    }

    #[test]
    fn nodes_outside_tile_stay_unset() {
        let step = 0.01;
        let t = plane(-100.0 + step / 2.0, 40.1 - step / 2.0, 10, step);
        let grid = Grid::for_bbox([-100.2, 39.9, -99.7, 40.2], 300.0);
        let mut out = vec![f32::NAN; grid.len()];
        warp_tile(&grid, &t, &mut out);
        let set = out.iter().filter(|v| v.is_finite()).count();
        let area_ratio = (0.1 * 0.1) / (0.5 * 0.3);
        assert!(set > 0 && (set as f64) < 1.5 * area_ratio * grid.len() as f64, "{set} of {}", grid.len());
    }
}
