//! EPSG:5070 (NAD83 / Conus Albers) forward/inverse projection and the candidate lattice.
//! NAD83 vs WGS84 differ by ~1 m, below our tolerance, so WGS84 lat/lon are used directly.
//! Formulas: Snyder, "Map Projections: A Working Manual" (USGS PP 1395), eqs. 14-3..14-21, 3-12, 3-16.

use std::sync::LazyLock;

/// Integer cell on a square lattice in EPSG:5070 metres. `step_m` halves per refinement level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Cell {
    pub ix: i32,
    pub iy: i32,
    pub step_m: u32,
}

/// Projected EPSG:5070 coordinate in metres.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Xy {
    pub x: f64,
    pub y: f64,
}

const GRS80_A: f64 = 6_378_137.0;
const GRS80_F: f64 = 1.0 / 298.257_222_101;
const LAT_0: f64 = 23.0;
const LON_0: f64 = -96.0;
const LAT_1: f64 = 29.5;
const LAT_2: f64 = 45.5;

struct Albers {
    e: f64,
    e2: f64,
    n: f64,
    c: f64,
    rho0: f64,
}

static P: LazyLock<Albers> = LazyLock::new(|| {
    let e2 = GRS80_F * (2.0 - GRS80_F);
    let e = e2.sqrt();
    let m = |lat: f64| {
        let s = lat.to_radians().sin();
        lat.to_radians().cos() / (1.0 - e2 * s * s).sqrt()
    };
    let (m1, m2) = (m(LAT_1), m(LAT_2));
    let (q0, q1, q2) = (q(e, e2, LAT_0.to_radians()), q(e, e2, LAT_1.to_radians()), q(e, e2, LAT_2.to_radians()));
    let n = (m1 * m1 - m2 * m2) / (q2 - q1);
    let c = m1 * m1 + n * q1;
    let rho0 = GRS80_A * (c - n * q0).sqrt() / n;
    Albers { e, e2, n, c, rho0 }
});

fn q(e: f64, e2: f64, lat_rad: f64) -> f64 {
    let s = lat_rad.sin();
    (1.0 - e2) * (s / (1.0 - e2 * s * s) - ((1.0 - e * s) / (1.0 + e * s)).ln() / (2.0 * e))
}

pub fn forward(lat_deg: f64, lon_deg: f64) -> Xy {
    let p = &*P;
    let rho = GRS80_A * (p.c - p.n * q(p.e, p.e2, lat_deg.to_radians())).sqrt() / p.n;
    let theta = p.n * (lon_deg - LON_0).to_radians();
    let (st, ct) = theta.sin_cos();
    Xy { x: rho * st, y: p.rho0 - rho * ct }
}

/// Returns (lat_deg, lon_deg).
pub fn inverse(xy: Xy) -> (f64, f64) {
    let p = &*P;
    let dy = p.rho0 - xy.y;
    let rho = xy.x.hypot(dy);
    let theta = xy.x.atan2(dy);
    let qv = (p.c - rho * rho * p.n * p.n / (GRS80_A * GRS80_A)) / p.n;
    let mut lat = (qv / 2.0).clamp(-1.0, 1.0).asin();
    for _ in 0..20 {
        let (s, c) = lat.sin_cos();
        let w = 1.0 - p.e2 * s * s;
        let d = w * w / (2.0 * c)
            * (qv / (1.0 - p.e2) - s / w + ((1.0 - p.e * s) / (1.0 + p.e * s)).ln() / (2.0 * p.e));
        lat += d;
        if d.abs() < 1e-15 {
            break;
        }
    }
    (lat.to_degrees(), LON_0 + (theta / p.n).to_degrees())
}

impl Cell {
    pub fn center(&self) -> Xy {
        let s = self.step_m as f64;
        Xy { x: (self.ix as f64 + 0.5) * s, y: (self.iy as f64 + 0.5) * s }
    }
    pub fn of(xy: Xy, step_m: u32) -> Cell {
        let s = step_m as f64;
        Cell { ix: (xy.x / s).floor() as i32, iy: (xy.y / s).floor() as i32, step_m }
    }
    /// The 4 children at half the step.
    pub fn children(&self) -> [Cell; 4] {
        let (ix, iy, s) = (self.ix * 2, self.iy * 2, self.step_m / 2);
        [
            Cell { ix, iy, step_m: s },
            Cell { ix: ix + 1, iy, step_m: s },
            Cell { ix, iy: iy + 1, step_m: s },
            Cell { ix: ix + 1, iy: iy + 1, step_m: s },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// pyproj 3.x, `Transformer.from_crs("EPSG:4269", "EPSG:5070", always_xy=True)`.
    const PYPROJ: [(f64, f64, f64, f64); 6] = [
        (24.5, -124.8, -2914358.8993, 608459.9260),
        (40.876466, -73.524951, 1861214.7636, 2206274.9068),
        (49.4, -66.9, 2109162.5192, 3257587.7536),
        (37.66327, -121.7819, -2225249.4736, 1928797.4005),
        (23.0, -96.0, 0.0, 0.0),
        (45.5, -110.0, -1090259.3652, 2581778.4823),
    ];

    #[test]
    fn forward_matches_pyproj() {
        for (lat, lon, x, y) in PYPROJ {
            let p = forward(lat, lon);
            assert!((p.x - x).abs() < 1e-3 && (p.y - y).abs() < 1e-3, "{lat},{lon}: {p:?} vs {x},{y}");
        }
    }

    #[test]
    fn origin_and_roundtrip() {
        let o = forward(LAT_0, LON_0);
        assert!(o.x.abs() < 1e-9 && o.y.abs() < 1e-6);
        for (lat, lon) in [(24.5, -124.8), (40.876466, -73.524951), (49.4, -66.9), (37.66327, -121.7819)] {
            let (la, lo) = inverse(forward(lat, lon));
            assert!((la - lat).abs() < 1e-11 && (lo - lon).abs() < 1e-11, "{lat},{lon} -> {la},{lo}");
        }
    }
}
