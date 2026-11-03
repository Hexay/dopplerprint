//! `[chain]` integration through `sim::render`: default equivalence, channel modes, truth outputs and
//! ground-reflection coherence loss.

use dp_core::sim;
use realfft::RealFftPlanner;

fn scenario(chain: &str, f0: f64, reflection: f64) -> String {
    format!(
        r#"
seed = 5
sample_rate = 4000
duration_s = 90.0
start_unix = 1725827400.0
[observer]
lat_deg = 40.0
lon_deg = -100.0
h_ell_m = 500.0
mic_height_m = 4.0
[ground]
reflection = {reflection}
[noise]
snr_db = 40.0
hum_hz = 60.0
[device]
highpass_hz = 40.0
[[aircraft]]
id = "t1"
[aircraft.source]
f0 = {f0}
harmonics = [1.0, 0.5]
jitter_frac = 0.001
[aircraft.straight]
heading_deg = 90.0
speed_mps = 70.0
alt_agl_m = 400.0
cpa_offset_m = 300.0
cpa_time_s = 45.0
{chain}
"#
    )
}

#[test]
fn empty_chain_table_renders_bit_identical_samples() {
    let a = sim::render_toml(&scenario("", 100.0, 0.7)).unwrap();
    let b = sim::render_toml(&scenario("[chain]\n[chain.device]\nhighpass_order = 2", 100.0, 0.7)).unwrap();
    assert_eq!(a.samples, b.samples);
    assert!(a.truth.chain.is_none() && b.truth.chain.is_some());
    assert!(!a.truth.to_json().unwrap().contains("\"chain\""), "chain-free truth.json must be unchanged");
}

#[test]
fn dual_mono_is_bit_identical_and_stereo_is_not() {
    let d = sim::render_toml(&scenario("[chain.device]\nchannels = \"dual_mono\"", 100.0, 0.7)).unwrap();
    let [l, r] = d.stereo.as_ref().unwrap();
    assert!(l == r && *l == d.samples);
    let s = sim::render_toml(&scenario("[chain.device]\nchannels = \"stereo\"\nstereo_baseline_m = 0.5", 100.0, 0.7))
        .unwrap();
    let [l, r] = s.stereo.as_ref().unwrap();
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (*x as f64) * (*y as f64)).sum::<f64>();
    let corr = dot(l, r) / (dot(l, l) * dot(r, r)).sqrt();
    assert!(corr > 0.5 && corr < 0.9999, "L/R correlation {corr}");
}

#[test]
fn chain_truth_carries_enf_lines_wind_and_applied_drift() {
    let chain = r#"
[chain.hum]
[[chain.lines]]
hz = 58.25
tnr_db = 3.0
[chain.wind]
fraction = 0.2
[chain.source]
drift_pct_per_min_sd = 0.5
step_prob = 1.0
step_pct_sd = 1.0
"#;
    let r = sim::render_toml(&scenario(chain, 100.0, 0.7)).unwrap();
    let c = r.truth.chain.as_ref().unwrap();
    let hum = c.hum.as_ref().unwrap();
    assert_eq!(hum.enf_hz.len(), 92);
    assert!(hum.enf_hz.iter().all(|f| (f - 60.0).abs() < 0.1));
    assert_eq!(c.lines[0].hz, 58.25);
    assert!(!c.wind.as_ref().unwrap().gusts.is_empty());
    let s = &c.sources[0];
    assert!(s.drift_per_s != 0.0 && s.step_frac != 0.0 && (0.0..=90.0).contains(&s.step_time_s));
    let dir = tempfile::tempdir().unwrap();
    sim::write_outputs(&r, dir.path()).unwrap();
    let enf = std::fs::read_to_string(dir.path().join("enf.csv")).unwrap();
    assert_eq!(enf.lines().count(), 93);
}

/// Per-frame peak power (dB) of the band [lo, hi] Hz, 0.5 s frames, over [t0, t1].
fn band_peak_db(x: &[f32], fs: f64, lo: f64, hi: f64, t0: f64, t1: f64) -> Vec<f64> {
    let n = (0.5 * fs) as usize;
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(n);
    let df = fs / n as f64;
    (((t0 * fs) as usize..(t1 * fs) as usize - n).step_by(n))
        .map(|s| {
            let mut buf: Vec<f64> = (0..n).map(|i| x[s + i] as f64).collect();
            let mut spec = fft.make_output_vec();
            fft.process(&mut buf, &mut spec).unwrap();
            let k = ((lo / df) as usize..(hi / df) as usize).map(|k| spec[k].norm_sqr()).fold(0.0, f64::max);
            10.0 * k.log10()
        })
        .collect()
}

#[test]
fn ground_coherence_loss_fills_lloyds_mirror_fringes() {
    let spread = |chain: &str| {
        let r = sim::render_toml(&scenario(chain, 300.0, 0.95)).unwrap();
        let p = band_peak_db(&r.samples, 4000.0, 200.0, 420.0, 20.0, 70.0);
        let m = p.iter().sum::<f64>() / p.len() as f64;
        (p.iter().map(|v| (v - m).powi(2)).sum::<f64>() / p.len() as f64).sqrt()
    };
    let rigid = spread("");
    let turbulent = spread("[chain.path]\nground_coherence = 0.0\nturbulence_hz = 1.0");
    assert!(turbulent < 0.8 * rigid, "fringe spread rigid {rigid:.2} dB vs incoherent {turbulent:.2} dB");
}
