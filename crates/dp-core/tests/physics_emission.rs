use dp_core::geo::{Geodetic, Vec3, east_north, geodetic_to_ecef, up_vector};
use dp_core::physics::cv::{self, CvParams, StraightLine};
use dp_core::physics::{Atmosphere, cpa, doppler_factors, emission, emission::emission_iters};
use dp_core::track::{EngineClass, Track, TrackPoint};

const T0: f64 = 1.725e9;
const LAT: f64 = 40.87;
const LON: f64 = -96.7;

fn observer() -> Vec3 {
    geodetic_to_ecef(Geodetic { lat_deg: LAT, lon_deg: LON, h_ell_m: 400.0 })
}

fn straight(v: f64, d: f64) -> (Track, StraightLine) {
    let (east, _) = east_north(LAT, LON);
    let line = StraightLine {
        obs: observer(),
        dir: east,
        offset: up_vector(LAT, LON) * d,
        speed: v,
        t_cpa_emit: T0 + 600.0,
        half_span_s: 600.0,
        dt: 1.0,
    };
    (line.track("cv"), line)
}

/// A level 3°/s turn, sampled every 5 s with exact velocities.
fn turning_track() -> Track {
    let (e, n) = east_north(LAT, LON);
    let up = up_vector(LAT, LON);
    let (r, w) = (1500.0, 3f64.to_radians());
    let points = (0..=60)
        .map(|i| {
            let s = i as f64 * 5.0;
            let a = w * s;
            let pos = observer() + e * (r * a.cos()) + n * (r * a.sin() - 800.0) + up * 900.0;
            let vel = e * (-r * w * a.sin()) + n * (r * w * a.cos());
            TrackPoint { t: T0 + s, pos, vel }
        })
        .collect();
    Track { id: "turn".into(), icao24: 0, type_code: None, engine: EngineClass::Unknown, points }
}

#[test]
fn cv_closed_form_matches_numeric_solve() {
    for &(v, d) in &[(80.0, 1000.0), (120.0, 400.0), (55.0, 3000.0)] {
        let (tr, line) = straight(v, d);
        let atm = Atmosphere::default();
        let p = CvParams { f0: 100.0, v, d_cpa: d, t_cpa_arrive: line.t_cpa_emit + d / atm.c, c: atm.c };
        let t: Vec<f64> = (-400..=400).map(|i| p.t_cpa_arrive + i as f64 * 0.5).collect();
        let k = doppler_factors(&tr, observer(), &t, &atm);
        let worst = t
            .iter()
            .zip(&k)
            .map(|(&ti, &ki)| ((100.0 * ki - cv::frequency(&p, ti)) / cv::frequency(&p, ti)).abs())
            .fold(0.0, f64::max);
        assert!(worst < 1e-6, "v={v} d={d}: worst relative error {worst:e}");
        assert!((cv::frequency(&p, p.t_cpa_arrive) - 100.0).abs() < 1e-9);
    }
}

#[test]
fn newton_warm_start_converges_in_three() {
    let atm = Atmosphere { c: 340.0, wind: Vec3::new(3.0, -6.0, 1.0) };
    for tr in [straight(110.0, 300.0).0, turning_track()] {
        let obs = observer();
        let mut prev: Option<(f64, f64, f64)> = None;
        let mut solved = 0;
        for i in 0..3000 {
            let t = T0 + 5.0 + i as f64 * 0.1;
            let guess = prev.map_or(t - 10.0, |(tp, tau, k)| tau + (t - tp) * k);
            let Some((e, it)) = emission_iters(&tr, obs, t, &atm, guess) else { continue };
            if prev.is_some() {
                assert!(it <= 3, "{}: {it} iterations at t={t}", tr.id);
            }
            prev = Some((t, e.tau, e.factor));
            solved += 1;
        }
        assert!(solved > 1000);
    }
}

#[test]
fn zero_wind_equals_still_air_and_factor_is_dtau_dt() {
    let tr = turning_track();
    let obs = observer();
    let still = Atmosphere::default();
    let calm = Atmosphere { c: 343.0, wind: Vec3::new(1e-12, 0.0, 0.0) };
    let windy = Atmosphere { c: 343.0, wind: Vec3::new(8.0, -5.0, 0.0) };
    for i in 0..50 {
        let t = T0 + 20.0 + i as f64 * 5.0;
        let a = emission(&tr, obs, t, &still).unwrap();
        let b = emission(&tr, obs, t, &calm).unwrap();
        assert!((a.tau - b.tau).abs() < 1e-6 && (a.factor - b.factor).abs() < 1e-12);
        // f_rx/f0 = dτ/dt: validates the moving-medium formula in docs/PHYSICS.md against the solver.
        let h = 1e-3;
        let (lo, hi) = (emission(&tr, obs, t - h, &windy).unwrap(), emission(&tr, obs, t + h, &windy).unwrap());
        let num = (hi.tau - lo.tau) / (2.0 * h);
        let e = emission(&tr, obs, t, &windy).unwrap();
        assert!((num - e.factor).abs() < 2e-4, "t={t}: numeric {num} vs {}", e.factor);
    }
}

#[test]
fn along_track_wind_asymptotes() {
    let (tr, line) = straight(80.0, 500.0);
    let atm = Atmosphere { c: 340.0, wind: line.dir * 10.0 };
    // Emissions ~500 s either side of CPA (40 km away), so k̂ is within 0.7° of the track.
    let t = [line.t_cpa_emit - 380.0, line.t_cpa_emit + 620.0];
    let k = doppler_factors(&tr, observer(), &t, &atm);
    assert!((k[0] - 350.0 / 270.0).abs() < 2e-3, "approach {}", k[0]);
    assert!((k[1] - 330.0 / 410.0).abs() < 2e-3, "recede {}", k[1]);
}

#[test]
fn cpa_on_straight_track() {
    let (tr, line) = straight(80.0, 1200.0);
    let atm = Atmosphere::default();
    let c = cpa(&tr, observer(), &atm).unwrap();
    assert!((c.t_emit - line.t_cpa_emit).abs() < 1e-6);
    assert!((c.slant_m - 1200.0).abs() < 1e-6);
    assert!((c.t_arrive - (line.t_cpa_emit + 1200.0 / 343.0)).abs() < 1e-5);
    assert!((c.ground_speed - 80.0).abs() < 1e-3);
    let windy = Atmosphere { c: 343.0, wind: up_vector(LAT, LON) * -5.0 };
    let cw = cpa(&tr, observer(), &windy).unwrap();
    // A downdraft carries the sound from the overhead aircraft down to the observer faster.
    assert!((cw.t_arrive - (line.t_cpa_emit + 1200.0 / 348.0)).abs() < 1e-5);

    let off_knot = StraightLine { half_span_s: 600.37, ..line }.track("off");
    let co = cpa(&off_knot, observer(), &atm).unwrap();
    assert!((co.t_emit - line.t_cpa_emit).abs() < 1e-6, "interior CPA off by {:e}", co.t_emit - line.t_cpa_emit);
}

#[test]
fn outside_track_is_none() {
    let (tr, line) = straight(80.0, 1000.0);
    let atm = Atmosphere::default();
    assert!(emission(&tr, observer(), line.t_cpa_emit - 700.0, &atm).is_none());
    assert!(emission(&tr, observer(), line.t_cpa_emit + 1000.0, &atm).is_none());
    let k = doppler_factors(&tr, observer(), &[T0 - 50.0, line.t_cpa_emit, T0 + 5000.0], &atm);
    assert!(k[0].is_nan() && k[2].is_nan() && (k[1] - 1.0).abs() < 0.3);
}
