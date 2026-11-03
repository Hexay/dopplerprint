//! EPSG:5070 node grid of a `dp-dem-u16-v1` raster: alignment, extent and bbox conversions (docs/GEO.md).

use crate::geo::albers::{self, Xy};

use super::DemHeader;

/// Points per bbox edge when projecting it, matching the Python prep's `transform_bounds(densify_pts=64)`.
const DENSIFY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub x0: f64,
    pub y0: f64,
    pub step_m: f64,
    pub ncols: usize,
    pub nrows: usize,
}

impl Grid {
    /// Nodes snapped to multiples of `step_m` covering the lon/lat bbox `[lon_w, lat_s, lon_e, lat_n]`.
    pub fn for_bbox(bbox: [f64; 4], step_m: f64) -> Grid {
        let [xmin, ymin, xmax, ymax] = project_bounds(bbox, |lon, lat| {
            let p = albers::forward(lat, lon);
            (p.x, p.y)
        });
        let (x0, x1) = ((xmin / step_m).floor() * step_m, (xmax / step_m).ceil() * step_m);
        let (y1, y0) = ((ymin / step_m).floor() * step_m, (ymax / step_m).ceil() * step_m);
        Grid {
            x0,
            y0,
            step_m,
            ncols: ((x1 - x0) / step_m).round() as usize + 1,
            nrows: ((y0 - y1) / step_m).round() as usize + 1,
        }
    }

    pub fn from_header(h: &DemHeader) -> Grid {
        Grid { x0: h.x0, y0: h.y0, step_m: h.step_m, ncols: h.ncols, nrows: h.nrows }
    }

    pub fn node(&self, row: usize, col: usize) -> Xy {
        Xy { x: self.x0 + col as f64 * self.step_m, y: self.y0 - row as f64 * self.step_m }
    }

    pub fn len(&self) -> usize {
        self.nrows * self.ncols
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lon/lat bounds of the node extent padded by one step, the region whose source tiles are needed.
    pub fn lonlat_bounds(&self) -> [f64; 4] {
        let s = self.step_m;
        let xy = [self.x0 - s, self.y0 - self.nrows as f64 * s, self.x0 + self.ncols as f64 * s, self.y0 + s];
        project_bounds(xy, |x, y| {
            let (lat, lon) = albers::inverse(Xy { x, y });
            (lon, lat)
        })
    }

    /// True when every node of `other` is a node of `self`.
    pub fn covers(&self, other: &Grid) -> bool {
        let same_lattice = (self.step_m - other.step_m).abs() < 1e-9
            && ((other.x0 - self.x0) / self.step_m).fract().abs() < 1e-9
            && ((self.y0 - other.y0) / self.step_m).fract().abs() < 1e-9;
        let x1 = |g: &Grid| g.x0 + (g.ncols - 1) as f64 * g.step_m;
        let y1 = |g: &Grid| g.y0 - (g.nrows - 1) as f64 * g.step_m;
        same_lattice
            && other.x0 >= self.x0 - 1e-6
            && other.y0 <= self.y0 + 1e-6
            && x1(other) <= x1(self) + 1e-6
            && y1(other) >= y1(self) - 1e-6
    }

    /// File stem for a raster built on this grid.
    pub fn key(&self) -> String {
        format!("dem_{}m_{}_{}_{}x{}", self.step_m, self.x0, self.y0, self.ncols, self.nrows)
    }

    /// Inclusive (row, col) ranges of nodes inside the EPSG:5070 rectangle, or `None` if it misses the grid.
    pub fn node_range(&self, xy_bounds: [f64; 4]) -> Option<((usize, usize), (usize, usize))> {
        let [xmin, ymin, xmax, ymax] = xy_bounds;
        let s = self.step_m;
        let c0 = ((xmin - self.x0) / s).ceil().max(0.0);
        let c1 = ((xmax - self.x0) / s).floor().min((self.ncols - 1) as f64);
        let r0 = ((self.y0 - ymax) / s).ceil().max(0.0);
        let r1 = ((self.y0 - ymin) / s).floor().min((self.nrows - 1) as f64);
        (c0 <= c1 && r0 <= r1).then_some(((r0 as usize, r1 as usize), (c0 as usize, c1 as usize)))
    }
}

/// Grows a lon/lat bbox by `margin_m` on every side.
pub fn expand_bbox(bbox: [f64; 4], margin_m: f64) -> [f64; 4] {
    let [lon_w, lat_s, lon_e, lat_n] = bbox;
    let dlat = margin_m / 111_000.0;
    let coslat = lat_s.abs().max(lat_n.abs()).to_radians().cos().max(0.01);
    let dlon = margin_m / (111_000.0 * coslat);
    [lon_w - dlon, lat_s - dlat, lon_e + dlon, lat_n + dlat]
}

/// Bounds of a rectangle `[a0, b0, a1, b1]` after mapping every densified edge point through `f`.
pub(super) fn project_bounds(r: [f64; 4], f: impl Fn(f64, f64) -> (f64, f64)) -> [f64; 4] {
    let [a0, b0, a1, b1] = r;
    let mut out = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let n = DENSIFY + 1;
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let (a, b) = (a0 + t * (a1 - a0), b0 + t * (b1 - b0));
        for (u, v) in [f(a, b0), f(a, b1), f(a0, b), f(a1, b)] {
            out = [out[0].min(u), out[1].min(v), out[2].max(u), out[3].max(v)];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Python-built `livermore_30m` header for this bbox: x0 −2256030, y0 1959810, 2049 × 2068 nodes.
    #[test]
    fn matches_python_grid() {
        let bbox = [-122.06638924045872, 37.437990774774775, -121.49736275954128, 37.88844122522522];
        let g = Grid::for_bbox(bbox, 30.0);
        assert_eq!(g, Grid { x0: -2256030.0, y0: 1959810.0, step_m: 30.0, ncols: 2049, nrows: 2068 });
    }

    #[test]
    fn alignment_and_cover() {
        let g = Grid::for_bbox([-100.3, 40.1, -99.8, 40.4], 30.0);
        assert_eq!(g.x0 % 30.0, 0.0);
        assert_eq!(g.y0 % 30.0, 0.0);
        let corners = [(40.1, -100.3), (40.4, -100.3), (40.1, -99.8), (40.4, -99.8), (40.4, -100.05)];
        for (lat, lon) in corners {
            let p = albers::forward(lat, lon);
            assert!(p.x >= g.x0 && p.x <= g.node(0, g.ncols - 1).x, "x of {lat},{lon}");
            assert!(p.y <= g.y0 && p.y >= g.node(g.nrows - 1, 0).y, "y of {lat},{lon}");
        }
        let inner = Grid::for_bbox([-100.2, 40.2, -99.9, 40.3], 30.0);
        assert!(g.covers(&inner) && !inner.covers(&g));
        assert!(!g.covers(&Grid { x0: inner.x0 + 10.0, ..inner }), "off-lattice");
        assert!(!g.covers(&Grid::for_bbox([-100.2, 40.2, -99.9, 40.3], 100.0)), "other step");
        let [w, s, e, n] = g.lonlat_bounds();
        assert!(w < -100.3 && s < 40.1 && e > -99.8 && n > 40.4);
    }

    #[test]
    fn node_range_clamps() {
        let g = Grid { x0: 0.0, y0: 300.0, step_m: 30.0, ncols: 11, nrows: 11 };
        assert_eq!(g.node_range([15.0, 100.0, 61.0, 250.0]), Some(((2, 6), (1, 2))));
        assert_eq!(g.node_range([-1e6, -1e6, 1e6, 1e6]), Some(((0, 10), (0, 10))));
        assert_eq!(g.node_range([400.0, 0.0, 500.0, 300.0]), None);
    }

    #[test]
    fn expand_grows_every_side() {
        let b = expand_bbox([-122.0, 37.0, -121.0, 38.0], 1000.0);
        assert!((b[1] - (37.0 - 1000.0 / 111_000.0)).abs() < 1e-12);
        assert!(b[0] < -122.011 && b[2] > -120.989 && b[3] > 38.009);
    }
}
