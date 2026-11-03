//! readsb `trace_full_*.json` parser (adsb.lol / ADSBx globe_history). Field layout: docs/research/adsb_data.md §2.1.

use std::fmt;
use std::io::Read;

use serde::de::{self, Deserializer, IgnoredAny, SeqAccess, Visitor};
use serde::Deserialize;

use super::model::source_code;

/// Flag bits of trace slot 6.
pub const FLAG_STALE: u8 = 1;
pub const FLAG_NEW_LEG: u8 = 2;
pub const FLAG_VRATE_GEOM: u8 = 4;
pub const FLAG_ALT_GEOM: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alt {
    Ground,
    Feet(f64),
    Missing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawPoint {
    /// Seconds after the file-level `timestamp`.
    pub dt: f64,
    pub lat: f64,
    pub lon: f64,
    pub alt: Alt,
    pub gs_kt: Option<f64>,
    pub track_deg: Option<f64>,
    pub flags: u8,
    pub baro_rate_fpm: Option<f64>,
    pub category: Option<String>,
    pub src: u8,
    pub alt_geom_ft: Option<f64>,
    pub geom_rate_fpm: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RawTrace {
    pub icao: String,
    #[serde(default)]
    pub r: Option<String>,
    #[serde(default)]
    pub t: Option<String>,
    pub timestamp: f64,
    pub trace: Vec<RawPoint>,
}

impl RawTrace {
    /// First ADS-B emitter category seen in the per-point aircraft objects (e.g. "A1").
    pub fn category(&self) -> Option<&str> {
        self.trace.iter().find_map(|p| p.category.as_deref())
    }
}

/// Parse a trace file's bytes, gunzipping when they carry the gzip magic (adsb.lol names gzip `.json`).
pub fn parse_trace(bytes: &[u8]) -> anyhow::Result<RawTrace> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut buf = Vec::with_capacity(bytes.len() * 6);
        flate2::read::MultiGzDecoder::new(bytes).read_to_end(&mut buf)?;
        Ok(serde_json::from_slice(&buf)?)
    } else {
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[derive(Deserialize)]
struct AircraftObj {
    category: Option<String>,
}

struct AltField(Alt);

impl<'de> Deserialize<'de> for AltField {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = AltField;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("altitude number, \"ground\" or null")
            }
            fn visit_f64<E>(self, v: f64) -> Result<AltField, E> {
                Ok(AltField(Alt::Feet(v)))
            }
            fn visit_i64<E>(self, v: i64) -> Result<AltField, E> {
                Ok(AltField(Alt::Feet(v as f64)))
            }
            fn visit_u64<E>(self, v: u64) -> Result<AltField, E> {
                Ok(AltField(Alt::Feet(v as f64)))
            }
            fn visit_str<E>(self, v: &str) -> Result<AltField, E> {
                Ok(AltField(if v == "ground" { Alt::Ground } else { Alt::Missing }))
            }
            fn visit_unit<E>(self) -> Result<AltField, E> {
                Ok(AltField(Alt::Missing))
            }
            fn visit_none<E>(self) -> Result<AltField, E> {
                Ok(AltField(Alt::Missing))
            }
        }
        d.deserialize_any(V)
    }
}

impl<'de> Deserialize<'de> for RawPoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_seq(PointVisitor)
    }
}

struct PointVisitor;

fn req<'de, A: SeqAccess<'de>, T: Deserialize<'de>>(seq: &mut A, i: usize) -> Result<T, A::Error> {
    seq.next_element()?.ok_or_else(|| de::Error::invalid_length(i, &"a readsb trace point"))
}

fn opt<'de, A: SeqAccess<'de>, T: Deserialize<'de>>(seq: &mut A) -> Result<Option<T>, A::Error> {
    Ok(seq.next_element::<Option<T>>()?.flatten())
}

impl<'de> Visitor<'de> for PointVisitor {
    type Value = RawPoint;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a readsb trace point array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<RawPoint, A::Error> {
        let dt = req(&mut seq, 0)?;
        let lat = req(&mut seq, 1)?;
        let lon = req(&mut seq, 2)?;
        let alt = seq.next_element::<AltField>()?.map_or(Alt::Missing, |a| a.0);
        let gs_kt = opt(&mut seq)?;
        let track_deg = opt(&mut seq)?;
        let flags = opt::<_, u8>(&mut seq)?.unwrap_or(0);
        let baro_rate_fpm = opt(&mut seq)?;
        let category = opt::<_, AircraftObj>(&mut seq)?.and_then(|a| a.category);
        let src = opt::<_, String>(&mut seq)?.map_or(source_code(""), |s| source_code(&s));
        let alt_geom_ft = opt(&mut seq)?;
        let geom_rate_fpm = opt(&mut seq)?;
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(RawPoint {
            dt,
            lat,
            lon,
            alt,
            gs_kt,
            track_deg,
            flags,
            baro_rate_fpm,
            category,
            src,
            alt_geom_ft,
            geom_rate_fpm,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_points() {
        let json = r#"{"icao":"~28f542","timestamp":1788825600.0,"trace":[
            [1.5,40.1,-83.2,"ground",0.0,12.0,0,null,null,"adsb_icao",null,null,null,null],
            [2.5,40.1,-83.2,1250,66.3,84.8,5,576,{"category":"A1","nac_p":9},"tisb_other",1475,576],
            [3.5,40.1,-83.2,null,null,null,0,null,null,"mlat"]]}"#;
        let t = parse_trace(json.as_bytes()).unwrap();
        assert_eq!(t.trace.len(), 3);
        assert_eq!(t.trace[0].alt, Alt::Ground);
        assert_eq!(t.trace[1].alt, Alt::Feet(1250.0));
        assert_eq!(t.trace[1].alt_geom_ft, Some(1475.0));
        assert_eq!(t.trace[1].src, source_code("tisb_other"));
        assert_eq!(t.category(), Some("A1"));
        assert_eq!(t.trace[2].alt, Alt::Missing);
        assert_eq!(t.trace[2].alt_geom_ft, None);
        assert_eq!(t.t, None);
    }
}
