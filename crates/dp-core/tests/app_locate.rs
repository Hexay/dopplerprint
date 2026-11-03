//! Offline end-to-end `app::locate`: a store built from the ADS-B fixtures, audio synthesised by `sim` for an
//! observer under the fixture C172's pattern, and the full pipeline run against a cache holding that store.

use std::path::{Path, PathBuf};

use dp_core::adsb::manifest::civil_date;
use dp_core::adsb::{IngestOptions, TrackQuery, ingest_day, load_tracks};
use dp_core::app::{self, LocateRequest, QuietUi, Status};
use dp_core::dem::geoid_undulation;
use dp_core::eval::distance_m;
use dp_core::geo::{Geodetic, ecef_to_geodetic, geodetic_to_ecef, east_north, up_vector};
use dp_core::sim::{self, Scenario};
use dp_core::track::Track;

/// The C172 (a03a42, N1134F) flies a downwind leg west at ~480 m over Marion, OH around this time.
const T_MID: f64 = 1_788_873_860.0;
const DURATION_S: f64 = 120.0;
const OFFSET_SOUTH_M: f64 = 700.0;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/adsb")
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
fn local_edt(t: f64) -> String {
    let l = (t - 4.0 * 3600.0).round() as i64;
    let s = l.rem_euclid(86_400);
    format!("{} {:02}:{:02}:{:02} -04:00", civil_date(l as f64), s / 3600, s % 3600 / 60, s % 60)
}

#[test]
fn locates_synthetic_flyover_offline() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    cache_with_store(&cache);
    let tr = c172(&cache.join("adsb/store"));
    let obs = observer(&tr);
    let start = T_MID - DURATION_S / 2.0;
    let r = sim::render(&scenario(&tr, obs, start));
    let rec = tmp.path().join("rec");
    sim::write_outputs(&r, &rec).unwrap();

    // Fewer refined cells and a narrower clock sweep than the defaults keep this a few seconds.
    let fast = tmp.path().join("fast.toml");
    std::fs::write(&fast, "[search]\nkeep_top = 40\n[search.score]\nclock_sweep_s = 2.0\n").unwrap();
    let req = LocateRequest {
        audio: rec.join("audio.wav"),
        start: Some(local_edt(start + 20.0)),
        near: Some(format!("{:.4},{:.4},1.5", obs.0 + 0.006, obs.1 - 0.004)),
        cache: Some(cache.clone()),
        config: Some(fast),
        out: Some(tmp.path().join("out")),
        offline: true,
        ..LocateRequest::default()
    };
    let rep = app::locate(&req, &QuietUi).unwrap();
    eprintln!("{}locate took {:.1} s", app::text::render(&rep), rep.runtime_s);
    assert_eq!(rep.status, Status::Found, "{:?}", rep.message);
    let loc = rep.location.as_ref().unwrap();
    let err = distance_m(loc.lat, loc.lon, obs.0, obs.1);
    assert!(err < 400.0, "error {err:.0} m");
    let clock = rep.clock.as_ref().unwrap();
    assert!((clock.start_unix - start).abs() < 3.0 && (clock.offset_from_given_s + 20.0).abs() < 3.0, "{clock:?}");
    let ac = &rep.aircraft[0];
    assert_eq!((ac.icao24.as_str(), ac.registration.as_deref(), ac.type_code.as_deref()), ("a03a42", Some("N1134F"), Some("C172")));
    assert!(rep.terrain.contains("flat ground"));
    for f in &rep.files {
        assert!(std::fs::metadata(f).is_ok_and(|m| m.len() > 0), "{f}");
    }
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(tmp.path().join("out/result.json")).unwrap()).unwrap();
    assert_eq!(json["status"], "found");
}

#[test]
fn missing_inputs_and_offline_data_need_input() {
    let tmp = tempfile::tempdir().unwrap();
    let wav = tmp.path().join("quiet.wav");
    let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&wav, spec).unwrap();
    (0..8000).for_each(|i| w.write_sample(((i % 50) as i16 - 25) * 10).unwrap());
    w.finalize().unwrap();
    let base = LocateRequest { audio: wav, cache: Some(tmp.path().join("cache")), offline: true, ..LocateRequest::default() };
    let run = |r: &LocateRequest| app::locate(r, &QuietUi).unwrap();

    let no_start = run(&LocateRequest { near: Some("40.41,-83.2".into()), ..base.clone() });
    assert_eq!(no_start.status, Status::NeedsInput);
    assert!(no_start.message.as_deref().unwrap().contains("--start"));

    let no_near = run(&LocateRequest { start: Some("2026-09-08T17:00Z".into()), ..base.clone() });
    assert!(no_near.message.as_deref().unwrap().contains("--near"));

    let no_zone = run(&LocateRequest { start: Some("2026-09-08 13:00".into()), near: Some("40.41,-83.2".into()), ..base.clone() });
    assert!(no_zone.message.as_deref().unwrap().contains("UTC offset"));

    let quiet = run(&LocateRequest { start: Some("2026-09-08T17:00Z".into()), near: Some("40.41,-83.2".into()), ..base });
    assert_eq!(quiet.status, Status::Inconclusive, "{:?}", quiet.message);
    assert!(quiet.message.as_deref().unwrap().contains("no aircraft pass"));
    assert_eq!(quiet.status.exit_code(), 3);
}
