//! In-memory cleaned ADS-B records shared by ingest and query, plus their on-disk integer codes.

use crate::geo::{Geodetic, east_north, geodetic_to_ecef, up_vector};
use crate::track::{EngineClass, TrackPoint};

/// Bit 24 marks a non-ICAO (`~`-prefixed) readsb address: TIS-B, anonymous UAT and similar.
pub const NON_ICAO_BIT: u32 = 1 << 24;

/// Altitude basis of a stored point. `Geometric` is WGS84 ellipsoidal height.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[repr(u8)]
pub enum AltSource {
    Geometric = 0,
    /// Barometric plus this aircraft's own interpolated (geometric − baro) offset.
    BaroOffset = 1,
    /// Pressure altitude only; not a height above anything.
    Baro = 2,
}

impl AltSource {
    pub fn from_code(c: u8) -> Self {
        match c {
            0 => Self::Geometric,
            1 => Self::BaroOffset,
            _ => Self::Baro,
        }
    }
}

/// readsb `type` strings, indexed by the stored `src` code; anything else is `SOURCES.len()`.
pub const SOURCES: [&str; 12] = [
    "adsb_icao",
    "adsb_icao_nt",
    "adsr_icao",
    "tisb_icao",
    "tisb_other",
    "tisb_trackfile",
    "mlat",
    "adsb_other",
    "adsr_other",
    "mode_s",
    "adsc",
    "other",
];

pub fn source_code(s: &str) -> u8 {
    SOURCES.iter().position(|x| *x == s).unwrap_or(SOURCES.len()) as u8
}

pub fn engine_code(e: EngineClass) -> u8 {
    match e {
        EngineClass::Piston => 0,
        EngineClass::Turboprop => 1,
        EngineClass::Jet => 2,
        EngineClass::Helicopter => 3,
        EngineClass::Unknown => 4,
    }
}

pub fn engine_from_code(c: u8) -> EngineClass {
    match c {
        0 => EngineClass::Piston,
        1 => EngineClass::Turboprop,
        2 => EngineClass::Jet,
        3 => EngineClass::Helicopter,
        _ => EngineClass::Unknown,
    }
}

/// Case-insensitive engine name ("jet", "piston", "turboprop", "helicopter", "unknown").
pub fn engine_from_name(s: &str) -> Option<EngineClass> {
    let all = [EngineClass::Piston, EngineClass::Turboprop, EngineClass::Jet, EngineClass::Helicopter, EngineClass::Unknown];
    all.into_iter().find(|e| format!("{e:?}").eq_ignore_ascii_case(s.trim()))
}

/// `"a03a42"` → 0xa03a42; `"~28f542"` → 0x28f542 | NON_ICAO_BIT.
pub fn parse_icao(s: &str) -> Option<u32> {
    let (hex, extra) = match s.strip_prefix('~') {
        Some(rest) => (rest, NON_ICAO_BIT),
        None => (s, 0),
    };
    u32::from_str_radix(hex, 16).ok().filter(|v| *v < NON_ICAO_BIT).map(|v| v | extra)
}

pub fn format_icao(icao: u32) -> String {
    let hex = format!("{:06x}", icao & (NON_ICAO_BIT - 1));
    if icao & NON_ICAO_BIT != 0 { format!("~{hex}") } else { hex }
}

/// Stable track id: `<icao>-<track_no>`, e.g. `a03a42-1`.
pub fn track_id(icao: u32, track_no: u32) -> String {
    format!("{}-{track_no}", format_icao(icao))
}

/// One cleaned sample: UNIX seconds, degrees, metres, and ENU velocity in m/s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CleanPoint {
    pub t: f64,
    pub lat: f64,
    pub lon: f64,
    pub alt_m: f64,
    pub alt_source: AltSource,
    pub ve: f64,
    pub vn: f64,
    pub vu: f64,
    pub src: u8,
    pub flags: u8,
}

impl CleanPoint {
    pub fn to_track_point(&self) -> TrackPoint {
        enu_to_track_point(self.t, self.lat, self.lon, self.alt_m, [self.ve, self.vn, self.vu])
    }
}

pub fn enu_to_track_point(t: f64, lat: f64, lon: f64, alt_m: f64, v_enu: [f64; 3]) -> TrackPoint {
    let pos = geodetic_to_ecef(Geodetic { lat_deg: lat, lon_deg: lon, h_ell_m: alt_m });
    let (e, n) = east_north(lat, lon);
    let vel = e * v_enu[0] + n * v_enu[1] + up_vector(lat, lon) * v_enu[2];
    TrackPoint { t, pos, vel }
}

/// A gap-free run of cleaned points from one aircraft.
#[derive(Clone, Debug)]
pub struct CleanTrack {
    pub icao: u32,
    pub track_no: u32,
    pub type_code: Option<String>,
    pub engine: EngineClass,
    pub points: Vec<CleanPoint>,
}

/// Equirectangular ground distance in metres; exact enough for consecutive ADS-B points.
pub fn ground_dist_m(lat0: f64, lon0: f64, lat1: f64, lon1: f64) -> f64 {
    const R: f64 = 6_371_008.8;
    let mut dlon = lon1 - lon0;
    if dlon > 180.0 {
        dlon -= 360.0;
    } else if dlon < -180.0 {
        dlon += 360.0;
    }
    let x = dlon.to_radians() * ((lat0 + lat1) * 0.5).to_radians().cos();
    let y = (lat1 - lat0).to_radians();
    R * x.hypot(y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icao_roundtrip() {
        for s in ["a03a42", "~28f542", "000001"] {
            assert_eq!(format_icao(parse_icao(s).unwrap()), s);
        }
        assert_eq!(parse_icao("zz"), None);
    }

    #[test]
    fn ecef_velocity_magnitude_matches_enu() {
        let tp = enu_to_track_point(0.0, 40.9, -73.5, 500.0, [30.0, 40.0, 2.0]);
        assert!((tp.vel.norm() - (2500.0f64 + 4.0).sqrt()).abs() < 1e-9);
    }
}
