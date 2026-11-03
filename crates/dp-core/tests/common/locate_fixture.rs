//! An offline `locate` setup shared by dp-core's and dp-cli's tests (`#[path]`-included): a cache holding a
//! store built from the ADS-B fixtures, and audio synthesised by `sim` for an observer under the fixture C172.

use std::path::{Path, PathBuf};

use dp_core::adsb::manifest::civil_date;
use dp_core::adsb::{IngestOptions, TrackQuery, ingest_day, load_tracks};
use dp_core::dem::geoid_undulation;
use dp_core::geo::{Geodetic, ecef_to_geodetic, geodetic_to_ecef, east_north, up_vector};
use dp_core::sim::{self, Scenario};
use dp_core::track::Track;

/// The C172 (a03a42, N1134F) flies a downwind leg west at ~480 m over Marion, OH around this time.
const T_MID: f64 = 1_788_873_860.0;
const DURATION_S: f64 = 120.0;
const OFFSET_SOUTH_M: f64 = 700.0;

pub struct Fixture {
    pub tmp: tempfile::TempDir,
    pub cache: PathBuf,
    pub audio: PathBuf,
    /// Fewer refined cells and a narrower clock sweep than the defaults keep a locate to a few seconds.
    pub config: PathBuf,
    /// Observer (lat, lon, h_ell).
    pub obs: (f64, f64, f64),
    /// UNIX start of the recording.
    pub start: f64,
}

impl Fixture {
    pub fn build() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache");
        cache_with_store(&cache);
        let tr = c172(&cache.join("adsb/store"));
        let obs = observer(&tr);
        let start = T_MID - DURATION_S / 2.0;
        let r = sim::render(&scenario(&tr, obs, start));
        let rec = tmp.path().join("rec");
        sim::write_outputs(&r, &rec).unwrap();
        let config = tmp.path().join("fast.toml");
        std::fs::write(&config, "[search]\nkeep_top = 40\n[search.score]\nclock_sweep_s = 2.0\n").unwrap();
        Fixture { cache, audio: rec.join("audio.wav"), config, obs, start, tmp }
    }

    /// `near` 1.5 km around a point ~700 m off the observer, as a user would roughly give it.
    pub fn near(&self) -> String {
        format!("{:.4},{:.4},1.5", self.obs.0 + 0.006, self.obs.1 - 0.004)
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dp-core/tests/fixtures/adsb")
}

fn cache_with_store(root: &Path) {
    let typedb = root.join("typedb");
    std::fs::create_dir_all(&typedb).unwrap();
    for f in ["aircraft.csv", "icao_aircraft_types.json"] {
        std::fs::copy(fixtures().join("typedb").join(f), typedb.join(f)).unwrap();
    }
    let opts = IngestOptions { typedb_dir: Some(typedb), batch_files: 3, ..IngestOptions::default() };
    ingest_day(&fixtures().join("traces"), &root.join("adsb/store"), &opts).unwrap();
}

fn c172(store: &Path) -> Track {
    let q = TrackQuery { t_start: T_MID - 600.0, t_end: T_MID + 600.0, bbox: None, max_alt_m: None, exclude_engines: vec![] };
    let tracks = load_tracks(store, &q).unwrap();
    tracks.into_iter().find(|t| t.id.contains("/a03a42-") && t.state_at(T_MID).is_some()).expect("C172 track at T_MID")
}

/// Observer on flat ground at the geoid (what `locate` assumes without a DEM), south of the leg.
fn observer(tr: &Track) -> (f64, f64, f64) {
    let g = ecef_to_geodetic(tr.state_at(T_MID).unwrap().pos);
    let lat = g.lat_deg - OFFSET_SOUTH_M / 111_000.0;
    (lat, g.lon_deg, geoid_undulation(lat, g.lon_deg))
}

/// Waypoints along the real track (in the observer's tangent frame), so `sim` renders the logged flight.
fn scenario(tr: &Track, (lat, lon, h): (f64, f64, f64), start: f64) -> Scenario {
    let ground = geodetic_to_ecef(Geodetic { lat_deg: lat, lon_deg: lon, h_ell_m: h });
    let ((e, n), u) = (east_north(lat, lon), up_vector(lat, lon));
    let (a, b) = ((start - 30.0).max(tr.t_start()), (start + DURATION_S + 30.0).min(tr.t_end()));
    let points: Vec<String> = (0..)
        .map(|k| a + 2.0 * k as f64)
        .take_while(|t| *t <= b)
        .map(|t| {
            let d = tr.state_at(t).unwrap().pos - ground;
            format!("[{:.3}, {:.2}, {:.2}, {:.2}]", t - start, d.dot(e), d.dot(n), d.dot(u))
        })
        .collect();
    let toml = format!(
        r#"seed = 7
sample_rate = 8000
duration_s = {DURATION_S}
start_unix = {start}
[observer]
lat_deg = {lat}
lon_deg = {lon}
h_ell_m = {h}
mic_height_m = 1.5
[noise]
snr_db = 30.0
[[aircraft]]
id = "c172"
engine = "Piston"
amplitude = 1.0
[aircraft.source]
f0 = 84.0
harmonics = [1.0, 0.7, 0.5, 0.35]
[aircraft.waypoints]
points = [{}]
"#,
        points.join(", ")
    );
    Scenario::from_toml_str(&toml).unwrap()
}

/// `2026-09-08 13:00:20 -04:00` for a UNIX time, written in US Eastern daylight time as a user would.
pub fn local_edt(t: f64) -> String {
    let l = (t - 4.0 * 3600.0).round() as i64;
    let s = l.rem_euclid(86_400);
    format!("{} {:02}:{:02}:{:02} -04:00", civil_date(l as f64), s / 3600, s % 3600 / 60, s % 60)
}
