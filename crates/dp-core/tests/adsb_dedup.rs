use dp_core::adsb::dedup::{DedupParams, drop_shadow_tracks};
use dp_core::adsb::model::{NON_ICAO_BIT, enu_to_track_point};
use dp_core::track::{EngineClass, Track};

/// Eastbound at 60 m/s from (lat, lon), one point every 5 s for 2 minutes.
fn track(id: &str, icao: u32, lat: f64, lon: f64, t0: f64) -> Track {
    let deg_per_m = 1.0 / (111_320.0 * lat.to_radians().cos());
    let points = (0..25)
        .map(|i| {
            let t = t0 + i as f64 * 5.0;
            enu_to_track_point(t, lat, lon + (t - t0) * 60.0 * deg_per_m, 600.0, [60.0, 0.0, 0.0])
        })
        .collect();
    Track { id: id.into(), icao24: icao, type_code: None, engine: EngineClass::Unknown, points }
}

#[test]
fn drops_only_tisb_tracks_riding_an_adsb_track() {
    let adsb = track("adsb", 0xa00001, 40.0, -83.0, 0.0);
    let shadow = track("shadow", NON_ICAO_BIT | 1, 40.003, -83.0, 2.0);
    let far = track("far", NON_ICAO_BIT | 2, 40.3, -83.0, 0.0);
    let later = track("later", NON_ICAO_BIT | 3, 40.0, -83.0, 3600.0);
    let (kept, dropped) = drop_shadow_tracks(vec![adsb, shadow, far, later], &DedupParams::default());
    assert_eq!(dropped, 1);
    assert_eq!(kept.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["adsb", "far", "later"]);
}
