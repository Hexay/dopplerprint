use dp_core::audio::{Audio, SpecParams};
use dp_core::geo::{Geodetic, east_north, geodetic_to_ecef, up_vector};
use dp_core::physics::{Atmosphere, cv::StraightLine, doppler_factors};
use dp_core::search::power_llr::{LlrParams, overlap_factor};
use dp_core::search::score::{EventTarget, ScoreMode, ScoreParams, ScoringPlane, score_joint};
use dp_core::sim::noise::Pcg;

const SR: u32 = 2000;
const T_REC: f64 = 1.7e9 + 500.0;

fn llr_params() -> ScoreParams {
    ScoreParams { mode: ScoreMode::PowerLlr, ..Default::default() }
}

fn noise(seconds: f64, seed: u64) -> Vec<f32> {
    let mut rng = Pcg::new(seed);
    (0..(seconds * SR as f64) as usize).map(|_| rng.gauss() as f32).collect()
}

#[test]
fn noise_only_plane_is_mean_one_and_overlap_factor_near_two() {
    let audio = Audio { sample_rate: SR, samples: noise(300.0, 7) };
    let plane = ScoringPlane::build(&audio, &SpecParams::default(), &llr_params());
    let s = &plane.spec;
    let interior = &s.data[40 * s.n_f..(s.n_t - 40) * s.n_f];
    let mean = interior.iter().map(|&x| x as f64).sum::<f64>() / interior.len() as f64;
    let (inflation, var_x) = overlap_factor(s, 40);
    eprintln!("noise-only: mean x {mean:.3}, Var x {var_x:.3}, overlap factor {inflation:.2}");
    assert!((mean - 1.0).abs() < 0.06, "mean {mean}");
    assert!((1.4..2.6).contains(&inflation), "overlap factor {inflation}");
}

#[test]
fn faint_tone_true_observer_wins_with_sub_frame_clock() {
    let (lat, lon) = (40.0, -96.0);
    let obs = geodetic_to_ecef(Geodetic { lat_deg: lat, lon_deg: lon, h_ell_m: 300.0 });
    let (e, n) = east_north(lat, lon);
    let line = StraightLine {
        obs,
        dir: n,
        offset: up_vector(lat, lon) * 500.0 + e * 300.0,
        speed: 70.0,
        t_cpa_emit: 1.7e9 + 600.0,
        half_span_s: 400.0,
        dt: 1.0,
    };
    let track = line.track("ac1");
    // Audio t = 0 is UNIX T_REC + 0.1: the true clock sits between frames.
    let (true_clock, f0, dur) = (T_REC + 0.1, 88.0, 240.0);
    let grid: Vec<f64> = (0..=(dur * 100.0) as usize).map(|i| true_clock + i as f64 * 0.01).collect();
    let fac = doppler_factors(&track, obs, &grid, &Atmosphere::default());
    let mut samples = noise(dur, 11);
    let mut phase = 0.0f64;
    for (i, x) in samples.iter_mut().enumerate() {
        let f = fac[(i as f64 / SR as f64 * 100.0) as usize];
        if f.is_finite() {
            phase += std::f64::consts::TAU * f0 * f / SR as f64;
            *x += (0.08 * phase.sin() + 0.05 * (2.0 * phase).sin()) as f32;
        }
    }
    let audio = Audio { sample_rate: SR, samples };
    let p = llr_params();
    let plane = ScoringPlane::build(&audio, &SpecParams::default(), &p);
    // Far from CPA the tone sits in one bin for > 45 s and the running q25 floor absorbs it (by design).
    let target = [EventTarget { track: &track, window: (70.0, 130.0), f0_range: (80.0, 96.0) }];
    let atm = Atmosphere::default();
    let truth = score_joint(&plane, &target, obs, &atm, T_REC, &p).unwrap();
    let far = score_joint(&plane, &target, obs + e * 1500.0, &atm, T_REC, &p).unwrap();
    let fixed = ScoreParams { llr: LlrParams { segment_s: 0.0, ..p.llr.clone() }, ..p.clone() };
    let fx = score_joint(&plane, &target, obs, &atm, T_REC, &fixed).unwrap();
    eprintln!(
        "truth {:.1} nats (clock {:+.3} s), 1.5 km off {:.1}; fixed-prior truth {:.1} (clock {:+.3})",
        truth.total,
        truth.clock_offset - true_clock,
        far.total,
        fx.total,
        fx.clock_offset - true_clock
    );
    assert!(truth.total > far.total + 10.0);
    assert!((truth.events[0].f0 - f0).abs() < 1.0, "f0 {}", truth.events[0].f0);
    assert!((truth.clock_offset - true_clock).abs() < 0.07, "clock {}", truth.clock_offset - true_clock);
}
