//! icao24 / ICAO type designator → engine class.

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
