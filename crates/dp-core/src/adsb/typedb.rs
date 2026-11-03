//! icao24 / ICAO type designator → engine class.
//! Join order (research/adsb_data.md §5): trace `t` → registry `aircraft.csv` type → ADS-B category → Unknown.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::track::EngineClass;

/// Map an ICAO type description like "L1P", "L2T", "L2J", "H1T" to an engine class.
pub fn engine_from_description(desc: &str) -> EngineClass {
    let b = desc.as_bytes();
    if b.first() == Some(&b'H') || b.first() == Some(&b'G') {
        return EngineClass::Helicopter;
    }
    match b.get(2) {
        Some(b'P') => EngineClass::Piston,
        Some(b'T') => EngineClass::Turboprop,
        Some(b'J') => EngineClass::Jet,
        _ => EngineClass::Unknown,
    }
}

/// Where an engine class came from; stored as `engine_source` in the segments table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EngineSource {
    None = 0,
    TraceType = 1,
    RegistryType = 2,
    Category = 3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Classification {
    pub type_code: Option<String>,
    pub engine: EngineClass,
    pub source: EngineSource,
}

const TYPE_FILES: [&str; 3] = ["icao_aircraft_types.json", "icao_aircraft_types.js", "tar1090-icao_aircraft_types.json"];
const REGISTRY_FILES: [&str; 2] = ["aircraft.csv.gz", "aircraft.csv"];

#[derive(Clone, Debug, Default)]
pub struct TypeDb {
    /// Type designator → ICAO description ("L1P").
    pub types: HashMap<String, String>,
    /// icao24 → type designator.
    pub registry: HashMap<u32, String>,
    pub files: Vec<PathBuf>,
}

fn read_maybe_gz(path: &Path) -> anyhow::Result<Vec<u8>> {
    let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if !raw.starts_with(&[0x1f, 0x8b]) {
        return Ok(raw);
    }
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(&raw[..]).read_to_end(&mut out)?;
    Ok(out)
}

impl TypeDb {
    /// Load tar1090-db files from `dir`; either file may be absent (gzip or plain, by magic).
    pub fn load_dir(dir: &Path) -> anyhow::Result<Self> {
        let mut db = TypeDb::default();
        if let Some(p) = TYPE_FILES.iter().map(|f| dir.join(f)).find(|p| p.exists()) {
            #[derive(serde::Deserialize)]
            struct Entry {
                desc: Option<String>,
            }
            let m: HashMap<String, Entry> = serde_json::from_slice(&read_maybe_gz(&p)?)?;
            db.types = m.into_iter().filter_map(|(k, v)| Some((k, v.desc?))).collect();
            db.files.push(p);
        }
        if let Some(p) = REGISTRY_FILES.iter().map(|f| dir.join(f)).find(|p| p.exists()) {
            db.registry = parse_registry(&read_maybe_gz(&p)?);
            db.files.push(p);
        }
        Ok(db)
    }

    pub fn classify(&self, icao: u32, trace_type: Option<&str>, category: Option<&str>) -> Classification {
        let trace_type = trace_type.map(str::trim).filter(|s| !s.is_empty());
        let (type_code, from) = match trace_type {
            Some(t) => (Some(t.to_string()), EngineSource::TraceType),
            None => (self.registry.get(&icao).cloned(), EngineSource::RegistryType),
        };
        let by_type = type_code.as_deref().and_then(|t| self.types.get(t)).map(|d| engine_from_description(d));
        let (engine, source) = match by_type {
            Some(e) if e != EngineClass::Unknown => (e, from),
            _ => match category.and_then(engine_from_category) {
                Some(e) => (e, EngineSource::Category),
                None => (EngineClass::Unknown, EngineSource::None),
            },
        };
        Classification { type_code, engine, source }
    }
}

/// ADS-B emitter category fallback; only A7 (rotorcraft) pins an engine class.
fn engine_from_category(cat: &str) -> Option<EngineClass> {
    (cat == "A7").then_some(EngineClass::Helicopter)
}

/// tar1090-db `aircraft.csv`: `ICAO;reg;type;dbFlags;desc;year;owner;`.
fn parse_registry(bytes: &[u8]) -> HashMap<u32, String> {
    let mut m = HashMap::with_capacity(700_000);
    for line in BufReader::new(bytes).lines().map_while(Result::ok) {
        let mut f = line.split(';');
        let (Some(icao), _, Some(ty)) = (f.next(), f.next(), f.next()) else { continue };
        if ty.is_empty() {
            continue;
        }
        if let Ok(v) = u32::from_str_radix(icao, 16) {
            m.insert(v, ty.to_string());
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions() {
        assert_eq!(engine_from_description("L1P"), EngineClass::Piston);
        assert_eq!(engine_from_description("L2T"), EngineClass::Turboprop);
        assert_eq!(engine_from_description("L2J"), EngineClass::Jet);
        assert_eq!(engine_from_description("H1T"), EngineClass::Helicopter);
        assert_eq!(engine_from_description(""), EngineClass::Unknown);
    }

    #[test]
    fn classify_join_order() {
        let mut db = TypeDb::default();
        db.types.insert("C172".into(), "L1P".into());
        db.types.insert("B407".into(), "H1T".into());
        db.registry.insert(0xabcdef, "B407".into());
        assert_eq!(db.classify(1, Some("C172"), None).engine, EngineClass::Piston);
        let reg = db.classify(0xabcdef, None, None);
        assert_eq!((reg.engine, reg.source), (EngineClass::Helicopter, EngineSource::RegistryType));
        assert_eq!(db.classify(2, Some("KODI"), Some("A7")).source, EngineSource::Category);
        let unk = db.classify(3, None, Some("A1"));
        assert_eq!((unk.engine, unk.type_code), (EngineClass::Unknown, None));
    }
}
