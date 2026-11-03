//! EGM2008 undulation from the embedded CONUS crop built by python/dopplerprint/prep/geoid.py.

use std::sync::LazyLock;

static ASSET: &[u8] = include_bytes!("../../../../assets/geoid/egm2008_conus_2p5.bin");
const MAGIC: &[u8; 8] = b"DPGEOID1";
const HEADER_LEN: usize = 8 + 3 * 8 + 2 * 4 + 2 * 4;

struct Grid {
    lat_s: f64,
    lon_w: f64,
    step: f64,
    nrows: usize,
    ncols: usize,
    scale: f64,
    offset: f64,
}

static GRID: LazyLock<Grid> = LazyLock::new(|| {
    assert_eq!(&ASSET[..8], MAGIC, "bad geoid asset");
    let f64_at = |o: usize| f64::from_le_bytes(ASSET[o..o + 8].try_into().unwrap());
    let u32_at = |o: usize| u32::from_le_bytes(ASSET[o..o + 4].try_into().unwrap());
    let f32_at = |o: usize| f32::from_le_bytes(ASSET[o..o + 4].try_into().unwrap()) as f64;
    let g = Grid {
        lat_s: f64_at(8),
        lon_w: f64_at(16),
        step: f64_at(24),
        nrows: u32_at(32) as usize,
        ncols: u32_at(36) as usize,
        scale: f32_at(40),
        offset: f32_at(44),
    };
    assert_eq!(ASSET.len(), HEADER_LEN + 2 * g.nrows * g.ncols, "truncated geoid asset");
    g
});

impl Grid {
    fn node(&self, r: usize, c: usize) -> f64 {
        let o = HEADER_LEN + 2 * (r * self.ncols + c);
        u16::from_le_bytes([ASSET[o], ASSET[o + 1]]) as f64 * self.scale + self.offset
    }
}

/// EGM2008 undulation N (m): h_ellipsoid = H_msl + N. Bilinear on the 2.5' grid; positions outside the
/// CONUS crop (24..50 N, 125.5..66 W) are clamped to its edge.
pub fn geoid_undulation(lat_deg: f64, lon_deg: f64) -> f64 {
    let g = &*GRID;
    let fr = ((lat_deg - g.lat_s) / g.step).clamp(0.0, (g.nrows - 1) as f64);
    let fc = ((lon_deg - g.lon_w) / g.step).clamp(0.0, (g.ncols - 1) as f64);
    let (r, c) = ((fr as usize).min(g.nrows - 2), (fc as usize).min(g.ncols - 2));
    let (tr, tc) = (fr - r as f64, fc - c as f64);
    let lo = g.node(r, c) * (1.0 - tc) + g.node(r, c + 1) * tc;
    let hi = g.node(r + 1, c) * (1.0 - tc) + g.node(r + 1, c + 1) * tc;
    lo * (1.0 - tr) + hi * tr
}

#[cfg(test)]
mod tests {
    use super::*;

    /// pyproj (PROJ, us_nga_egm08_25.tif bilinear): EPSG:4979 h=0 -> EPSG:9518 H, N = -H.
    const PYPROJ: [(f64, f64, f64); 5] = [
        (24.5, -124.8, -45.4091),
        (40.876466, -73.524951, -31.7186),
        (49.4, -66.9, -24.2651),
        (37.66327, -121.7819, -31.9339),
        (45.5, -110.0, -8.7243),
    ];

    #[test]
    fn matches_pyproj() {
        for (lat, lon, n) in PYPROJ {
            let got = geoid_undulation(lat, lon);
            assert!((got - n).abs() < 0.01, "{lat},{lon}: {got} vs {n}");
        }
    }
}
