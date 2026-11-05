use super::tests::ridge_from_model;
use super::*;

/// Event fitted (production fit) on a CV ridge spanning [t0, t1] with 0.3 % frame noise, as on real ridges.
fn event(truth: (f64, f64, f64, f64), t0: f64, t1: f64, snr: f64) -> Event {
    let ridge = ridge_from_model(truth, t0, t1, 0.003 * truth.0, 3);
    let fit = fit_cv(&ridge, 343.0);
    let mut e = Event { id: 0, ridge, harmonics: vec![1], fit, ridge_harmonic: 1, snr, pass_p: 0.0 };
    e.pass_p = pass_likeness(&pass_features(&e));
    e
}

#[test]
fn full_aircraft_pass_is_pass_like() {
    let e = event((110.0, 70.0, 900.0, 60.0), 20.0, 100.0, 20.0);
    let x = pass_features(&e);
    assert!(x.sides > 0.4 && x.swing > 0.3 && x.implausible == 0.0, "{x:?}");
    assert!(e.pass_p > 0.9, "p = {}", e.pass_p);
}

#[test]
fn flat_line_and_road_traffic_are_not() {
    let flat = event((300.0, 8.0, 400.0, 60.0), 40.0, 80.0, 20.0);
    assert!(flat.pass_p < MIN_PASS_P, "flat p = {} {:?}", flat.pass_p, pass_features(&flat));
    let car = event((110.0, 21.0, 34.0, 60.0), 40.0, 80.0, 30.0);
    assert!(pass_features(&car).implausible > 1.0);
    assert!(car.pass_p < MIN_PASS_P, "car p = {}", car.pass_p);
}

#[test]
fn select_gates_then_ranks_by_snr_in_cpa_order() {
    let mut ev = vec![
        event((110.0, 70.0, 900.0, 200.0), 160.0, 240.0, 10.0),
        event((110.0, 21.0, 34.0, 60.0), 40.0, 80.0, 50.0),
        event((95.0, 60.0, 1200.0, 100.0), 60.0, 140.0, 30.0),
    ];
    assert_eq!(select(&ev, 4), vec![2, 0], "the strong road-traffic event is not searched");
    assert_eq!(select(&ev, 1), vec![2]);
    for e in &mut ev {
        e.pass_p = 0.1;
    }
    ev[1].pass_p = 0.2;
    assert_eq!(select(&ev, 4), vec![1], "nothing qualifies: the most pass-like event alone");
}

#[test]
fn select_keeps_one_event_per_pass() {
    let ev = vec![
        event((110.0, 70.0, 900.0, 100.0), 60.0, 140.0, 20.0),
        event((220.0, 70.0, 900.0, 103.0), 60.0, 140.0, 30.0),
        event((95.0, 60.0, 1200.0, 200.0), 160.0, 240.0, 10.0),
    ];
    assert_eq!(select(&ev, 4), vec![1, 2], "a second tone of the same pass is not a second pass");
}

#[test]
fn select_skips_events_that_place_nothing() {
    let mut ev = vec![event((110.0, 70.0, 900.0, 100.0), 60.0, 140.0, 20.0), event((95.0, 70.0, 900.0, 200.0), 160.0, 240.0, 30.0)];
    ev[1].pass_p = 1.0;
    ev[1].fit.d_range = (1.0, 13_159.0);
    assert_eq!(select(&ev, 4), vec![0], "a d range over three decades places nothing");
}
