//! Offline end-to-end `app::locate` against the shared fixture (tests/common/locate_fixture.rs).

#[path = "common/locate_fixture.rs"]
mod locate_fixture;

use dp_core::adsb::manifest::civil_date;
use dp_core::app::{self, LocateRequest, QuietUi, Status};
use dp_core::eval::distance_m;
use locate_fixture::{Fixture, local_edt};

#[test]
fn locates_synthetic_flyover_offline() {
    let fx = Fixture::build();
    let out = fx.tmp.path().join("out");
    let req = LocateRequest {
        audio: fx.audio.clone(),
        start: Some(local_edt(fx.start + 20.0)),
        near: Some(fx.near()),
        cache: Some(fx.cache.clone()),
        config: Some(fx.config.clone()),
        out: Some(out.clone()),
        offline: true,
        ..LocateRequest::default()
    };
    let rep = app::locate(&req, &QuietUi).unwrap();
    eprintln!("{}locate took {:.1} s", app::text::render(&rep), rep.runtime_s);
    assert_eq!(rep.status, Status::Found, "{:?}", rep.message);
    let loc = rep.location.as_ref().unwrap();
    let err = distance_m(loc.lat, loc.lon, fx.obs.0, fx.obs.1);
    assert!(err < 400.0, "error {err:.0} m");
    let clock = rep.clock.as_ref().unwrap();
    assert!((clock.start_unix - fx.start).abs() < 3.0 && (clock.offset_from_given_s + 20.0).abs() < 3.0, "{clock:?}");
    let ac = &rep.aircraft[0];
    assert_eq!((ac.icao24.as_str(), ac.registration.as_deref(), ac.type_code.as_deref()), ("a03a42", Some("N1134F"), Some("C172")));
    assert!(rep.terrain.contains("flat ground"));
    assert!(rep.air.starts_with("still air, c 343.0 m/s (offline"), "{}", rep.air);
    for f in &rep.files {
        assert!(std::fs::metadata(f).is_ok_and(|m| m.len() > 0), "{f}");
    }
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join("result.json")).unwrap()).unwrap();
    assert_eq!(json["status"], "found");

    // Cached archive weather (the Livermore fixture moved to this hour) is used offline and reported.
    let near: Vec<f64> = fx.near().split(',').map(|s| s.parse().unwrap()).collect();
    let mid = fx.start + 0.5 * rep.duration_s;
    let h0 = (mid / 3600.0).floor() * 3600.0 - 3600.0;
    let mut wx: serde_json::Value = serde_json::from_str(include_str!("../src/weather/fixture_livermore.json")).unwrap();
    wx["hourly"]["time"] = serde_json::json!([h0, h0 + 3600.0, h0 + 7200.0]);
    let file = dp_core::weather::openmeteo::cache_file(&fx.cache.join("weather"), near[0], near[1], &civil_date(mid));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, wx.to_string()).unwrap();
    let rep = app::locate(&req, &QuietUi).unwrap();
    assert!(rep.air.contains("averaged 0–") && rep.air.contains("Open-Meteo"), "{}", rep.air);
    let loc = rep.location.as_ref().unwrap();
    assert!(distance_m(loc.lat, loc.lon, fx.obs.0, fx.obs.1) < 400.0);
}

#[test]
fn nationwide_single_pass_asks_for_near_with_candidates() {
    let fx = Fixture::build();
    let req = LocateRequest {
        audio: fx.audio.clone(),
        start: Some(local_edt(fx.start + 20.0)),
        cache: Some(fx.cache.clone()),
        config: Some(fx.config.clone()),
        out: Some(fx.tmp.path().join("out")),
        offline: true,
        ..LocateRequest::default()
    };
    let rep = app::locate(&req, &QuietUi).unwrap();
    assert_eq!(rep.status, Status::NeedsInput, "{:?}", rep.message);
    let msg = rep.message.as_deref().unwrap();
    assert!(msg.starts_with("Searched the whole country:") && msg.contains("a03a42"), "{msg}");
    assert_eq!(app::missing_fields(&req, &rep), ["near"]);
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

    let no_start_req = LocateRequest { near: Some("40.41,-83.2".into()), ..base.clone() };
    let no_start = run(&no_start_req);
    assert_eq!(no_start.status, Status::NeedsInput);
    assert!(no_start.message.as_deref().unwrap().contains("--start"));
    assert_eq!(app::missing_fields(&no_start_req, &no_start), ["start"]);
    assert_eq!(app::missing_fields(&base, &run(&base)), ["start", "near"]);

    // Without --near the whole country is searched; quiet audio stops before that at "no aircraft pass".
    let no_near_req = LocateRequest { start: Some("2026-09-08T17:00Z".into()), ..base.clone() };
    let no_near = run(&no_near_req);
    assert_eq!(no_near.status, Status::Inconclusive, "{:?}", no_near.message);
    assert!(no_near.message.as_deref().unwrap().contains("no aircraft pass"));
    assert!(app::missing_fields(&no_near_req, &no_near).is_empty());

    let no_zone = run(&LocateRequest { start: Some("2026-09-08 13:00".into()), near: Some("40.41,-83.2".into()), ..base.clone() });
    assert!(no_zone.message.as_deref().unwrap().contains("UTC offset"));

    let quiet = run(&LocateRequest { start: Some("2026-09-08T17:00Z".into()), near: Some("40.41,-83.2".into()), ..base });
    assert_eq!(quiet.status, Status::Inconclusive, "{:?}", quiet.message);
    assert!(quiet.message.as_deref().unwrap().contains("no aircraft pass"));
    assert!(app::missing_fields(&no_start_req, &quiet).is_empty());
    assert_eq!(quiet.status.exit_code(), 3);
}
