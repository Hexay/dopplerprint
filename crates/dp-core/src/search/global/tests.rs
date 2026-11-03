use super::*;
use crate::dem::geoid_undulation;
use crate::geo::{Geodetic, Vec3, east_north, geodetic_to_ecef, up_vector};
use crate::physics::cv::StraightLine;

const LAT: f64 = 40.0;
const LON: f64 = -100.0;
const T0: f64 = 1.79e9;
const C: f64 = 343.0;

/// ENU basis at the test observer, plus the observer (ground at 300 m MSL).
fn frame() -> (Vec3, Vec3, Vec3, Vec3) {
    let obs = geodetic_to_ecef(Geodetic { lat_deg: LAT, lon_deg: LON, h_ell_m: geoid_undulation(LAT, LON) + 300.0 });
    let (e, n) = east_north(LAT, LON);
    (obs, e, n, up_vector(LAT, LON))
}

/// Straight pass with CPA emission at T0 + t_cpa, and the event an observer at `obs` would record.
fn pass(id: &str, obs: Vec3, dir: Vec3, offset: Vec3, speed: f64, t_cpa: f64) -> (Track, EventSpec) {
    let line = StraightLine { obs, dir, offset, speed, t_cpa_emit: T0 + t_cpa, half_span_s: 400.0, dt: 1.0 };
    let d = offset.norm();
    let t_arr = t_cpa + d / C;
    let ev = EventSpec {
        window: (t_arr - 60.0, t_arr + 60.0),
        t_cpa_audio: t_arr,
        d_range: (d * 0.7, d * 1.4),
        v_range: (speed * 0.8, speed * 1.25),
        f0_range: (90.0, 110.0),
        t_cpa_halfwidth_s: 0.0,
        kappa_range: None,
    };
    (line.track(id), ev)
}

fn contains(p: &RegionProposal) -> bool {
    let a = geodetic_to_ecef(Geodetic { lat_deg: LAT, lon_deg: LON, h_ell_m: 0.0 });
    let b = geodetic_to_ecef(Geodetic { lat_deg: p.region.lat_deg, lon_deg: p.region.lon_deg, h_ell_m: 0.0 });
    (a - b).norm() <= p.region.radius_m
}

fn scene() -> (Vec<Track>, Vec<EventSpec>) {
    let (obs, e, n, u) = frame();
    let (t1, e1) = pass("a-0", obs, e, n * 1200.0 + u * 900.0, 60.0, 300.0);
    let (t2, e2) = pass("b-0", obs, n, e * -800.0 + u * 1100.0, 70.0, 700.0);
    // A distractor that fits event 1 alone, 30 km east; nothing there fits event 2.
    let (t3, _) = pass("c-0", obs + e * 30_000.0, e, n * 1200.0 + u * 900.0, 60.0, 300.0);
    (vec![t1, t2, t3], vec![e1, e2])
}

#[test]
fn two_events_intersect_at_truth() {
    let (tracks, events) = scene();
    let p = GlobalParams { ground_msl_m: (250.0, 350.0), ..Default::default() };
    let (props, stats) = propose_regions(&events, &tracks, (T0 - 60.0, T0 + 60.0), &p);
    assert_eq!(stats.n_max, 2);
    let top = &props[0];
    assert_eq!(top.n_events, 2);
    assert!(contains(top), "{top:?}");
    assert!(top.clock_range.0 <= T0 + 1.0 && top.clock_range.1 >= T0 - 1.0, "{:?}", top.clock_range);
    assert_eq!(top.flights, vec![vec!["a-0".to_string()], vec!["b-0".to_string()]]);
    assert!(props.iter().all(|q| q.n_events == 2), "single-event patches are dropped once two agree");
}

#[test]
fn single_event_reports_mirror_and_distractor() {
    let (tracks, events) = scene();
    let p = GlobalParams { ground_msl_m: (250.0, 350.0), ..Default::default() };
    let (props, stats) = propose_regions(&events[..1], &tracks, (T0 - 60.0, T0 + 60.0), &p);
    assert_eq!(stats.n_max, 1);
    assert!(props.iter().any(contains));
    let flights: std::collections::HashSet<&str> = props.iter().flat_map(|q| q.flights[0].iter().map(String::as_str)).collect();
    assert!(flights.contains("a-0") && flights.contains("c-0"), "{flights:?}");
    assert!(props.len() >= 4, "mirror × two flights: {}", props.len());
}

#[test]
fn wide_prior_coarsens_lattice() {
    let (_, events) = scene();
    let p = GlobalParams::default();
    let narrow = vote_cell_m(&events, (T0 - 5.0, T0 + 5.0), &p);
    let wide = vote_cell_m(&events, (T0 - 600.0, T0 + 600.0), &p);
    assert_eq!(narrow, p.cell_m);
    assert!(wide > narrow);
}
