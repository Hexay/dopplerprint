//! Cross-checks between the independent synthesiser (sim) and the analytic physics.

use dp_core::geo::Vec3;
use dp_core::physics::cv::{self, CvParams};
use dp_core::physics::{Atmosphere, cpa};
use dp_core::sim;
use realfft::RealFftPlanner;

const C: f64 = 343.0;
const F0: f64 = 100.0;
const SPEED: f64 = 80.0;
const ALT: f64 = 1000.0;
const MIC: f64 = 2.0;
const CPA_T: f64 = 60.0;

fn scenario(wind: [f64; 3], ground: &str, harmonics: &str) -> String {
    format!(
        r#"
seed = 11
sample_rate = 2000
duration_s = 120.0
start_unix = 1725827400.0
[observer]
lat_deg = 40.0
lon_deg = -100.0
h_ell_m = 500.0
mic_height_m = {MIC}
[atmosphere]
c = {C}
wind_enu = [{}, {}, {}]
{ground}
[noise]
snr_db = 45.0
[[aircraft]]
id = "t1"
[aircraft.source]
f0 = {F0}
harmonics = {harmonics}
[aircraft.straight]
heading_deg = 90.0
speed_mps = {SPEED}
alt_agl_m = {ALT}
cpa_offset_m = 0.0
cpa_time_s = {CPA_T}
"#,
        wind[0], wind[1], wind[2]
    )
}

/// Peak frequency per STFT frame in [lo, hi] Hz, with parabolic interpolation. Returns (centre s, Hz).
fn peak_track(x: &[f32], fs: f64, win_s: f64, hop_s: f64, lo: f64, hi: f64) -> Vec<(f64, f64)> {
    let (n, nfft) = ((win_s * fs) as usize, (4.0 * win_s * fs) as usize);
    let hop = (hop_s * fs) as usize;
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(nfft);
    let hann: Vec<f64> = (0..n).map(|i| (std::f64::consts::PI * i as f64 / n as f64).sin().powi(2)).collect();
    let df = fs / nfft as f64;
    let (b_lo, b_hi) = ((lo / df) as usize, (hi / df) as usize);
    let mut out = Vec::new();
    let mut start = 0;
    while start + n <= x.len() {
        let mut buf = fft.make_input_vec();
        for i in 0..n {
            buf[i] = x[start + i] as f64 * hann[i];
        }
        let mut spec = fft.make_output_vec();
        fft.process(&mut buf, &mut spec).unwrap();
        let p: Vec<f64> = spec.iter().map(|c| c.norm_sqr().max(1e-300).ln()).collect();
        let k = (b_lo..b_hi).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap();
        let delta = 0.5 * (p[k - 1] - p[k + 1]) / (p[k - 1] - 2.0 * p[k] + p[k + 1]);
        out.push(((start as f64 + n as f64 / 2.0) / fs, (k as f64 + delta) * df));
        start += hop;
    }
    out
}

#[test]
fn sim_ridge_matches_cv_closed_form() {
    let r = sim::render_toml(&scenario([0.0; 3], "", "[1.0]")).unwrap();
    let win = 1.0;
    let d = ALT - MIC;
    let p = CvParams { f0: F0, v: SPEED, d_cpa: d, t_cpa_arrive: CPA_T + d / C, c: C };
    let track = peak_track(&r.samples, r.sample_rate as f64, win, 0.5, 50.0, 200.0);
    let mut worst: f64 = 0.0;
    for &(t, f) in track.iter().filter(|(t, _)| (t - p.t_cpa_arrive).abs() < 50.0) {
        worst = worst.max((f - cv::frequency(&p, t)).abs());
    }
    eprintln!("sim vs cv: worst ridge deviation {worst:.4} Hz over {} frames", track.len());
    assert!(worst < 1.0 / win, "worst ridge deviation {worst:.3} Hz exceeds one bin");
}

#[test]
fn sim_ridge_matches_doppler_factors_in_wind() {
    let r = sim::render_toml(&scenario([12.0, -4.0, 0.0], "", "[1.0]")).unwrap();
    let t = &r.truth;
    let ridge = peak_track(&r.samples, r.sample_rate as f64, 1.0, 0.5, 50.0, 200.0);
    let near: Vec<_> = ridge.into_iter().filter(|(s, _)| (s - CPA_T).abs() < 50.0).collect();
    let times: Vec<f64> = near.iter().map(|(s, _)| t.start_unix + s).collect();
    let worst = |atm: Atmosphere| {
        let k = dp_core::physics::doppler_factors(&t.tracks[0], t.observer.mic_ecef, &times, &atm);
        near.iter().zip(&k).map(|(&(_, f), &ki)| (f - F0 * ki).abs()).fold(0.0, f64::max)
    };
    let (windy, still) = (worst(Atmosphere { c: C, wind: t.wind_ecef }), worst(Atmosphere { c: C, wind: Vec3::ZERO }));
    eprintln!("sim vs doppler_factors: worst {windy:.4} Hz with wind, {still:.4} Hz ignoring it");
    assert!(windy < 0.1 && still > 3.0 * windy);
}

#[test]
fn sim_truth_cpa_matches_physics_cpa() {
    for wind in [[0.0; 3], [6.0, -3.0, 0.0]] {
        let r = sim::render_toml(&scenario(wind, "[ground]\nreflection = 0.8", "[1.0, 0.5]")).unwrap();
        let t = &r.truth;
        let atm = Atmosphere { c: t.c, wind: t.wind_ecef };
        let c = cpa(&t.tracks[0], t.observer.mic_ecef, &atm).unwrap();
        let e = &t.events[0];
        assert!((c.t_arrive - e.cpa_arrival_unix).abs() < 1e-3, "{} vs {}", c.t_arrive, e.cpa_arrival_unix);
        assert!((c.slant_m - e.slant_m).abs() < 1e-3);
        assert!((c.slant_m - (ALT - MIC)).abs() < 0.5, "tangent-plane straight line: slant {}", c.slant_m);
        assert!(e.in_recording);
    }
}

#[test]
fn sim_is_deterministic_and_bounded() {
    let s = scenario([0.0; 3], "[ground]\nreflection = 0.8", "[1.0, 0.5, 0.25]");
    let (a, b) = (sim::render_toml(&s).unwrap(), sim::render_toml(&s).unwrap());
    assert_eq!(a.samples, b.samples);
    assert!(a.samples.iter().all(|x| x.abs() <= 1.0));
    let peak = a.samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((peak - 10f32.powf(-1.0 / 20.0)).abs() < 1e-4);
    assert_eq!(a.truth.tracks[0].points[0].vel, a.truth.tracks[0].points[1].vel);
    assert!(a.truth.wind_ecef == Vec3::ZERO);
}

#[test]
fn scenario_examples_parse() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../experiments/sim");
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "toml") {
            sim::Scenario::from_toml_str(&std::fs::read_to_string(&path).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            n += 1;
        }
    }
    assert!(n >= 3);
}
