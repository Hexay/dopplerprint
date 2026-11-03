use std::path::PathBuf;

use dp_core::adsb::AltSource;
use dp_core::adsb::clean::{CleanParams, clean_trace};
use dp_core::adsb::trace::{Alt, RawPoint, RawTrace, parse_trace};
use dp_core::track::EngineClass;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/adsb").join(rel)
}

fn real(rel: &str) -> RawTrace {
    parse_trace(&std::fs::read(fixture(rel)).unwrap()).unwrap()
}

/// Straight eastbound flight at 60 m/s, 1000 ft geometric, one point every 5 s.
fn synthetic(n: usize) -> Vec<RawPoint> {
    (0..n)
        .map(|i| RawPoint {
            dt: i as f64 * 5.0,
            lat: 40.0,
            lon: -83.0 + i as f64 * 5.0 * 60.0 / (111_320.0 * 40f64.to_radians().cos()),
            alt: Alt::Feet(1000.0),
            gs_kt: Some(60.0 / 0.514_444),
            track_deg: Some(90.0),
            flags: 0,
            baro_rate_fpm: Some(0.0),
            category: None,
            src: 0,
            alt_geom_ft: Some(1100.0),
            geom_rate_fpm: None,
        })
        .collect()
}

fn clean(points: Vec<RawPoint>) -> (Vec<dp_core::adsb::model::CleanTrack>, dp_core::adsb::clean::CleanStats) {
    let raw = RawTrace { icao: "a00001".into(), r: None, t: None, timestamp: 1_788_825_600.0, trace: points };
    clean_trace(&raw, 0xa00001, None, EngineClass::Unknown, &CleanParams::default())
}

#[test]
fn parses_real_trace() {
    let t = real("traces/42/trace_full_a03a42.json");
    assert_eq!(t.icao, "a03a42");
    assert_eq!(t.t.as_deref(), Some("C172"));
    assert_eq!(t.timestamp, 1_788_825_600.0);
    assert_eq!(t.trace.len(), 389);
    assert_eq!(t.category(), Some("A1"));
}

#[test]
fn rejects_position_and_altitude_jumps() {
    let mut pts = synthetic(20);
    pts[7].lat += 0.5;
    pts[12].alt_geom_ft = Some(9000.0);
    let (tracks, st) = clean(pts);
    assert_eq!(st.jumps, 2);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].points.len(), 18);
}

#[test]
fn bad_anchor_is_abandoned_after_consecutive_rejects() {
    let mut pts = synthetic(10);
    pts[0].lat += 1.0;
    let (tracks, st) = clean(pts);
    assert_eq!(st.jumps, 3);
    assert_eq!(st.short_tracks, 1);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].points.len(), 9);
}

#[test]
fn splits_on_gaps_and_ground() {
    let mut pts = synthetic(30);
    for p in &mut pts[10..] {
        p.dt += 65.0;
    }
    pts[20].alt = Alt::Ground;
    let (tracks, st) = clean(pts);
    assert_eq!(st.ground, 1);
    assert_eq!(tracks.iter().map(|t| t.points.len()).collect::<Vec<_>>(), vec![10, 10, 9]);
    assert_eq!(tracks.iter().map(|t| t.track_no).collect::<Vec<_>>(), vec![0, 1, 2]);
}

#[test]
fn altitude_source_on_real_traces() {
    let count = |rel: &str, icao: u32| {
        let raw = real(rel);
        let (tracks, _) = clean_trace(&raw, icao, None, EngineClass::Unknown, &CleanParams::default());
        let mut c = [0usize; 3];
        tracks.iter().flat_map(|t| &t.points).for_each(|p| c[p.alt_source as usize] += 1);
        c
    };
    let c172 = count("traces/42/trace_full_a03a42.json", 0xa03a42);
    assert!(c172[AltSource::Geometric as usize] > 300 && c172[AltSource::Baro as usize] == 0, "{c172:?}");
    let k35r = count("traces/42/trace_full_ae0242.json", 0xae0242);
    assert!(k35r[AltSource::Baro as usize] > 1000 && k35r[AltSource::Geometric as usize] == 0, "{k35r:?}");
}

#[test]
fn ecef_velocity_matches_ground_speed_and_motion() {
    let raw = real("traces/42/trace_full_a03a42.json");
    let (tracks, _) = clean_trace(&raw, 0xa03a42, None, EngineClass::Piston, &CleanParams::default());
    let mut errs = Vec::new();
    for tr in &tracks {
        let tps: Vec<_> = tr.points.iter().map(|p| p.to_track_point()).collect();
        for (cp, tp) in tr.points.iter().zip(&tps) {
            let enu = (cp.ve.hypot(cp.vn)).hypot(cp.vu);
            assert!((tp.vel.norm() - enu).abs() < 1e-3);
        }
        for w in tps.windows(2) {
            let fd = (w[1].pos - w[0].pos) * (1.0 / (w[1].t - w[0].t));
            errs.push((fd - (w[0].vel + w[1].vel) * 0.5).norm());
        }
    }
    errs.sort_by(f64::total_cmp);
    let median = errs[errs.len() / 2];
    assert!(median < 3.0, "median |Δpos/Δt − v| = {median} m/s");
    let gs_kt = raw.trace.iter().find_map(|p| p.gs_kt.filter(|_| !matches!(p.alt, Alt::Ground))).unwrap();
    let first = tracks[0].points[0];
    assert!((first.ve.hypot(first.vn) - gs_kt * 0.514_444).abs() < 1e-9);
}
