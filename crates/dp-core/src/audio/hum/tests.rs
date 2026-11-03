use std::f64::consts::TAU;

use super::*;

struct Rng(u64);

impl Rng {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn gauss(&mut self) -> f64 {
        (-2.0 * (1.0 - self.uniform()).ln()).sqrt() * (TAU * self.uniform()).cos()
    }
}

struct Hum {
    nominal: f64,
    wander: f64,
    amps: Vec<(u32, f64)>,
    noise: f64,
    seed: u64,
}

impl Hum {
    fn sixty() -> Self {
        Self { nominal: 60.0, wander: 0.02, amps: vec![(1, 0.01), (2, 0.006), (3, 0.004)], noise: 0.05, seed: 7 }
    }

    /// Grid frequency at time t: two slow sinusoids, peak excursion ≈ `wander`.
    fn freq(&self, t: f64) -> f64 {
        self.nominal + self.wander * (0.7 * (TAU * t / 37.0).sin() + 0.3 * (TAU * t / 11.0 + 1.0).sin())
    }

    /// Samples taken at `true_sr` (the stream the device really produced).
    fn render(&self, true_sr: f64, dur: f64) -> Vec<f32> {
        let mut rng = Rng(self.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut ph = 0.0;
        (0..(dur * true_sr) as usize)
            .map(|i| {
                ph += TAU * self.freq(i as f64 / true_sr) / true_sr;
                let s: f64 = self.amps.iter().map(|&(k, a)| a * (k as f64 * ph + 0.3 * k as f64).sin()).sum();
                (s + self.noise * rng.gauss()) as f32
            })
            .collect()
    }
}

fn audio(samples: Vec<f32>, sr: u32) -> Audio {
    Audio { sample_rate: sr, samples }
}

#[test]
fn trace_tracks_wandering_60hz_with_harmonics() {
    // Per-frame scatter is noise-limited (≈2× the 2 s-frame CRB); the long-run mean must stay unbiased.
    for (noise, rms_max) in [(0.05, 0.03), (0.005, 0.003)] {
        trace_case(&Hum { noise, ..Hum::sixty() }, rms_max);
    }
}

fn trace_case(h: &Hum, rms_max: f64) {
    let r = analyse_hum(&audio(h.render(4000.0, 120.0), 4000), &HumParams::default());
    assert!(r.present, "{:?}", r.notes);
    assert_eq!(r.nominal_hz, Some(60.0));
    assert_eq!(r.rate.status, RateStatus::Ok);
    assert!(!r.rate.drift_flag);
    assert!(r.detect_frac > 0.95, "frac {}", r.detect_frac);
    let ks: Vec<u32> = r.harmonics.iter().map(|l| l.k).collect();
    assert_eq!(ks, [1, 2, 3]);
    let err: Vec<f64> = r
        .trace
        .iter()
        .filter(|f| f.valid)
        .map(|f| {
            let truth = (0..=200).map(|i| h.freq(f.t_s - 1.0 + i as f64 / 100.0)).sum::<f64>() / 201.0;
            f.f_hz - truth
        })
        .collect();
    let n = err.len() as f64;
    let rms = (err.iter().map(|e| e * e).sum::<f64>() / n).sqrt();
    let bias = err.iter().sum::<f64>() / n;
    assert!(rms < rms_max, "trace rms error {rms} Hz");
    assert!(bias.abs() < 0.2 * rms_max, "trace bias {bias} Hz");
    assert!((r.fundamental_hz - 60.0).abs() < 0.005, "mean {}", r.fundamental_hz);
    assert!(r.discontinuities.is_empty(), "{:?}", r.discontinuities);
    assert!(r.bands.iter().any(|b| b.lo_hz < 120.0 && b.hi_hz > 120.0));
    let back: HumReport = serde_json::from_str(&r.to_json().unwrap()).unwrap();
    assert_eq!((back.present, back.trace.len(), back.rate.status), (r.present, r.trace.len(), r.rate.status));
}

#[test]
fn weak_hum_falls_back_to_long_frames() {
    let h = Hum { noise: 0.2, ..Hum::sixty() };
    let r = analyse_hum(&audio(h.render(4000.0, 240.0), 4000), &HumParams::default());
    assert!(r.present, "{:?} frac {}", r.notes, r.detect_frac);
    assert_eq!(r.frame_s, 8.0);
    assert_eq!(r.rate.status, RateStatus::Ok);
    assert!((r.fundamental_hz - 60.0).abs() < 0.01, "mean {}", r.fundamental_hz);
}

#[test]
fn missing_fundamental_still_gives_nominal() {
    let h = Hum { amps: vec![(2, 0.008), (3, 0.005)], ..Hum::sixty() };
    let r = analyse_hum(&audio(h.render(4000.0, 60.0), 4000), &HumParams::default());
    assert!(r.present, "{:?}", r.notes);
    assert_eq!(r.nominal_hz, Some(60.0));
    assert!((r.fundamental_hz - 60.0).abs() < 0.03);
}

#[test]
fn mislabelled_rate_is_detected_with_implied_rate() {
    // (grid, true container rate, labelled container rate); audio is a 10× decimated stand-in.
    for (nominal, true_sr, label_sr) in [(60.0, 44100u32, 48000u32), (50.0, 48000, 44100)] {
        let h = Hum { nominal, ..Hum::sixty() };
        let x = h.render(true_sr as f64 / 10.0, 60.0);
        let p = HumParams { labelled_sr: Some(label_sr), ..Default::default() };
        let r = analyse_hum(&audio(x, label_sr / 10), &p);
        assert!(r.present, "{:?}", r.notes);
        assert_eq!(r.nominal_hz, Some(nominal));
        assert_eq!(r.rate.status, RateStatus::Mislabelled, "{:?}", r.rate);
        assert_eq!(r.rate.likely_sr, Some(true_sr));
        assert!((r.rate.implied_sr / true_sr as f64 - 1.0).abs() < 1e-3, "{:?}", r.rate);
        let apparent = nominal * label_sr as f64 / true_sr as f64;
        assert!((r.fundamental_hz - apparent).abs() < 0.05, "{} vs {apparent}", r.fundamental_hz);
        assert_eq!(r.notch_fundamental(), Some(r.fundamental_hz));
    }
}

#[test]
fn mislabel_on_fundamental_alone_is_flagged_but_ambiguous() {
    let h = Hum { amps: vec![(1, 0.01)], noise: 0.02, ..Hum::sixty() };
    let p = HumParams { labelled_sr: Some(48000), ..Default::default() };
    let r = analyse_hum(&audio(h.render(4410.0, 60.0), 4800), &p);
    assert!(r.present, "{:?}", r.notes);
    assert_eq!(r.rate.status, RateStatus::Mislabelled);
    assert_eq!(r.rate.likely_sr, Some(44100));
    assert!(r.ambiguous);
}

#[test]
fn clock_offset_is_not_called_a_mislabel() {
    let h = Hum::sixty();
    let x = h.render(4000.0 * 1.003, 60.0);
    let r = analyse_hum(&audio(x, 4000), &HumParams::default());
    assert!(r.present, "{:?}", r.notes);
    assert_eq!(r.rate.status, RateStatus::ClockOffset, "{:?}", r.rate);
    assert!((r.rate.deviation_ppm + 2991.0).abs() < 400.0, "{:?}", r.rate);
}

#[test]
fn inserted_gap_is_a_dropout() {
    let mut x = Hum::sixty().render(4000.0, 120.0);
    x.splice(240_000..240_000, std::iter::repeat_n(0.0f32, 2000));
    let r = analyse_hum(&audio(x, 4000), &HumParams::default());
    assert!(r.present, "{:?}", r.notes);
    let d: Vec<_> = r.discontinuities.iter().filter(|d| d.kind == DiscontinuityKind::Dropout).collect();
    assert_eq!(d.len(), 1, "{:?}", r.discontinuities);
    assert!((d[0].t_s - 60.0).abs() < 0.15 && (d[0].t_end_s - 60.5).abs() < 0.15, "{:?}", d[0]);
}

#[test]
fn removed_quarter_cycle_is_a_phase_jump() {
    // 1250 samples at 4 kHz = 18.75 cycles of 60 Hz: the hum resumes a quarter cycle early.
    let mut x = Hum { noise: 0.02, ..Hum::sixty() }.render(4000.0, 90.0);
    x.drain(160_000..161_250);
    let r = analyse_hum(&audio(x, 4000), &HumParams::default());
    assert!(r.present, "{:?}", r.notes);
    assert_eq!(r.discontinuities.len(), 1, "{:?}", r.discontinuities);
    let d = &r.discontinuities[0];
    assert_eq!(d.kind, DiscontinuityKind::PhaseJump);
    assert!((d.t_s - 40.0).abs() < 0.2, "{d:?}");
    let k = r.harmonics.iter().max_by(|a, b| a.snr_db.total_cmp(&b.snr_db)).unwrap().k as f64;
    let c = d.jump_cycles.unwrap() * k;
    let frac = (0.25 * k - c.abs()).rem_euclid(1.0);
    assert!(frac.min(1.0 - frac) < 0.08, "jump {c} cycles at harmonic {k}");
}

#[test]
fn noise_and_a_moving_tone_are_not_hum() {
    for seed in 1..=5u64 {
        let mut rng = Rng(seed * 0x2545_F491_4F6C_DD1D);
        let (sr, dur) = (4000.0, 60.0);
        let mut red = 0.0;
        let mut ph = 0.0;
        let x: Vec<f32> = (0..(sr * dur) as usize)
            .map(|i| {
                let t = i as f64 / sr;
                red += 0.02 * (rng.gauss() - red);
                ph += TAU * (130.0 - 0.4 * t) / sr;
                (0.05 * rng.gauss() + 0.5 * red + 0.01 * ph.sin()) as f32
            })
            .collect();
        let r = analyse_hum(&audio(x, 4000), &HumParams::default());
        assert!(!r.present, "seed {seed}: false hum {:?} {:?}", r.harmonics, r.notes);
        assert_eq!(r.rate.status, RateStatus::Unknown);
        assert!(r.bands.is_empty());
    }
}

#[test]
fn front_end_auto_mains_notches_the_apparent_fundamental() {
    let a = audio(Hum::sixty().render(4410.0, 60.0), 4800);
    let off = crate::audio::FrontEnd::default();
    assert_eq!(off.resolved_mains_hz(&a), None);
    let on = crate::audio::FrontEnd { auto_mains: true, ..Default::default() };
    let f = on.resolved_mains_hz(&a).expect("hum found");
    assert!((f - 60.0 * 4800.0 / 4410.0).abs() < 0.05, "{f}");
}

#[test]
fn silence_and_short_clips_do_not_panic() {
    let p = HumParams::default();
    assert!(!analyse_hum(&audio(vec![0.0; 40_000], 4000), &p).present);
    assert!(!analyse_hum(&audio(vec![0.1; 100], 4000), &p).present);
    assert!(!analyse_hum(&audio(Vec::new(), 4000), &p).present);
}

#[test]
fn rate_ratios_include_the_44k1_48k_pair() {
    let r = grid::rate_ratios();
    assert!(r.iter().any(|&x| (x - 44100.0 / 48000.0).abs() < 1e-12));
    assert!(r.iter().any(|&x| (x - 48000.0 / 44100.0).abs() < 1e-12));
    assert!(r.contains(&1.0));
}
