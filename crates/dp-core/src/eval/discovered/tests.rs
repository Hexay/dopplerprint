use super::DiscoveryParams;
use super::front::add_front_stages;
use super::truth::{Found, Labelled, Pass, Source, Truthed, label, match_labelled};
use crate::eval::CaseResult;
use crate::eval::metrics::Stage;

fn found(t_cpa: f64, fund_hz: f64, harmonics: &[u32], d_m: f64) -> Found {
    Found {
        t_cpa,
        fund_hz,
        harmonics: harmonics.to_vec(),
        d_m,
        v_mps: 60.0,
        d_range: (0.5 * d_m, 2.0 * d_m),
        v_range: (55.0, 90.0),
        snr: 10.0,
    }
}

fn truth(t_cpa: f64, f0: f64, flight: &str) -> Labelled {
    Labelled { t_cpa, f0, flight: Some(flight.into()) }
}

fn pass(flight: &str, t_cpa: f64, slant_m: f64) -> Pass {
    Pass { flight: flight.into(), t_cpa, slant_m, speed_mps: 50.0 }
}

#[test]
fn labelled_match_is_one_to_one_closest_time_first_on_any_harmonic() {
    let p = DiscoveryParams::default();
    // Truth tone 240 Hz is the 3rd harmonic of an 80 Hz fundamental.
    let f = [found(103.0, 80.0, &[1, 3], 500.0), found(100.5, 81.0, &[3], 500.0), found(100.0, 50.0, &[1, 2], 500.0)];
    let m = match_labelled(&f, &[truth(100.0, 240.0, "a")], &p);
    assert_eq!(m[0], None, "taken by the closer event");
    let (j, dt, df) = m[1].unwrap();
    assert_eq!(j, 0);
    assert!((dt - 0.5).abs() < 1e-12 && (df - 0.0125).abs() < 1e-12);
    assert_eq!(m[2], None, "50/100 Hz is outside ±6 % of 240 Hz");
}

#[test]
fn unlabelled_events_are_attributed_to_coinciding_adsb_passes_else_spurious() {
    let p = DiscoveryParams::default();
    let f = [
        found(50.0, 100.0, &[1], 1_000.0),
        found(200.0, 90.0, &[1], 800.0),
        found(300.0, 90.0, &[1], 800.0),
        found(400.0, 90.0, &[1], 800.0),
    ];
    let passes = [
        pass("2026-09-08/abc123-1", 49.0, 1_100.0),
        pass("x-other", 205.0, 900.0),
        pass("x-far", 301.0, 5_000.0),
        pass("x-late", 420.0, 800.0),
    ];
    let l = label(&f, &[truth(50.5, 100.0, "abc123")], &passes, &p);
    assert_eq!(l[0].source, Some(Source::Labelled));
    assert_eq!(l[0].flight.as_deref(), Some("abc123"));
    assert!((l[0].d_err.unwrap() - (1_000.0 / 1_100.0 - 1.0)).abs() < 1e-12, "kinematics from the labelled flight's pass");
    assert_eq!((l[0].d_gate, l[0].v_gate), (Some(true), Some(false)), "ground speed 50 is below the fit's v range");
    assert_eq!((l[1].source, l[1].flight.as_deref()), (Some(Source::Adsb), Some("x-other")));
    assert!((l[1].dt_cpa_s.unwrap() + 5.0).abs() < 1e-12);
    assert!(!l[2].is_real(), "slant outside the fit's d range");
    assert!(!l[3].is_real(), "CPA 20 s away");
}

#[test]
fn an_off_tone_event_at_the_labelled_flights_cpa_recalls_it_by_flight() {
    let p = DiscoveryParams::default();
    let f = [found(52.0, 70.0, &[1], 3_000.0)];
    let l = label(&f, &[truth(50.0, 100.0, "abc123")], &[pass("2026-09-08/abc123-1", 51.0, 2_000.0)], &p);
    assert_eq!((l[0].source, l[0].truth_index), (Some(Source::Labelled), Some(0)));
    assert!((l[0].dt_cpa_s.unwrap() - 2.0).abs() < 1e-12 && (l[0].f0_err.unwrap() + 0.3).abs() < 1e-12);
    let none = label(&f, &[truth(50.0, 100.0, "other")], &[pass("2026-09-08/abc123-1", 51.0, 2_000.0)], &p);
    assert_eq!(none[0].source, Some(Source::Adsb));
}

#[test]
fn front_stages_precede_search_stages_and_set_lost_at() {
    let real = Truthed { source: Some(Source::Adsb), flight: Some("f".into()), ..Default::default() };
    let mut r = CaseResult::default();
    r.metrics.stages = vec![Stage { stage: "catalog".into(), ok: true, rank: None, of: None }];
    add_front_stages(&mut r, &[real.clone(), Truthed::default()], &[0, 1]);
    let names: Vec<&str> = r.metrics.stages.iter().map(|s| s.stage.as_str()).collect();
    assert_eq!(names, ["discover", "select", "catalog"]);
    assert_eq!(r.metrics.lost_at.as_deref(), Some("select"), "a spurious event was searched");

    let mut r = CaseResult::default();
    add_front_stages(&mut r, &[real], &[0]);
    assert_eq!(r.metrics.lost_at, None);
}
