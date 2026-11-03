use dp_core::audio::LogSpectrogram;
use dp_core::geo::{Geodetic, east_north, geodetic_to_ecef, up_vector};
use dp_core::physics::{Atmosphere, cv::StraightLine, doppler_factors};
use dp_core::search::score::{EventTarget, ScoreParams, ScoringPlane, score_joint};

const T_REC: f64 = 1.7e9 + 500.0;

fn scene() -> (dp_core::geo::Vec3, dp_core::track::Track, dp_core::geo::Vec3, dp_core::geo::Vec3) {
    let (lat, lon) = (40.0, -96.0);
    let obs = geodetic_to_ecef(Geodetic { lat_deg: lat, lon_deg: lon, h_ell_m: 300.0 });
    let (e, n) = east_north(lat, lon);
    let u = up_vector(lat, lon);
    let line = StraightLine {
        obs,
        dir: n,
        offset: u * 1500.0 + e * 500.0,
        speed: 60.0,
        t_cpa_emit: 1.7e9 + 600.0,
        half_span_s: 400.0,
        dt: 1.0,
    };
    (obs, line.track("ac1"), e, n)
}

fn painted(track: &dp_core::track::Track, obs: dp_core::geo::Vec3, f0: f64) -> LogSpectrogram {
    let (dt, bpo, n_t) = (0.25, 96u32, 1200usize);
    let n_f = (96.0 * (1000.0f64 / 20.0).log2()) as usize;
    let mut s = LogSpectrogram { t0: 0.0, dt, f_min: 20.0, bins_per_octave: bpo, n_t, n_f, data: vec![0.0; n_t * n_f] };
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for v in s.data.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *v = ((x >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 2.0;
    }
    let times: Vec<f64> = (0..n_t).map(|i| T_REC + i as f64 * dt).collect();
    let fac = doppler_factors(track, obs, &times, &Atmosphere::default());
    for (i, f) in fac.iter().enumerate().filter(|(_, f)| f.is_finite()) {
        for h in 1..=4 {
            let b = s.bin_of(f0 * h as f64 * f).round() as usize;
            if b < n_f {
                s.data[i * n_f + b] = 6.0;
            }
        }
    }
    s
}

#[test]
fn true_observer_wins_and_recovers_f0_and_clock() {
    let (obs, track, e, _) = scene();
    let plane = ScoringPlane::new(&painted(&track, obs, 88.0), &ScoreParams::default());
    let target = [EventTarget { track: &track, window: (40.0, 160.0), f0_range: (60.0, 120.0) }];
    let p = ScoreParams::default();
    let atm = Atmosphere::default();
    let nominal = T_REC + 3.0;

    let truth = score_joint(&plane, &target, obs, &atm, nominal, &p).unwrap();
    assert!((truth.clock_offset - T_REC).abs() <= 0.25 + 1e-6, "clock {}", truth.clock_offset - T_REC);
    assert!((truth.events[0].f0 - 88.0).abs() < 88.0 * 0.01, "f0 {}", truth.events[0].f0);
    assert!(truth.events[0].coverage > 0.99);

    // Scores are z-normalised (noise σ ≈ 0.58 here), so judge margins in σ, not ratios: the flat
    // asymptotes only constrain speed and dilute any ratio.
    let sigma = 1.0 / 3f64.sqrt();
    let near = score_joint(&plane, &target, obs + e * 300.0, &atm, nominal, &p).unwrap();
    assert!(truth.total - near.total > 5.0 * sigma, "truth {} vs 300 m off {}", truth.total, near.total);
    let far = score_joint(&plane, &target, obs + e * 1500.0, &atm, nominal, &p).unwrap();
    assert!(truth.total - far.total > 30.0 * sigma, "truth {} vs 1500 m off {}", truth.total, far.total);
}
