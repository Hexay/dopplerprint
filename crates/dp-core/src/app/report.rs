//! The locate report: best spot, how much better it is than the alternatives, and which flights it rests on.
//! Confidence levels are heuristics on the score gap (z units), not probabilities; see docs/USAGE.md#output.

use super::place::Area;
use super::when::{StartPrior, fmt_utc};
use crate::search::pipeline::Candidate;
use crate::track::{EngineClass, Track};

/// Alternatives must be this far from every spot already listed.
const MODE_SEP_M: f64 = 600.0;
/// Only alternatives within this score gap are worth mentioning.
const MODE_WINDOW: f64 = 10.0;
const MAX_ALTERNATIVES: usize = 3;
/// Below this per-event z a matched curve is not distinguishable from noise.
pub const MIN_EVENT_Z: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Found,
    NeedsInput,
    Inconclusive,
}

impl Status {
    pub fn exit_code(self) -> u8 {
        match self {
            Status::Found => 0,
            Status::NeedsInput => 2,
            Status::Inconclusive => 3,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Report {
    pub status: Status,
    /// Why there is no location (needs input / inconclusive).
    pub message: Option<String>,
    pub audio: String,
    pub duration_s: f64,
    pub location: Option<Location>,
    pub confidence: Option<Confidence>,
    /// Calibrated 90% region and mode probabilities.
    pub posterior: Option<super::posterior::Posterior>,
    pub alternatives: Vec<Alternative>,
    pub aircraft: Vec<Matched>,
    pub clock: Option<Clock>,
    pub start: Option<StartPrior>,
    pub area: Option<Area>,
    pub terrain: String,
    /// Model atmosphere: archived layer winds and c, or why still air was used.
    pub air: String,
    pub events_found: usize,
    pub events_used: usize,
    pub tracks_considered: usize,
    pub cells_scored: usize,
    pub runtime_s: f64,
    pub files: Vec<String>,
    /// Look-elsewhere check (`app::decoy`); `None` when no decoy ADS-B was cached or no answer was found.
    pub decoy: Option<super::decoy::DecoyCheck>,
}

impl Report {
    pub fn new(audio: &str) -> Self {
        Report {
            status: Status::Inconclusive,
            message: None,
            audio: audio.into(),
            duration_s: 0.0,
            location: None,
            confidence: None,
            posterior: None,
            alternatives: Vec::new(),
            aircraft: Vec::new(),
            clock: None,
            start: None,
            area: None,
            terrain: String::new(),
            air: String::new(),
            events_found: 0,
            events_used: 0,
            tracks_considered: 0,
            cells_scored: 0,
            runtime_s: 0.0,
            files: Vec::new(),
            decoy: None,
        }
    }

    pub fn stop(mut self, status: Status, message: impl Into<String>) -> Self {
        self.status = status;
        self.message = Some(message.into());
        self
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    pub osm_url: String,
    pub score: f64,
    /// See `Candidate::capped`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hypothesis_cap_hit: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Confidence {
    /// "high", "medium" or "low".
    pub level: String,
    /// Score of the best spot minus the best alternative (z units); `None` when nothing comes close.
    pub margin: Option<f64>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Alternative {
    pub lat: f64,
    pub lon: f64,
    pub distance_km: f64,
    /// 8-point compass direction from the best spot.
    pub direction: String,
    pub score_gap: f64,
    /// Same flights as the best spot: the mirror or along-track ambiguity of a pass.
    pub same_aircraft: bool,
    #[serde(skip)]
    pub rank: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Matched {
    pub event: usize,
    /// Audio-relative seconds of the closest approach as heard.
    pub t_audio_s: f64,
    pub icao24: String,
    pub registration: Option<String>,
    pub type_code: Option<String>,
    pub engine: String,
    /// Flagged, not dropped: a jet can be the source.
    pub jet: bool,
    pub cpa_utc: String,
    pub f0_hz: f64,
    pub z: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Clock {
    /// UNIX seconds of the first audio sample (`t_unix = t_audio + start_unix`).
    pub start_unix: f64,
    pub start_utc: String,
    /// Fitted start minus the given start.
    pub offset_from_given_s: f64,
}

pub fn osm_url(lat: f64, lon: f64) -> String {
    format!("https://www.openstreetmap.org/?mlat={lat:.5}&mlon={lon:.5}#map=15/{lat:.5}/{lon:.5}")
}

pub fn location(c: &Candidate) -> Location {
    Location {
        lat: c.lat_deg,
        lon: c.lon_deg,
        osm_url: osm_url(c.lat_deg, c.lon_deg),
        score: c.score,
        hypothesis_cap_hit: c.capped,
    }
}

/// Distinct alternatives to `ranked[0]` and the confidence that follows from their score gap.
pub fn alternatives(ranked: &[Candidate]) -> (Vec<Alternative>, Confidence) {
    let best = &ranked[0];
    let mut kept: Vec<&Candidate> = vec![best];
    let mut out = Vec::new();
    for (rank, c) in ranked.iter().enumerate().skip(1) {
        if out.len() >= MAX_ALTERNATIVES || best.score - c.score > MODE_WINDOW {
            break;
        }
        if kept.iter().any(|k| crate::eval::distance_m(k.lat_deg, k.lon_deg, c.lat_deg, c.lon_deg) < MODE_SEP_M) {
            continue;
        }
        kept.push(c);
        let (d, dir) = offset(best, c);
        out.push(Alternative {
            lat: c.lat_deg,
            lon: c.lon_deg,
            distance_km: d / 1000.0,
            direction: dir.into(),
            score_gap: best.score - c.score,
            same_aircraft: c.track_ids == best.track_ids,
            rank,
        });
    }
    let margin = out.first().map(|a| a.score_gap);
    let level = match margin {
        None => "high",
        Some(m) if m >= 6.0 => "high",
        Some(m) if m >= 2.0 => "medium",
        Some(_) => "low",
    };
    (out, Confidence { level: level.into(), margin })
}

fn offset(a: &Candidate, b: &Candidate) -> (f64, &'static str) {
    offset_to(a.lat_deg, a.lon_deg, b.lat_deg, b.lon_deg)
}

/// Distance (m) and 8-point compass direction from (lat1, lon1) to (lat2, lon2).
pub fn offset_to(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> (f64, &'static str) {
    let (m, n) = crate::geo::radii_of_curvature(lat1);
    let dn = (lat2 - lat1).to_radians() * m;
    let de = (lon2 - lon1).to_radians() * n * lat1.to_radians().cos();
    let bearing = de.atan2(dn).to_degrees().rem_euclid(360.0);
    const DIRS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    (dn.hypot(de), DIRS[((bearing + 22.5) / 45.0) as usize % 8])
}

/// Matched flight per event of the best spot. `t_cpa_audio[i]` is event i's heard CPA time.
pub fn matched(best: &Candidate, t_cpa_audio: &[f64], tracks: &[Track], reg: &dyn Fn(u32) -> Option<String>) -> Vec<Matched> {
    best.track_ids
        .iter()
        .zip(&best.events)
        .zip(t_cpa_audio)
        .enumerate()
        .map(|(i, ((id, es), &t))| {
            let tr = tracks.iter().find(|t| &t.id == id);
            let engine = tr.map_or(EngineClass::Unknown, |t| t.engine);
            Matched {
                event: i + 1,
                t_audio_s: t,
                icao24: tr.map_or_else(|| id.clone(), |t| icao_hex(t.icao24)),
                registration: tr.and_then(|t| reg(t.icao24)),
                type_code: tr.and_then(|t| t.type_code.clone()),
                engine: format!("{engine:?}").to_lowercase(),
                jet: engine == EngineClass::Jet,
                cpa_utc: fmt_utc(t + best.clock),
                f0_hz: es.f0,
                z: es.z,
            }
        })
        .collect()
}

/// Bit 24 marks a non-ICAO (`~`) address in the store.
pub fn icao_hex(icao: u32) -> String {
    if icao & (1 << 24) != 0 { format!("~{:06x}", icao & 0xff_ffff) } else { format!("{icao:06x}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::albers::Cell;

    fn cand(lat: f64, lon: f64, score: f64, ids: &[&str]) -> Candidate {
        Candidate {
            cell: Cell { ix: 0, iy: 0, step_m: 25 },
            lat_deg: lat,
            lon_deg: lon,
            score,
            raw: score,
            clock: 0.0,
            track_ids: ids.iter().map(|s| s.to_string()).collect(),
            events: Vec::new(),
            unexplained: 0,
            capped: false,
        }
    }

    #[test]
    fn mirror_is_reported_with_direction_and_low_confidence() {
        let ranked = [cand(37.68, -121.77, 20.0, &["a"]), cand(37.6801, -121.7701, 19.9, &["a"]), cand(37.6857, -121.7629, 19.0, &["a"])];
        let (alts, conf) = alternatives(&ranked);
        assert_eq!(alts.len(), 1);
        assert_eq!((alts[0].direction.as_str(), alts[0].same_aircraft, alts[0].rank), ("NE", true, 2));
        assert!((alts[0].distance_km - 0.9).abs() < 0.1, "{}", alts[0].distance_km);
        assert_eq!(conf.level, "low");
    }

    #[test]
    fn clear_winner_is_high() {
        let ranked = [cand(37.68, -121.77, 30.0, &["a"]), cand(37.60, -121.77, 22.0, &["b"])];
        let (alts, conf) = alternatives(&ranked);
        assert_eq!((alts[0].direction.as_str(), alts[0].same_aircraft), ("S", false));
        assert_eq!(conf.level, "high");
        assert_eq!(alternatives(&ranked[..1]).1.margin, None);
    }

    #[test]
    fn icao_formatting() {
        assert_eq!(icao_hex(0xa03a42), "a03a42");
        assert_eq!(icao_hex(0x1283635), "~283635");
        assert!(osm_url(37.682, -121.768).ends_with("#map=15/37.68200/-121.76800"));
    }
}
