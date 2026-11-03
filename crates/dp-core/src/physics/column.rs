//! Height-resolved medium: c(z) and horizontal wind(z) as piecewise-linear nodes, averaged over the
//! receiver→source height layer per emission, as departures from a reference layer (docs/PHYSICS.md §4a).

use crate::geo::Vec3;

pub const MAX_NODES: usize = 16;
const EARTH_R_M: f64 = 6_371_000.0;
/// Layers thinner than this use the point value (the mean's 0/0 limit).
const THIN_M: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Column {
    /// ECEF point the local vertical is taken at, its ellipsoidal height and unit up vector.
    origin: Vec3,
    up: Vec3,
    h0: f64,
    n: usize,
    /// Node ellipsoidal heights (ascending), slowness 1/c and ECEF horizontal wind.
    z: [f64; MAX_NODES],
    slow: [f64; MAX_NODES],
    wind: [Vec3; MAX_NODES],
    /// ∫ slowness dz and ∫ wind dz from z[0] (exact for the linear interpolant).
    slow_int: [f64; MAX_NODES],
    wind_int: [Vec3; MAX_NODES],
    /// Layer mean the owning `Atmosphere` carries as its `c` / `wind`.
    c_ref: f64,
    wind_ref: Vec3,
}

/// Uniform medium seen by one receiver/source pair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Medium {
    pub c: f64,
    pub wind: Vec3,
}

impl Column {
    /// `nodes` = (ellipsoidal height m, c m/s, ECEF horizontal wind), any order; at most `MAX_NODES`.
    /// The column's own reference is its mean over `ref_layer` (ellipsoidal heights).
    pub fn new(origin: Vec3, up: Vec3, h0: f64, nodes: &[(f64, f64, Vec3)], ref_layer: (f64, f64)) -> Option<Self> {
        let mut v: Vec<(f64, f64, Vec3)> = nodes.iter().copied().filter(|n| n.0.is_finite() && n.1 > 0.0).collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6);
        if v.is_empty() || v.len() > MAX_NODES {
            return None;
        }
        let mut col = Column {
            origin,
            up,
            h0,
            n: v.len(),
            z: [0.0; MAX_NODES],
            slow: [0.0; MAX_NODES],
            wind: [Vec3::ZERO; MAX_NODES],
            slow_int: [0.0; MAX_NODES],
            wind_int: [Vec3::ZERO; MAX_NODES],
            c_ref: 0.0,
            wind_ref: Vec3::ZERO,
        };
        for (i, &(z, c, w)) in v.iter().enumerate() {
            (col.z[i], col.slow[i], col.wind[i]) = (z, 1.0 / c, w);
            if i > 0 {
                let dz = z - col.z[i - 1];
                col.slow_int[i] = col.slow_int[i - 1] + 0.5 * dz * (col.slow[i] + col.slow[i - 1]);
                col.wind_int[i] = col.wind_int[i - 1] + (col.wind[i] + col.wind[i - 1]) * (0.5 * dz);
            }
        }
        let m = col.layer_mean(ref_layer.0, ref_layer.1);
        (col.c_ref, col.wind_ref) = (m.c, m.wind);
        Some(col)
    }

    pub fn reference(&self) -> Medium {
        Medium { c: self.c_ref, wind: self.wind_ref }
    }

    /// Ellipsoidal height of a point near the origin (spherical-earth drop, < 1 m error within 20 km).
    pub fn height(&self, p: Vec3) -> f64 {
        let d = p - self.origin;
        let along = d.dot(self.up);
        self.h0 + along + (d.dot(d) - along * along) / (2.0 * EARTH_R_M)
    }

    /// Harmonic-mean c and mean wind over [a, b] (ellipsoidal heights, either order); constant beyond the
    /// end nodes.
    pub fn layer_mean(&self, a: f64, b: f64) -> Medium {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        if hi - lo < THIN_M {
            let (s, w) = self.point(0.5 * (lo + hi));
            return Medium { c: 1.0 / s, wind: w };
        }
        let (s0, w0) = self.integral(lo);
        let (s1, w1) = self.integral(hi);
        let k = 1.0 / (hi - lo);
        Medium { c: 1.0 / ((s1 - s0) * k), wind: (w1 - w0) * k }
    }

    /// Departure of the receiver→source layer mean from the reference.
    pub fn anomaly(&self, obs: Vec3, src: Vec3) -> Medium {
        let m = self.layer_mean(self.height(obs), self.height(src));
        Medium { c: m.c - self.c_ref, wind: m.wind - self.wind_ref }
    }

    /// Node segment containing z (clamped) and the fraction along it.
    fn locate(&self, z: f64) -> (usize, f64) {
        let n = self.n;
        if n == 1 || z <= self.z[0] {
            return (0, 0.0);
        }
        if z >= self.z[n - 1] {
            return (n - 2, 1.0);
        }
        let i = self.z[..n].partition_point(|&zi| zi <= z) - 1;
        (i, (z - self.z[i]) / (self.z[i + 1] - self.z[i]))
    }

    fn point(&self, z: f64) -> (f64, Vec3) {
        let (i, f) = self.locate(z);
        if self.n == 1 {
            return (self.slow[0], self.wind[0]);
        }
        (self.slow[i] + f * (self.slow[i + 1] - self.slow[i]), self.wind[i] + (self.wind[i + 1] - self.wind[i]) * f)
    }

    fn integral(&self, z: f64) -> (f64, Vec3) {
        let n = self.n;
        if z <= self.z[0] {
            let dz = z - self.z[0];
            return (dz * self.slow[0], self.wind[0] * dz);
        }
        if z >= self.z[n - 1] {
            let dz = z - self.z[n - 1];
            return (self.slow_int[n - 1] + dz * self.slow[n - 1], self.wind_int[n - 1] + self.wind[n - 1] * dz);
        }
        let (i, _) = self.locate(z);
        let (s, w) = self.point(z);
        let dz = z - self.z[i];
        (self.slow_int[i] + 0.5 * dz * (self.slow[i] + s), self.wind_int[i] + (self.wind[i] + w) * (0.5 * dz))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(nodes: &[(f64, f64, Vec3)]) -> Column {
        Column::new(Vec3::new(0.0, 0.0, 6_371_000.0), Vec3::new(0.0, 0.0, 1.0), 0.0, nodes, (0.0, 1000.0)).unwrap()
    }

    #[test]
    fn linear_wind_mean_is_the_midpoint_value() {
        let c = col(&[(0.0, 340.0, Vec3::ZERO), (1000.0, 340.0, Vec3::new(10.0, 0.0, 0.0))]);
        let m = c.layer_mean(200.0, 600.0);
        assert!((m.wind.x - 4.0).abs() < 1e-12 && (m.c - 340.0).abs() < 1e-9);
        assert!((c.layer_mean(600.0, 200.0).wind.x - 4.0).abs() < 1e-12);
        assert!((c.layer_mean(500.0, 500.2).wind.x - 5.001).abs() < 1e-9);
    }

    #[test]
    fn mean_c_is_harmonic_and_extrapolation_is_constant() {
        let c = col(&[(100.0, 330.0, Vec3::ZERO), (200.0, 350.0, Vec3::ZERO)]);
        let m = c.layer_mean(0.0, 300.0);
        let slow = (100.0 / 330.0 + 100.0 * 0.5 * (1.0 / 330.0 + 1.0 / 350.0) + 100.0 / 350.0) / 300.0;
        assert!((m.c - 1.0 / slow).abs() < 1e-9);
    }

    #[test]
    fn anomaly_is_zero_on_the_reference_layer() {
        let c = col(&[(0.0, 345.0, Vec3::new(2.0, 0.0, 0.0)), (3000.0, 330.0, Vec3::new(14.0, 3.0, 0.0))]);
        let a = c.anomaly(Vec3::new(0.0, 0.0, 6_371_000.0), Vec3::new(0.0, 0.0, 6_372_000.0));
        assert!(a.c.abs() < 1e-9 && a.wind.norm() < 1e-9);
        let high = c.anomaly(Vec3::new(0.0, 0.0, 6_371_000.0), Vec3::new(0.0, 0.0, 6_374_000.0));
        assert!(high.c < -1.0 && high.wind.x > 3.0);
    }

    #[test]
    fn height_includes_earth_curvature() {
        let c = col(&[(0.0, 340.0, Vec3::ZERO)]);
        let h = c.height(Vec3::new(10_000.0, 0.0, 6_371_000.0));
        assert!((h - 100_000_000.0 / (2.0 * EARTH_R_M)).abs() < 1e-6);
    }
}
