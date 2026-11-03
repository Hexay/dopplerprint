pub mod albers;
pub mod vec3;

pub use vec3::Vec3;

const WGS84_A: f64 = 6_378_137.0;
const WGS84_F: f64 = 1.0 / 298.257_223_563;
const WGS84_E2: f64 = WGS84_F * (2.0 - WGS84_F);

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Geodetic {
    pub lat_deg: f64,
    pub lon_deg: f64,
    /// Height above the WGS84 ellipsoid (not MSL).
    pub h_ell_m: f64,
}

pub fn geodetic_to_ecef(g: Geodetic) -> Vec3 {
    let (lat, lon) = (g.lat_deg.to_radians(), g.lon_deg.to_radians());
    let (sl, cl) = lat.sin_cos();
    let n = WGS84_A / (1.0 - WGS84_E2 * sl * sl).sqrt();
    Vec3::new(
        (n + g.h_ell_m) * cl * lon.cos(),
        (n + g.h_ell_m) * cl * lon.sin(),
        (n * (1.0 - WGS84_E2) + g.h_ell_m) * sl,
    )
}

/// Bowring's method; sub-mm for terrestrial/aviation heights.
pub fn ecef_to_geodetic(p: Vec3) -> Geodetic {
    let b = WGS84_A * (1.0 - WGS84_F);
    let ep2 = (WGS84_A * WGS84_A - b * b) / (b * b);
    let r = (p.x * p.x + p.y * p.y).sqrt();
    let th = (p.z * WGS84_A).atan2(r * b);
    let (st, ct) = th.sin_cos();
    let lat = (p.z + ep2 * b * st.powi(3)).atan2(r - WGS84_E2 * WGS84_A * ct.powi(3));
    let sl = lat.sin();
    let n = WGS84_A / (1.0 - WGS84_E2 * sl * sl).sqrt();
    Geodetic {
        lat_deg: lat.to_degrees(),
        lon_deg: p.y.atan2(p.x).to_degrees(),
        h_ell_m: r / lat.cos() - n,
    }
}

/// Unit vector of local "up" (ellipsoid normal) at a geodetic position.
pub fn up_vector(lat_deg: f64, lon_deg: f64) -> Vec3 {
    let (lat, lon) = (lat_deg.to_radians(), lon_deg.to_radians());
    Vec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin())
}

/// East/North unit vectors at a geodetic position (for wind and velocity conversion).
pub fn east_north(lat_deg: f64, lon_deg: f64) -> (Vec3, Vec3) {
    let (lat, lon) = (lat_deg.to_radians(), lon_deg.to_radians());
    let east = Vec3::new(-lon.sin(), lon.cos(), 0.0);
    let north = Vec3::new(-lat.sin() * lon.cos(), -lat.sin() * lon.sin(), lat.cos());
    (east, north)
}

/// Meridional (M) and prime-vertical (N) radii of curvature in metres: dn = M·dφ, de = N·cosφ·dλ.
pub fn radii_of_curvature(lat_deg: f64) -> (f64, f64) {
    let s = lat_deg.to_radians().sin();
    let w = 1.0 - WGS84_E2 * s * s;
    (WGS84_A * (1.0 - WGS84_E2) / w.powf(1.5), WGS84_A / w.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geodetic_roundtrip() {
        let g = Geodetic { lat_deg: 40.8735, lon_deg: -96.7, h_ell_m: 2500.0 };
        let back = ecef_to_geodetic(geodetic_to_ecef(g));
        assert!((back.lat_deg - g.lat_deg).abs() < 1e-9);
        assert!((back.lon_deg - g.lon_deg).abs() < 1e-9);
        assert!((back.h_ell_m - g.h_ell_m).abs() < 1e-3);
    }
}
