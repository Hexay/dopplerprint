//! Plain result rows filled by `search` and consumed by every writer (GeoPackage, GeoJSON, viewer).
//! Lat/lon are WGS84 degrees; cells are EPSG:5070 squares.

use crate::geo::albers::Cell;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Results {
    pub run: RunInfo,
    pub cells: Vec<CellRow>,
    pub candidates: Vec<CandidateRow>,
    pub tracks: Vec<TrackRow>,
    pub events: Vec<EventRow>,
    pub regions: Vec<RegionRow>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct RunInfo {
    pub audio: String,
    pub date_utc: String,
    pub version: String,
    /// Free-form note shown in the viewer header (config name, experiment, ...).
    pub note: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CellRow {
    pub cell: Cell,
    /// Combined locked score; higher is better.
    pub score: f64,
    /// One entry per event, in `Results::events` order.
    pub event_scores: Vec<f64>,
    /// Refinement level (0 = coarsest).
    pub level: u8,
    pub in_cr90: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CandidateRow {
    pub rank: u32,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub score: f64,
    pub event_scores: Vec<f64>,
    /// Matched flight per event (track id), in `Results::events` order.
    pub flight_ids: Vec<String>,
    /// Audio-relative seconds to UNIX seconds: t_unix = t_audio + clock_offset_s.
    pub clock_offset_s: f64,
    pub posterior: f64,
    pub reliable: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TrackRow {
    pub id: String,
    pub icao24: String,
    pub type_code: String,
    /// `EngineClass` name in lower case (piston, turboprop, jet, helicopter, unknown).
    pub engine: String,
    pub t_start: f64,
    pub t_end: f64,
    /// (lat_deg, lon_deg, h_ell_m) vertices.
    pub points: Vec<[f64; 3]>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EventRow {
    pub index: u32,
    pub name: String,
    /// Audio-relative seconds of the received CPA.
    pub t_cpa_s: f64,
    pub f0_hz: f64,
    pub band_lo_hz: f64,
    pub band_hi_hz: f64,
    pub snr_db: f64,
    pub flight_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegionKind {
    /// Highest-posterior-density region holding 90 % of the mass.
    Credible90,
    /// 1-sigma (or chosen-level) covariance ellipse of a mode.
    Ellipse,
}

impl RegionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RegionKind::Credible90 => "credible90",
            RegionKind::Ellipse => "ellipse",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RegionRow {
    pub rank: u32,
    pub kind: RegionKind,
    pub mass: f64,
    pub area_km2: f64,
    /// Closed outer ring of (lat_deg, lon_deg).
    pub ring: Vec<[f64; 2]>,
}

/// Min-max normalised scores in [0, 1] (all 1 when constant), used for fixed-class styling.
pub fn normalise(scores: impl Iterator<Item = f64> + Clone) -> Vec<f64> {
    let (lo, hi) = scores.clone().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), s| (l.min(s), h.max(s)));
    scores.map(|s| if hi > lo { (s - lo) / (hi - lo) } else { 1.0 }).collect()
}

/// Closed `n`-vertex ring of an ellipse around (lat, lon) in local east/north metres: semi-axes in metres,
/// major axis bearing clockwise from true north. Local tangent plane, so keep axes under ~20 km.
pub fn ellipse_ring(lat_deg: f64, lon_deg: f64, major_m: f64, minor_m: f64, bearing_deg: f64, n: usize) -> Vec<[f64; 2]> {
    let (m, nr) = crate::geo::radii_of_curvature(lat_deg);
    let (sb, cb) = bearing_deg.to_radians().sin_cos();
    (0..=n)
        .map(|i| {
            let t = std::f64::consts::TAU * (i % n) as f64 / n as f64;
            let (a, b) = (major_m * t.cos(), minor_m * t.sin());
            let (east, north) = (a * sb + b * cb, a * cb - b * sb);
            [lat_deg + (north / m).to_degrees(), lon_deg + (east / (nr * lat_deg.to_radians().cos())).to_degrees()]
        })
        .collect()
}
