//! Cut clean tracks into short `segments`-table pieces with tight bboxes and a spatial sort key.

use super::model::{AltSource, CleanTrack, ground_dist_m};

pub const MAX_PIECE_S: f64 = 60.0;
pub const MAX_PIECE_M: f64 = 5_000.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub seg_id: u64,
    pub icao: u32,
    pub track_no: u32,
    pub hour: i32,
    pub hilbert: u64,
    pub t0: f64,
    pub t1: f64,
    pub lat_min: f64,
    pub lat_max: f64,
    pub lon_min: f64,
    pub lon_max: f64,
    pub alt_min_m: f32,
    pub alt_max_m: f32,
    pub n_points: u32,
    /// Worst (highest code) altitude basis in the piece.
    pub alt_source: AltSource,
    pub src: u8,
}

/// Hour of `t` relative to the UTC day start `day0` (may be −1 or ≥ 24 for traces spilling past midnight).
pub fn hour_of(t: f64, day0: f64) -> i32 {
    ((t - day0) / 3600.0).floor() as i32
}

/// Consecutive pieces share their boundary point, so every point-to-point leg lies inside one bbox.
pub fn cut_pieces(tr: &CleanTrack, day0: f64) -> Vec<Piece> {
    let pts = &tr.points;
    let mut out = Vec::new();
    let mut start = 0;
    while start + 1 < pts.len() {
        let mut end = start + 1;
        let mut len = ground_dist_m(pts[start].lat, pts[start].lon, pts[end].lat, pts[end].lon);
        while end + 1 < pts.len() {
            let step = ground_dist_m(pts[end].lat, pts[end].lon, pts[end + 1].lat, pts[end + 1].lon);
            if pts[end + 1].t - pts[start].t > MAX_PIECE_S || len + step > MAX_PIECE_M {
                break;
            }
            len += step;
            end += 1;
        }
        let run = &pts[start..=end];
        let fold = |f: fn(f64, f64) -> f64, g: fn(&super::model::CleanPoint) -> f64| {
            run.iter().map(g).reduce(f).unwrap_or(f64::NAN)
        };
        let (lat_min, lat_max) = (fold(f64::min, |p| p.lat), fold(f64::max, |p| p.lat));
        let (lon_min, lon_max) = (fold(f64::min, |p| p.lon), fold(f64::max, |p| p.lon));
        let piece_idx = out.len() as u64;
        out.push(Piece {
            seg_id: ((tr.icao as u64) << 39) | (((tr.track_no as u64) & 0x7fff) << 24) | (piece_idx & 0xff_ffff),
            icao: tr.icao,
            track_no: tr.track_no,
            hour: hour_of(run[0].t, day0),
            hilbert: hilbert_latlon((lat_min + lat_max) * 0.5, (lon_min + lon_max) * 0.5),
            t0: run[0].t,
            t1: run[run.len() - 1].t,
            lat_min,
            lat_max,
            lon_min,
            lon_max,
            alt_min_m: fold(f64::min, |p| p.alt_m) as f32,
            alt_max_m: fold(f64::max, |p| p.alt_m) as f32,
            n_points: run.len() as u32,
            alt_source: run.iter().map(|p| p.alt_source).max().unwrap_or(AltSource::Baro),
            src: run[0].src,
        });
        start = end;
    }
    out
}

/// 32-bit-per-axis Hilbert index of a lat/lon on an equirectangular grid (~1 cm cells).
pub fn hilbert_latlon(lat: f64, lon: f64) -> u64 {
    let scale = |v: f64, lo: f64, span: f64| (((v - lo) / span).clamp(0.0, 1.0) * u32::MAX as f64) as u32;
    hilbert_xy(scale(lon, -180.0, 360.0), scale(lat, -90.0, 180.0))
}

fn hilbert_xy(mut x: u32, mut y: u32) -> u64 {
    let mut d: u64 = 0;
    let mut s: u32 = 1 << 31;
    while s > 0 {
        let rx = u32::from(x & s != 0);
        let ry = u32::from(y & s != 0);
        d += (s as u64) * (s as u64) * u64::from((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = !x;
                y = !y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        s >>= 1;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hilbert_neighbours_are_close() {
        let a = hilbert_latlon(40.0, -73.0);
        let b = hilbert_latlon(40.000_01, -73.000_01);
        let far = hilbert_latlon(-33.0, 151.0);
        assert!(a.abs_diff(b) < a.abs_diff(far));
        assert_eq!(hilbert_xy(0, 0), 0);
    }
}
