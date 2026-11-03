use super::thermo::wind_en;
use super::*;
use serde_json::Value;

/// Open-Meteo best_match (HRRR), 37.70,-121.85 on 2026-05-09, hours 21–23 UTC (CC BY 4.0).
const FIXTURE: &str = include_str!("fixture_livermore.json");
const T22: f64 = 1_778_364_000.0;

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}

fn site(p: &Profile) -> Site {
    Site { lat: 37.70, lon: -121.85, ground_ell: p.ground_ell(37.70, -121.85) }
}

#[test]
fn profile_interpolates_hours_and_drops_underground_levels() {
    let p = openmeteo::profile_at(&fixture(), T22 + 1800.0).unwrap();
    let sfc = p.levels.iter().find(|l| (l.z_msl - 118.0).abs() < 1e-9).unwrap();
    let (a, b) = (wind_en(5.47, 286.0), wind_en(5.73, 284.0));
    assert!((sfc.wind_en[0] - 0.5 * (a[0] + b[0])).abs() < 1e-9);
    assert!((sfc.t_k - (25.1 + 273.15)).abs() < 1e-9);
    assert!(p.levels.iter().all(|l| l.z_msl > p.ground_msl + 9.0), "1000 hPa (114 m) is below the 10 m node");
    assert_eq!(p.levels.len(), 1 + 3 + 11);
    assert!(openmeteo::profile_at(&fixture(), T22 + 5400.0).is_some(), "23:30 holds 23:00");
    assert!(openmeteo::profile_at(&fixture(), T22 + 2.0 * 3600.0 + 1.0).is_none());
    assert!(openmeteo::profile_at(&fixture(), T22 - 3601.0).is_none());
}

#[test]
fn atmosphere_layers_and_report() {
    let p = openmeteo::profile_at(&fixture(), T22).unwrap();
    let s = site(&p);
    let atm = p.atmosphere(&s).unwrap();
    assert!((340.0..350.0).contains(&atm.c), "warm afternoon: {}", atm.c);
    let low = layer(&atm, s.ground_ell, s.ground_ell + 300.0);
    let high = layer(&atm, s.ground_ell, s.ground_ell + 3000.0);
    assert!(high.c < low.c, "lapse rate: {} vs {}", high.c, low.c);
    let r = layer(&atm, s.ground_ell, s.ground_ell + REF_DEPTH_M);
    assert!((r.c - atm.c).abs() < 1e-9 && (r.wind - atm.wind).norm() < 1e-9);
    let text = describe(&atm, &s, s.ground_ell + 904.0);
    assert!(text.starts_with("wind ") && text.contains("averaged 0–900 m; c 3"), "{text}");
}

#[test]
fn humidity_is_filled_between_levels() {
    let p = Profile {
        levels: vec![
            Level { z_msl: 0.0, t_k: 300.0, q: Some(0.02), wind_en: [0.0; 2] },
            Level { z_msl: 100.0, t_k: 300.0, q: None, wind_en: [0.0; 2] },
            Level { z_msl: 200.0, t_k: 300.0, q: Some(0.0), wind_en: [0.0; 2] },
        ],
        ground_msl: 0.0,
        valid_unix: 0.0,
        source: String::new(),
    };
    let n = p.nodes();
    assert!((n[1].1 - thermo::sound_speed(300.0, 0.01)).abs() < 1e-9);
}

#[test]
fn cache_is_read_and_offline_never_fetches() {
    let dir = tempfile::tempdir().unwrap();
    let date = "2026-05-09";
    assert!(openmeteo::day(dir.path(), 37.70, -121.85, date, true, T22).unwrap().is_none());
    std::fs::write(openmeteo::cache_file(dir.path(), 37.70, -121.85, date), FIXTURE).unwrap();
    let v = openmeteo::day(dir.path(), 37.7012, -121.8549, date, true, T22).unwrap().unwrap();
    assert_eq!(v, fixture());
    assert!(archive(dir.path(), 37.70, -121.85, T22 + 60.0, true).unwrap().is_some());
}
