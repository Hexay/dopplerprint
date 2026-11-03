//! Per-point altitude basis selection: geometric (HAE) → baro + own geom−baro offset → baro.
//! Rationale: docs/research/adsb_data.md §2.2.

use super::model::AltSource;
use super::trace::{Alt, FLAG_ALT_GEOM, RawPoint};

pub const FT_TO_M: f64 = 0.3048;

/// An offset sample further than this from a baro-only point is not trusted to describe it.
pub const MAX_OFFSET_AGE_S: f64 = 600.0;

/// Altitude in metres and its basis for every point; `None` for ground and altitude-less points.
pub fn select_altitudes(points: &[RawPoint]) -> Vec<Option<(f64, AltSource)>> {
    let offsets: Vec<(f64, f64)> = points
        .iter()
        .filter_map(|p| match (p.alt, p.alt_geom_ft) {
            (Alt::Feet(b), Some(g)) if p.flags & FLAG_ALT_GEOM == 0 => Some((p.dt, g - b)),
            _ => None,
        })
        .collect();
    points
        .iter()
        .map(|p| {
            let (ft, src) = match (p.alt, p.alt_geom_ft) {
                (Alt::Ground, _) => return None,
                (Alt::Feet(a), _) if p.flags & FLAG_ALT_GEOM != 0 => (a, AltSource::Geometric),
                (_, Some(g)) => (g, AltSource::Geometric),
                (Alt::Feet(b), None) => match interp_offset(&offsets, p.dt) {
                    Some(off) => (b + off, AltSource::BaroOffset),
                    None => (b, AltSource::Baro),
                },
                (Alt::Missing, None) => return None,
            };
            Some((ft * FT_TO_M, src))
        })
        .collect()
}

/// Linear interpolation of (t, offset) samples sorted by t, clamped at the ends, within MAX_OFFSET_AGE_S.
fn interp_offset(offsets: &[(f64, f64)], t: f64) -> Option<f64> {
    let i = offsets.partition_point(|o| o.0 < t);
    let before = i.checked_sub(1).map(|j| offsets[j]);
    let after = offsets.get(i).copied();
    match (before, after) {
        (Some(a), Some(b)) if b.0 > a.0 && b.0 - a.0 <= 2.0 * MAX_OFFSET_AGE_S => {
            Some(a.1 + (b.1 - a.1) * (t - a.0) / (b.0 - a.0))
        }
        (Some(a), Some(b)) => {
            let nearest = if t - a.0 <= b.0 - t { a } else { b };
            ((nearest.0 - t).abs() <= MAX_OFFSET_AGE_S).then_some(nearest.1)
        }
        (Some(o), None) | (None, Some(o)) => ((o.0 - t).abs() <= MAX_OFFSET_AGE_S).then_some(o.1),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(dt: f64, alt: Alt, geom: Option<f64>, flags: u8) -> RawPoint {
        RawPoint {
            dt,
            lat: 40.0,
            lon: -83.0,
            alt,
            gs_kt: None,
            track_deg: None,
            flags,
            baro_rate_fpm: None,
            category: None,
            src: 0,
            alt_geom_ft: geom,
            geom_rate_fpm: None,
        }
    }

    #[test]
    fn prefers_geometric_then_offset_then_baro() {
        let pts = [
            pt(0.0, Alt::Feet(1000.0), Some(1100.0), 0),
            pt(10.0, Alt::Feet(1000.0), None, 0),
            pt(20.0, Alt::Feet(2000.0), None, FLAG_ALT_GEOM),
            pt(30.0, Alt::Ground, Some(50.0), 0),
            pt(5000.0, Alt::Feet(3000.0), None, 0),
            pt(5001.0, Alt::Missing, None, 0),
        ];
        let a = select_altitudes(&pts);
        assert_eq!(a[0], Some((1100.0 * FT_TO_M, AltSource::Geometric)));
        assert_eq!(a[1], Some((1100.0 * FT_TO_M, AltSource::BaroOffset)));
        assert_eq!(a[2], Some((2000.0 * FT_TO_M, AltSource::Geometric)));
        assert_eq!(a[3], None);
        assert_eq!(a[4], Some((3000.0 * FT_TO_M, AltSource::Baro)));
        assert_eq!(a[5], None);
    }

    #[test]
    fn offset_interpolates_between_samples() {
        let o = [(0.0, 100.0), (100.0, 200.0)];
        assert_eq!(interp_offset(&o, 50.0), Some(150.0));
        assert_eq!(interp_offset(&o, -10.0), Some(100.0));
        assert_eq!(interp_offset(&o, 900.0), None);
    }
}
